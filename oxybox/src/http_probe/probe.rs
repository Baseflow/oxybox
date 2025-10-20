use std::net::SocketAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use tokio::time::sleep;
use trust_dns_resolver::name_server::GenericConnector;
use url::Url;

use tokio_native_tls::TlsConnector as TokioTlsConnector;
use x509_parser::parse_x509_certificate;

use crate::config::probe_config::{OrganisationConfig, TargetConfig};
use crate::http_probe::report;
use crate::mimir::client::send_to_mimir;
use crate::mimir::create_probe_metrics;

use trust_dns_resolver::{AsyncResolver, TokioAsyncResolver};

use super::result::ProbeResult;

/// Struct to hold the results of an HTTP probe.
/// This struct contains various metrics related to the HTTP request, such as DNS resolution time, connection time, TLS handshake time, HTTP status code, and more.
/// # Fields
///     * `url` - The URL that was probed.
///     * `dns_time` - The time taken for DNS resolution, in seconds.
///     * `connect_time` - The time taken to establish a TCP connection, in seconds.
///     * `tls_time` - The time taken to establish a TLS connection, in seconds.
#[derive(Debug)]
struct HttpProbeResult {
    dns_time: Option<f64>,
    cert_validity_seconds: Option<f64>,
    connect_time: Option<f64>,
    tls_time: Option<f64>,
}

/// Function to get connection timings including DNS resolution, TCP connection, and TLS handshake
/// Returns a `HttpProbeResult` containing the timings and certificate validity
/// # Errors
///     Returns an error string if any step fails, such as DNS resolution failure, TCP connection failure, or TLS handshake failure.
async fn get_connect_timings(
    host: &str,
    connector: &TokioTlsConnector,
    resolver: &AsyncResolver<
        GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>,
    >,
    with_tls: bool,
) -> Result<HttpProbeResult, String> {
    // step one: DNS resolution
    let dns_start = Instant::now();
    if host.is_empty() {
        return Err("Host is empty".to_string());
    }
    let ip = match resolver.lookup_ip(host).await {
        Ok(lookup) => {
            // Use the first IP address from the lookup result
            lookup
                .iter()
                .next()
                .ok_or_else(|| format!("No IP addresses found for host {host}"))
        }
        Err(e) => return Err(format!("DNS resolution failed for host {host}: {e}")),
    };
    let dns_time = Some(dns_start.elapsed().as_secs_f64());

    // step two: TCP connection
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

    // step tree: TCP connection established
    let connect_time = Some(connect_start.elapsed().as_secs_f64());
    if !with_tls {
        return Ok(HttpProbeResult {
            dns_time,
            cert_validity_seconds: None,
            connect_time,
            tls_time: None,
        });
    }

    // step four: TLS handshake
    let tls_start = Instant::now();
    let tls_stream = connector.connect(host, stream).await;
    let (tls_time, cert_validity_seconds) = match tls_stream {
        Ok(tls_stream) => {
            let tls_time = Some(tls_start.elapsed().as_secs_f64());

            // step five: Parse the certificate and calculate its validity
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
        }
        Err(e) => {
            log::error!("Failed to establish TLS connection for host {host}: {e}");
            return Err(format!(
                "Failed to establish TLS connection for host {host}: {e}"
            ));
        }
    };

    Ok(HttpProbeResult {
        dns_time,
        cert_validity_seconds,
        connect_time,
        tls_time,
    })
}

/// Convert reqwest HTTP version to a float representation
/// # Arguments
///     * `version` - The HTTP version from reqwest
/// # Returns
///     A float representation of the HTTP version (e.g., 1.1 -> 1.1, 2.0 -> 2.0)
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

/// Probes a URL to validate its connectivity and performance metrics.
/// # Arguments
///     * `client` - An instance of `reqwest::Client` for making HTTP requests.
///     * `connector` - An instance of `TokioTlsConnector` for establishing TLS connections.
///     * `resolver` - An instance of `AsyncResolver` for DNS resolution.
///     * `url` - The URL to probe, which should be a valid HTTP or HTTPS URL.
/// # Returns
///     A `Result` containing a `ProbeResult` struct with the probe metrics if successful, or an error message if the probe fails.
/// # Errors
///     Returns an error string if the URL parsing fails, DNS resolution fails, connection fails, or HTTP request fails.
async fn probe_url(
    client: reqwest::Client,
    connector: &TokioTlsConnector,
    resolver: &AsyncResolver<
        GenericConnector<trust_dns_resolver::name_server::TokioRuntimeProvider>,
    >,
    url: &str,
) -> Result<ProbeResult, String> {
    let probe_start = Instant::now();
    let url = url.to_string();

    let parsed_url = Url::parse(&url).ok();
    let host = parsed_url
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or_default()
        .to_string();

    let probe_result =
        get_connect_timings(&host, connector, resolver, url.starts_with("https://")).await?;
    // Measure HTTP probe
    let start = Instant::now();
    let status_result = client.get(&url).send().await;
    let (processing_time, transfer_time, http_status, http_version) = match status_result {
        Ok(resp) => {
            let time_till_first_byte = start.elapsed().as_secs_f64();
            let http_status_val = resp.status().as_u16();
            let http_version_val = convert_http_version(resp.version());

            let transfer_start = Instant::now();
            let _ = resp
                .bytes()
                .await
                .map_err(|e| format!("Failed to read body: {e}"))?;
            let transfer_time_val = transfer_start.elapsed().as_secs_f64();
            (
                Some(time_till_first_byte),
                Some(transfer_time_val),
                Some(http_status_val),
                Some(http_version_val),
            )
        }
        Err(e) => {
            let error = report(&e);
            log::error!("HTTP request failed for URL {url}: {error}");
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

pub async fn run_probe_loop(
    tenant_name: String,
    org_config: OrganisationConfig,
    resolver: TokioAsyncResolver,
    tls_connector: TokioTlsConnector,
    mimir_endpoint: String,
    max_org_width: usize,
) {
    loop {
        let mut handles = vec![];

        let start_time = Instant::now();

        for target in &org_config.targets {
            let connector = tls_connector.clone();
            let resolver = resolver.clone();
            let target = target.clone();
            let tenant_name = tenant_name.clone();
            let org_id = org_config.organisation_id.clone();
            let mimir_endpoint = mimir_endpoint.clone();

            let probe_timeout_duration: Duration =
                Duration::from_secs(org_config.polling_interval_seconds);

            handles.push(tokio::spawn(tokio::time::timeout(
                probe_timeout_duration,
                async move {
                    handle_target_probe(
                        tenant_name,
                        &org_id,
                        &target,
                        &connector,
                        &resolver,
                        &mimir_endpoint,
                        max_org_width,
                    )
                    .await;
                },
            )));
        }

        for handle in handles {
            if let Err(join_err) = handle.await {
                log::error!("Task panicked: {:?}", join_err);
            }
        }
        let elapsed = start_time.elapsed().as_secs();
        let wait = org_config
            .polling_interval_seconds
            .checked_sub(elapsed)
            .unwrap_or(0);

        sleep(Duration::from_secs(wait)).await;
    }
}

/// Formats a string to a fixed width, truncating if necessary
/// # Arguments
///     * `input` - The input string to format.
///     * `width` - The desired width of the output string.
fn to_fixed_width(input: &str, width: usize) -> String {
    use unicode_truncate::UnicodeTruncateStr;

    let (truncated, _) = input.unicode_truncate(width);
    format!("{:<width$}", truncated, width = width)
}

/// Handles probing a target URL and sending the results to Mimir.
/// # Arguments
///     * `tenant` - The tenant name for logging and metrics.
///     * `org_id` - The organisation ID for Mimir metrics.
///     * `target` - The target configuration containing the URL and accepted status codes.
///     * `client` - The HTTP client used for making requests.
///     * `tls_connector` - The TLS connector for establishing secure connections.
///     * `resolver` - The DNS resolver for resolving hostnames.
///     * `mimir_target` - The Mimir endpoint to send metrics to.
///     * `max_width` - The maximum width for tenant name formatting in logs.
async fn handle_target_probe(
    tenant: String,
    org_id: &str,
    target: &TargetConfig,
    tls_connector: &TokioTlsConnector,
    resolver: &TokioAsyncResolver,
    mimir_target: &str,
    max_width: usize,
) {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .user_agent("reqwest-h2-h3-probe/1.0")
        .build()
        .expect("Failed to create client");

    let url = &target.url;
    let result = probe_url(client.clone(), tls_connector, resolver, url).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let padded_tenant = to_fixed_width(&tenant, max_width);

    match result {
        Ok(probe) => {
            let accepted = probe
                .http_status
                .map(|code| target.accepted_status_codes.contains(&code))
                .unwrap_or(false);

            if accepted {
                log::debug!(
                    "[{padded_tenant}] ✅ URL: {}, Status: {:?}, Elapsed: {:.2}ms, Cert: {}",
                    url,
                    probe.http_status,
                    probe.total_probe_time * 1000.0,
                    probe
                        .cert_validity_seconds
                        .map(|d| format!("{:.2}d", (d - now) / 86400.0))
                        .unwrap_or_else(|| "N/A".to_string())
                );
            } else {
                log::error!(
                    "[{padded_tenant}] ❌ Unexpected status for {url}: {:?} (accepted: {:?})",
                    probe.http_status,
                    target.accepted_status_codes
                );
            }

            let metrics = create_probe_metrics(&probe, accepted);
            if let Err(e) = send_to_mimir(mimir_target, Some(org_id), metrics).await {
                log::error!("[{padded_tenant}] Failed to send metrics for {url}: {e}");
            }
        }
        Err(e) => {
            // in case we cannot probe the url, send a failed probe with zeroed metrics
            log::error!("[{padded_tenant}] ❌ Probe error for {url}: {e}");
            let probe = ProbeResult {
                url: url.to_string(),
                dns_time: None,
                connect_time: None,
                tls_time: None,
                processing_time: None,
                cert_validity_seconds: None,
                http_status: None,
                http_version: None,
                transfer_time: None,
                total_probe_time: 0.0,
            };
            let metrics = create_probe_metrics(&probe, false);
            if let Err(e) = send_to_mimir(mimir_target, Some(org_id), metrics).await {
                log::error!("[{padded_tenant}] Failed to send error metrics for {url}: {e}");
            }
        }
    }
}
