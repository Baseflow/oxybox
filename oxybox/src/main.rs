use config::model::Config;
use reqwest::Client;
use std::{net::IpAddr, time::{Duration, SystemTime, UNIX_EPOCH}};

use tokio::time::sleep;
use tokio_native_tls::TlsConnector as TokioTlsConnector;
use trust_dns_resolver::{
    TokioAsyncResolver,
    config::{NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig, ResolverOpts},
};

pub mod mimir;
use mimir::{client::send_to_mimir, create_probe_metrics};
pub mod http_probe;
use http_probe::prelude::*;
pub mod config;

fn to_fixed_width(input: &str, width: usize) -> String {
    use unicode_truncate::UnicodeTruncateStr;

    let (truncated, _) = input.unicode_truncate(width);
    format!("{:<width$}", truncated, width = width)
}

#[tokio::main]
async fn main() {
    let config_file_location =
        std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yml".to_string());
    let config_str =
        std::fs::read_to_string(config_file_location).expect("Failed to read config.yaml");
    let dns_hosts = std::env::var("DNS_HOSTS").unwrap_or_else(|_| "1.1.1.1, 8.8.8.8".to_string());

    let dns_host_vec: Vec<&str> = dns_hosts.split(',').map(str::trim).collect();

    let config: Config = serde_yaml::from_str(&config_str).expect("Invalid YAML");

    let mut builder = native_tls::TlsConnector::builder();
    builder.danger_accept_invalid_certs(true);

    let tls_connector = builder.build().expect("Failed to build TLS connector");
    let tls_connector = TokioTlsConnector::from(tls_connector);
    let mut opts = ResolverOpts::default();
    opts.attempts = 2;
    opts.timeout = std::time::Duration::from_millis(100);
    opts.cache_size = 1024;

    let mut name_servers = NameServerConfigGroup::new();

    for host in dns_host_vec {
        let ip: IpAddr = host.parse().expect("Invalid DNS host");
        let config = NameServerConfig {
            socket_addr: (ip, 53).into(),
            protocol: Protocol::Tcp, // TCP is more reliable then UDP for DNS queries
            tls_dns_name: None,
            trust_negative_responses: false,
            bind_addr: None,
        };
        name_servers.push(config);
    }
    let resolver_config = ResolverConfig::from_parts(None, vec![], name_servers);

    let resolver = TokioAsyncResolver::tokio(resolver_config, opts);

    let max_org_width = config.keys().map(|org| org.len()).max().unwrap_or(10); // fallback to 10 if empty

    let mimir_endpoint =
        std::env::var("MIMIR_ENDPOINT").unwrap_or_else(|_| "http://localhost:9009".to_string());
    println!("Using Mimir endpoint: {}", mimir_endpoint);

    for (key, org_config) in config {
        let tls_connector = TokioTlsConnector::from(tls_connector.clone());
        let resolver = resolver.clone();
        let targets = org_config.targets.clone();
        let interval = org_config.polling_interval_seconds;
        let mimir_target = mimir_endpoint.clone();

        tokio::spawn(async move {
            let client = Client::builder()
                .timeout(Duration::from_secs(5))
                .danger_accept_invalid_certs(true)
                .user_agent("reqwest-h2-h3-probe/1.0")
                .build()
                .expect("Failed to create client");

            loop {
                let mut handles = vec![];

                for target in &targets {
                    let client = client.clone();
                    let connector = tls_connector.clone();
                    let resolver = resolver.clone();
                    let target = target.clone();
                    let tenant_name = key.clone();
                    let organisation_id = org_config.organisation_id.clone();
                    let mimir_target = mimir_target.clone();

                    let handle = tokio::spawn(async move {
                        let url = target.url.clone();
                        match probe_url(client, &connector, &resolver, &url).await {
                            Ok(result) => {
                                let is_accepted = result
                                    .http_status
                                    .map(|code| target.accepted_status_codes.contains(&code))
                                    .unwrap_or(false);

                                let now = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .expect("Cannot determine time")
                                    .as_secs_f64();
                                let tenant_name = to_fixed_width(&tenant_name, max_org_width);
                                if is_accepted {
                                    println!(
                                        "[{tenant_name}] ✅ URL: {}, Status: {:?}, Elapsed: {:.2}ms, Cert: {}",
                                        url,
                                        result.http_status,
                                        result.total_probe_time * 1000.0,
                                        result
                                            .cert_validity_seconds
                                            .map(|d| format!("{:.2}d", (d - now) / 86400.0))
                                            .unwrap_or_else(|| "N/A".to_string())
                                    );
                                } else {
                                    println!(
                                        "[{tenant_name}] ❌ Unexpected status for {url}: {:?} (accepted: {:?})",
                                        result.http_status, target.accepted_status_codes
                                    );
                                }

                                let metrics = create_probe_metrics(&result, is_accepted);
                                if let Err(e) =
                                    send_to_mimir(&mimir_target, Some(&organisation_id), metrics)
                                        .await
                                {
                                    println!(
                                        "[{tenant_name}] Failed to send metrics for {url}: {e}"
                                    );
                                }
                            }
                            Err(e) => {
                                println!("[{tenant_name}] ❌ Probe error for {url}: {e}");
                            }
                        }
                    });

                    handles.push(handle);
                }

                for handle in handles {
                    let _ = handle.await;
                }

                sleep(Duration::from_secs(interval)).await;
            }
        });
    }

    // Keep main thread alive
    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
