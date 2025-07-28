use std::net::SocketAddr;
use std::time::Instant;

use trust_dns_resolver::name_server::GenericConnector;
use url::Url;

use tokio_native_tls::TlsConnector as TokioTlsConnector;
use x509_parser::parse_x509_certificate;

use super::prelude::*;
use trust_dns_resolver::AsyncResolver;

struct HttpProbeResult {
    dns_time: Option<f64>,
    cert_validity_seconds: Option<f64>,
    connect_time: Option<f64>,
    tls_time: Option<f64>,
}

async fn get_connect_timings(host: &str, connector: &TokioTlsConnector, resolver: &AsyncResolver<GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>>, with_tls: bool) -> Result<HttpProbeResult, String> {
    let dns_start = Instant::now();
    if host.is_empty() {
        return Err("Host is empty".to_string());
    }
    let ip = match resolver.lookup_ip(host).await {
        Ok(lookup) => {
            // Use the first IP address from the lookup result
            lookup.iter().next().ok_or_else(|| format!("No IP addresses found for host {host}"))
        },
        Err(e) => return Err(format!("DNS resolution failed for host {host}: {e}")),
    };
    let dns_time = Some(dns_start.elapsed().as_secs_f64());
    let connect_start = Instant::now();
    let socket_addr = match with_tls {
        // true connect to port 80, otherwise port 443
        true => SocketAddr::new(ip?, 443),
        false => SocketAddr::new(ip?, 80),
    };
    let stream = tokio::net::TcpStream::connect(socket_addr).await;
    let stream = match stream {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to connect to host {host}: {e}")),
    };
    let connect_time = Some(connect_start.elapsed().as_secs_f64());
    if !with_tls {
        return Ok(HttpProbeResult {
            dns_time,
            cert_validity_seconds: None,
            connect_time,
            tls_time: None,
        });
    }

    println!("Establishing TLS connection to {host} on {socket_addr}");
    let tls_start = Instant::now();
    let tls_stream = connector.connect(host, stream).await;
    let (tls_time, cert_validity_seconds) = match tls_stream {
        Ok(tls_stream) => {
            let tls_time = Some(tls_start.elapsed().as_secs_f64());
            // Extract certificate in blocking context
            let cert_der = tokio::task::spawn_blocking(move || {
                let cert = tls_stream.get_ref().peer_certificate().ok().flatten()?;
                cert.to_der().ok()
            })
            .await;

            let cert_der = match cert_der {
                Ok(Some(cert)) => cert,
                _ => return Err(format!("Failed to retrieve certificate for host {host}")),
            };
            
            let parsed = parse_x509_certificate(&cert_der);
            let (_, parsed) = match parsed {
                Ok(cert) => cert,
                Err(e) => return Err(format!("Failed to parse certificate for host {host}: {e}")),
            };
            let not_after = parsed.validity().not_after.timestamp();
            let cert_validity_seconds = Some((not_after) as f64);
            (tls_time, cert_validity_seconds)

        },
        Err(e) => {
            eprintln!("Failed to establish TLS connection for host {host}: {e}");
            return Err(format!("Failed to establish TLS connection for host {host}: {e}"));
        }
    };

    Ok(HttpProbeResult {
        dns_time,
        cert_validity_seconds,
        connect_time,
        tls_time,
    })
}

fn convert_http_version(version: reqwest::Version) -> f64 {
    match version {
        reqwest::Version::HTTP_09 => 0.9,
        reqwest::Version::HTTP_10 => 1.0,
        reqwest::Version::HTTP_11 => 1.1,
        reqwest::Version::HTTP_2 => 2.0,
        reqwest::Version::HTTP_3 => 3.0,
        _ => 0.0, // Default case for unknown versions
    }
}

pub async fn probe_url(client: reqwest::Client, connector: &TokioTlsConnector, resolver: &AsyncResolver<GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>>, url: &str) -> Result<ProbeResult, String> {
    let probe_start = Instant::now();
    // Simulate HTTP request and response (replace with actual HTTP client logic)
    let url = url.to_string();

    let parsed_url = Url::parse(&url).ok();
    let host = parsed_url
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or_default()
        .to_string();

    let probe_result = get_connect_timings(&host, connector, resolver, url.starts_with("https://")).await?;
    // Measure HTTP probe
    let start = Instant::now();
    let status_result = client.get(&url).send().await;
    let (processing_time, transfer_time, http_status, http_version) = match status_result {
        Ok(resp) => {

            // Time to first byte (TTFB)
            let reqwest_send_duration = start.elapsed().as_secs_f64();

            // Calculate processing_time by deducting known connection setup times
            // Use the times from the get_tls_probe_metrics call for deduction.
            // Ensure the result doesn't go negative due to measurement inaccuracies or connection reuse.
            let deducted_time = probe_result.dns_time.unwrap_or(0.0) + probe_result.connect_time.unwrap_or(0.0) + probe_result.tls_time.unwrap_or(0.0);
            let actual_processing_time = (reqwest_send_duration - deducted_time).max(0.0);
            let http_status_val = resp.status().as_u16();
            let http_version_val = convert_http_version(resp.version());

            let transfer_start = Instant::now();
            let _ = resp.bytes().await.map_err(|e| format!("Failed to read body: {e}"))?;
            let transfer_time_val = transfer_start.elapsed().as_secs_f64();
            (
                Some(actual_processing_time),
                Some(transfer_time_val),
                Some(http_status_val),
                Some(http_version_val),
            )
        }
        Err(e) => {
            eprintln!("HTTP request failed for URL {url}: {e}");
            return Err(format!("HTTP request failed for URL {url}: {e}"));
        }
    };

    // Measure certificate validity days
    let total_probe_time = probe_start.elapsed().as_secs_f64();

    Ok(ProbeResult {
        url: url.to_string(),
        dns_time: probe_result.dns_time,
        connect_time: probe_result.connect_time,
        tls_time: probe_result.tls_time,
        processing_time, 
        cert_validity_seconds: probe_result.cert_validity_seconds,
        http_status,
        http_version,
        transfer_time,
        total_probe_time,
    })
}
