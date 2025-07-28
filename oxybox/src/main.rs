use native_tls::TlsConnector;
use reqwest::Client;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio_native_tls::TlsConnector as TokioTlsConnector;

use tokio::{task, time::sleep};
use trust_dns_resolver::AsyncResolver;

pub mod mimir;
use mimir::{client::send_to_mimir, create_probe_metrics};
pub mod http_probe;
use http_probe::prelude::*;

#[tokio::main]
async fn main() {
    let endpoints = vec![
        // "https://www.google.com",
        // "https://www.github.com",
        // "https://expired.badssl.com",
        // "http://example.com",
        "https://kloosterdolphia.nl",
    ];

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("reqwest-h2-h3-probe/1.0")
        .build()
        .expect("Failed to create HTTP client");

    let connector = TlsConnector::new().expect("Failed to create TLS connector");
    let connector = TokioTlsConnector::from(connector);
    let resolver = AsyncResolver::tokio_from_system_conf().expect("Failed to create DNS resolver");

    loop {
        println!("--- Checking endpoints ---");

        let mut handles = vec![];

        for url in &endpoints {
            let url = url.to_string();
            let client = client.clone();
            let connector = connector.clone();
            let resolver = resolver.clone();

            let handle = task::spawn(async move {
                match probe_url(client, &connector, &resolver, &url).await {
                    Ok(result) => {
                        let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("cannot determine now").as_secs() as f64;
                        println!(
                            "URL: {}, Status: {:?}, DNS Time: {:.2} ms, Cert Validity Days: {:?}, Elapsed: {:.2} ms, HTTP Version: {:?}",
                            url,
                            result.http_status,
                            result.dns_time.map_or(0.0, |d| d * 1000.0),
                            result
                                .cert_validity_seconds
                                .map_or("N/A".to_string(), |d| format!(
                                    "{:.2} days",
                                    (d - now) / 86400.0
                                )),
                            result.total_probe_time * 1000.0,
                            result.http_version,
                        );
                        let metrics = create_probe_metrics(&result);
                        if let Err(e) =
                            send_to_mimir("http://localhost:9009", Some("demo"), metrics).await
                        {
                            println!("Failed to send metrics for {url}: {e}");
                        } else {
                            println!("Metrics sent successfully for {url}");
                        }
                    }
                    Err(e) => {
                        println!("Error probing {url}: {e}");
                    }
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        sleep(Duration::from_secs(10)).await;
    }
}
