use std::net::SocketAddr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use trust_dns_resolver::name_server::GenericConnector;
use url::Url;

use tokio_native_tls::TlsConnector as TokioTlsConnector;
use x509_parser::parse_x509_certificate;

use super::prelude::*;
use trust_dns_resolver::AsyncResolver;

async fn measure_dns_time(host: &str, resolver: &AsyncResolver<GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>>) -> Option<f64> {

    let start = Instant::now();
    let _ = resolver.lookup_ip(host).await;
    let dur = start.elapsed().as_secs_f64();
    Some(dur)
}

async fn get_cert_validity_days(host: &str, connector: &TokioTlsConnector, resolver: &AsyncResolver<GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>>) -> Option<i64> {
    let ip = resolver.lookup_ip(host).await.ok()?.iter().next()?;
    let socket_addr = SocketAddr::new(ip, 443);

    let stream = tokio::net::TcpStream::connect(socket_addr).await.ok()?;


    let tls_stream = connector.connect(host, stream).await.ok()?;

    // Extract certificate in blocking context
    let cert_der = tokio::task::spawn_blocking(move || {
        let cert = tls_stream.get_ref().peer_certificate().ok().flatten()?;
        cert.to_der().ok()
    })
    .await
    .ok()??;

    // Parse the certificate (fast, but still sync)
    let (_, parsed) = parse_x509_certificate(&cert_der).ok()?;

    let not_after = parsed.validity().not_after.timestamp();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;

    Some((not_after - now) / 86400)
}

pub async fn probe_url(client: reqwest::Client, connector: &TokioTlsConnector, resolver: &AsyncResolver<GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>>, url: &str) -> Result<ProbeResult, String> {
    // Simulate HTTP request and response (replace with actual HTTP client logic)
    let url = url.to_string();
    let client = client.clone();

    let parsed_url = Url::parse(&url).ok();
    let host = parsed_url
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or_default()
        .to_string();

    // Measure DNS resolution time
    let dns_duration = measure_dns_time(&host, resolver).await;

    // Measure HTTP probe
    let start = Instant::now();
    let status_result = client.get(&url).send().await;
    let http_duration = start.elapsed().as_secs_f64();
    let http_status = match &status_result {
        Ok(resp) => Some(resp.status().as_u16()),
        Err(_) => None,
    };

    // Measure certificate validity days
    let cert_validity_days = match &status_result {
        Ok(resp) => {
            if url.starts_with("http://") || (url.starts_with("https://")) && resp.version() == reqwest::Version::HTTP_09 {
                None // HTTP/0.9 does not support TLS
            } else {
                get_cert_validity_days(&host, connector, resolver).await
            }
        },
        Err(_) => None,
    };

    let http_version = match &status_result {
        Ok(resp) => Some(match resp.version() {
            reqwest::Version::HTTP_09 => "HTTP/0.9".to_string(),
            reqwest::Version::HTTP_10 => "HTTP/1.0".to_string(),
            reqwest::Version::HTTP_11 => "HTTP/1.1".to_string(),
            reqwest::Version::HTTP_2 => "HTTP/2.0".to_string(),
            reqwest::Version::HTTP_3 => "HTTP/3.0".to_string(),
            _ => "UNKNOWN".to_string(),
        }),
        Err(_) => None,
    };

    Ok(ProbeResult {
        url: url.to_string(),
        dns_time: dns_duration,
        cert_validity_days,
        http_time: http_duration,
        http_status,
        http_version,
    })
}
