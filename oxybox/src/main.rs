use reqwest::Client;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use native_tls::TlsConnector;
use tokio::{task, time::sleep};
use tokio_native_tls::TlsConnector as TokioTlsConnector;
use tokio::net::lookup_host;
use url::Url;
use x509_parser::parse_x509_certificate;

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

    loop {
        println!("--- Checking endpoints ---");

        let mut handles = vec![];

        for url in &endpoints {
            let url = url.to_string();
            let client = client.clone();

            let handle = task::spawn(async move {
                let parsed_url = Url::parse(&url).ok();
                let host = parsed_url
                    .as_ref()
                    .and_then(|u| u.host_str())
                    .unwrap_or_default()
                    .to_string();

                // Measure DNS resolution time
                let dns_duration = measure_dns_time(&host).await;

                // Measure HTTP probe
                let start = Instant::now();
                let status_result = client.get(&url).send().await;
                let http_duration = start.elapsed().as_secs_f64();

                match status_result {
                    Ok(resp) => {
                        let status = resp.status();
                        let version = match resp.version() {
                            reqwest::Version::HTTP_09 => "HTTP/0.9",
                            reqwest::Version::HTTP_10 => "HTTP/1.0",
                            reqwest::Version::HTTP_11 => "HTTP/1.1",
                            reqwest::Version::HTTP_2 => "HTTP/2.0",
                            reqwest::Version::HTTP_3 => "HTTP/3.0",
                            _ => "UNKNOWN",
                        };

                        let dns_info = dns_duration
                            .map(|d| format!("{:.3} sec", d))
                            .unwrap_or_else(|| "FAILED".into());

                        println!(
                            "[UP]     {:<40} ({}) {:<8} DNS: {:<10} HTTP: {:.3} sec",
                            url, status, version, dns_info, http_duration
                        );
                    }
                    Err(e) => {
                        let dns_info = dns_duration
                            .map(|d| format!("{:.3} sec", d))
                            .unwrap_or_else(|| "FAILED".into());

                        println!(
                            "[ERROR]  {:<40} ({}) DNS: {:<10} HTTP: {:.3} sec",
                            url, e, dns_info, http_duration
                        );
                    }
                }

                // TLS Cert Expiration
                if url.starts_with("https://") {
                    if let Some(hostname) = url.strip_prefix("https://").and_then(|u| u.split('/').next()) {
                        match get_cert_validity_days(hostname).await {
                            Some(days) => {
                                if days < 0 {
                                    println!("         SSL cert expired {} days ago", -days);
                                } else {
                                    println!("         SSL cert valid for {} days", days);
                                }
                            }
                            None => println!("         Failed to read SSL cert"),
                        }
                    }
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        println!();
        sleep(Duration::from_secs(10)).await;
    }
}

async fn measure_dns_time(host: &str) -> Option<f64> {
    let start = Instant::now();
    let result = lookup_host((host, 443)).await;
    let duration = start.elapsed().as_secs_f64();

    match result {
        Ok(_) => Some(duration),
        Err(_) => None,
    }
}

async fn get_cert_validity_days(host: &str) -> Option<i64> {
    let addr = format!("{}:443", host);
    let socket_addr = addr.to_socket_addrs().ok()?.next()?;

    let stream = tokio::net::TcpStream::connect(socket_addr).await.ok()?;

    let connector = TlsConnector::builder().build().ok()?;
    let connector = TokioTlsConnector::from(connector);

    let tls_stream = connector.connect(host, stream).await.ok()?;
    let cert = tls_stream.get_ref().peer_certificate().ok().flatten()?;
    let cert_der = cert.to_der().ok()?;
    let (_, parsed) = parse_x509_certificate(&cert_der).ok()?;

    let not_after = parsed.validity().not_after.timestamp();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;

    Some((not_after - now) / 86400) // Convert to days
}
