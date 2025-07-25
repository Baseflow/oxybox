use native_tls::TlsConnector;
use reqwest::Client;
use std::time::Duration;

use tokio_native_tls::TlsConnector as TokioTlsConnector;

use tokio::{task, time::sleep};

pub mod mimir;
pub mod probe;
use probe::prelude::*;

#[tokio::main]
async fn main() {
    let endpoints = vec![
        "https://www.google.com",
        "https://www.github.com",
        "https://expired.badssl.com",
        "http://example.com",
    ];

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create HTTP client");

    let connector = TlsConnector::new().ok().expect("Failed to create TLS connector");
    let connector = TokioTlsConnector::from(connector);

    loop {
        println!("--- Checking endpoints ---");

        let mut handles = vec![];

        for url in &endpoints {
            let url = url.to_string();
            let client = client.clone();
            let connector = connector.clone();

            let handle = task::spawn(async move {
                match probe_url(client, &connector, &url).await {
                    Ok(result) => {
                        println!(
                            "URL: {}, Status: {:?}, DNS Time: {:.2} ms, Cert Validity Days: {:?}, Elapsed: {:.2} ms",
                            url,
                            result.http_status,
                            result.dns_time.map_or(0.0, |d| d * 1000.0),
                            result.cert_validity_days,
                            result.http_time * 1000.0
                        );
                    }
                    Err(e) => {
                        println!("Error probing {}: {}", url, e);
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

