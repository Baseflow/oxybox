use config::model::Config;
use reqwest::Client;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::time::sleep;
use tokio_native_tls::TlsConnector as TokioTlsConnector;
use trust_dns_resolver::AsyncResolver;

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
    let config_str = std::fs::read_to_string("config.yml").expect("Failed to read config.yaml");
    let config: Config = serde_yaml::from_str(&config_str).expect("Invalid YAML");

    let mut builder = native_tls::TlsConnector::builder();
    builder.danger_accept_invalid_certs(true);

    let tls_connector = builder.build().expect("Failed to build TLS connector");
    let tls_connector = TokioTlsConnector::from(tls_connector);
    let resolver = AsyncResolver::tokio_from_system_conf().expect("DNS resolver failed");
    
    let max_org_width = config
        .keys()
        .map(|org| org.len())
        .max()
        .unwrap_or(10); // fallback to 10 if empty



    for (key, org_config) in config {
        let tls_connector = TokioTlsConnector::from(tls_connector.clone());
        let resolver = resolver.clone();
        let targets = org_config.targets.clone();
        let interval = org_config.polling_interval_seconds;

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
                    let organisation_id = key.clone();

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
                                let organisation_id = to_fixed_width(&organisation_id, max_org_width);
                                if is_accepted {
                                    println!(
                                        "[{organisation_id}] ✅ URL: {}, Status: {:?}, Elapsed: {:.2}ms, Cert: {}",
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
                                        "[{organisation_id}] ❌ Unexpected status for {url}: {:?} (accepted: {:?})",
                                        result.http_status, target.accepted_status_codes
                                    );
                                }

                                let metrics = create_probe_metrics(&result);
                                if let Err(e) = send_to_mimir(
                                    "http://localhost:9009",
                                    Some(&organisation_id),
                                    metrics,
                                )
                                .await
                                {
                                    println!(
                                        "[{organisation_id}] Failed to send metrics for {url}: {e}"
                                    );
                                }
                            }
                            Err(e) => {
                                println!("[{organisation_id}] ❌ Probe error for {url}: {e}");
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
