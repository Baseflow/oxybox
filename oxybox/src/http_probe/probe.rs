use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Method, Request, Version, header};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rand::{Rng, rng};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::{Duration, sleep, timeout};
use url::Url;

use tokio_native_tls::TlsConnector as TokioTlsConnector;
use x509_parser::parse_x509_certificate;

use crate::config::probe_config::{OrganisationConfig, TargetConfig};
use crate::mimir::client::send_to_mimir;
use crate::mimir::create_probe_metrics;

use trust_dns_resolver::TokioAsyncResolver;

use super::result::ProbeResult;

const USER_AGENT_VALUE: &str = "oxybox-probe/1.0";

trait IoStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> IoStream for T {}

type DynStream = Box<dyn IoStream>;

/// Convert an HTTP version to a float representation (e.g. 1.1 -> 1.1, 2.0 -> 2.0)
fn http_version_to_f64(version: Version) -> f64 {
    match version {
        Version::HTTP_09 => 0.9,
        Version::HTTP_10 => 1.0,
        Version::HTTP_11 => 1.1,
        Version::HTTP_2 => 2.0,
        Version::HTTP_3 => 3.0,
        _ => 0.0,
    }
}

/// Maximum number of redirects followed in a single probe before giving up.
const MAX_REDIRECTS: usize = 10;

/// Per-phase result of a single request/response exchange (one redirect hop).
struct HopResult {
    dns_time: f64,
    connect_time: f64,
    tls_time: Option<f64>,
    processing_time: f64,
    transfer_time: f64,
    cert_validity_seconds: Option<f64>,
    http_status: u16,
    http_version: f64,
    location: Option<String>,
}

/// Performs a single request/response exchange over one freshly established
/// connection, timing every phase on that connection.
///
/// The request that produces the `processing` and `transfer` timings is sent over
/// the exact connection whose DNS resolution, TCP connect and TLS handshake are
/// timed, so all phases describe one real request rather than a throwaway probe.
async fn probe_hop(
    connector: &TokioTlsConnector,
    resolver: &TokioAsyncResolver,
    url: &str,
) -> Result<HopResult, String> {
    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL {url}: {e}"))?;
    let scheme = parsed.scheme();
    let is_https = scheme.eq_ignore_ascii_case("https");
    if !is_https && !scheme.eq_ignore_ascii_case("http") {
        return Err(format!("Unsupported scheme '{scheme}' for {url}"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL has no host: {url}"))?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("Could not determine port for {url}"))?;

    // step one: DNS resolution
    let dns_start = Instant::now();
    let ip = resolver
        .lookup_ip(host.as_str())
        .await
        .map_err(|e| format!("DNS resolution failed for host {host}: {e}"))?
        .iter()
        .next()
        .ok_or_else(|| format!("No IP addresses found for host {host}"))?;
    let dns_time = dns_start.elapsed().as_secs_f64();

    // step two: TCP connection
    let connect_start = Instant::now();
    let socket_addr = SocketAddr::new(ip, port);
    let connect_deadline = Duration::from_secs(5);
    let tcp = match timeout(connect_deadline, TcpStream::connect(socket_addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("Failed to connect to {host}:{port}: {e}")),
        Err(_) => {
            return Err(format!(
                "Connect timeout after {connect_deadline:?} to {host}:{port}"
            ));
        }
    };
    let connect_time = connect_start.elapsed().as_secs_f64();

    // step three: TLS handshake (https only), capturing ALPN and certificate expiry
    let mut tls_time = None;
    let mut cert_validity_seconds = None;
    let mut alpn_h2 = false;

    let stream: DynStream = if is_https {
        let tls_start = Instant::now();
        let tls_stream = connector
            .connect(&host, tcp)
            .await
            .map_err(|e| format!("Failed to establish TLS connection for host {host}: {e}"))?;
        tls_time = Some(tls_start.elapsed().as_secs_f64());

        {
            let raw = tls_stream.get_ref();
            if let Ok(Some(proto)) = raw.negotiated_alpn() {
                alpn_h2 = proto == b"h2";
            }
            if let Ok(Some(cert)) = raw.peer_certificate() {
                if let Ok(der) = cert.to_der() {
                    if let Ok((_, parsed_cert)) = parse_x509_certificate(&der) {
                        cert_validity_seconds =
                            Some(parsed_cert.validity().not_after.timestamp() as f64);
                    }
                }
            }
        }

        Box::new(tls_stream)
    } else {
        Box::new(tcp)
    };

    // step four: send the HTTP request over the same connection, measuring
    // time-to-first-byte (processing) and body transfer separately.
    let io = TokioIo::new(stream);
    let host_header = match parsed.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.clone(),
    };
    let path_and_query = match parsed.query() {
        Some(q) => format!("{}?{}", parsed.path(), q),
        None => parsed.path().to_string(),
    };

    let http_start = Instant::now();
    let resp = if alpn_h2 {
        let req = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header(header::USER_AGENT, USER_AGENT_VALUE)
            .body(Empty::<Bytes>::new())
            .map_err(|e| format!("Failed to build request for {url}: {e}"))?;

        let (mut sender, conn) = http2::handshake(TokioExecutor::new(), io)
            .await
            .map_err(|e| format!("HTTP/2 handshake failed for {host}: {e}"))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        sender
            .send_request(req)
            .await
            .map_err(|e| format!("HTTP/2 request failed for {url}: {e}"))?
    } else {
        let req = Request::builder()
            .method(Method::GET)
            .uri(&path_and_query)
            .header(header::HOST, &host_header)
            .header(header::USER_AGENT, USER_AGENT_VALUE)
            .body(Empty::<Bytes>::new())
            .map_err(|e| format!("Failed to build request for {url}: {e}"))?;

        let (mut sender, conn) = http1::handshake(io)
            .await
            .map_err(|e| format!("HTTP handshake failed for {host}: {e}"))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        sender
            .send_request(req)
            .await
            .map_err(|e| format!("HTTP request failed for {url}: {e}"))?
    };
    let processing_time = http_start.elapsed().as_secs_f64();

    let http_status = resp.status().as_u16();
    let http_version = http_version_to_f64(resp.version());
    let location = resp
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // step five: read the response body
    let transfer_start = Instant::now();
    resp.into_body()
        .collect()
        .await
        .map_err(|e| format!("Failed to read body for {url}: {e}"))?;
    let transfer_time = transfer_start.elapsed().as_secs_f64();

    Ok(HopResult {
        dns_time,
        connect_time,
        tls_time,
        processing_time,
        transfer_time,
        cert_validity_seconds,
        http_status,
        http_version,
        location,
    })
}

/// Probes a URL, following redirects up to [`MAX_REDIRECTS`] hops.
///
/// Per-phase timings are summed across every hop in the redirect chain, while the
/// reported status, HTTP version and certificate expiry come from the final
/// response. The `url` field of the result keeps the originally configured target
/// so its Mimir labels stay stable regardless of where the redirects lead.
///
/// # Errors
/// Returns an error string if any hop fails to resolve, connect, handshake or
/// exchange, if a redirect target is unparseable, or if the redirect limit is hit.
async fn probe_url(
    connector: &TokioTlsConnector,
    resolver: &TokioAsyncResolver,
    url: &str,
) -> Result<ProbeResult, String> {
    let probe_start = Instant::now();

    let mut current = url.to_string();
    let mut dns_time = 0.0;
    let mut connect_time = 0.0;
    let mut tls_time: Option<f64> = None;
    let mut processing_time = 0.0;
    let mut transfer_time = 0.0;
    let mut redirects = 0usize;

    loop {
        let hop = probe_hop(connector, resolver, &current).await?;

        dns_time += hop.dns_time;
        connect_time += hop.connect_time;
        if let Some(t) = hop.tls_time {
            tls_time = Some(tls_time.unwrap_or(0.0) + t);
        }
        processing_time += hop.processing_time;
        transfer_time += hop.transfer_time;

        let is_redirect = (300..400).contains(&hop.http_status);
        match hop.location {
            Some(location) if is_redirect => {
                if redirects >= MAX_REDIRECTS {
                    return Err(format!("Exceeded {MAX_REDIRECTS} redirects starting at {url}"));
                }
                let base =
                    Url::parse(&current).map_err(|e| format!("Invalid URL {current}: {e}"))?;
                let next = base.join(&location).map_err(|e| {
                    format!("Invalid redirect target '{location}' from {current}: {e}")
                })?;
                current = next.to_string();
                redirects += 1;
            }
            _ => {
                return Ok(ProbeResult {
                    url: url.to_string(),
                    dns_time: Some(dns_time),
                    connect_time: Some(connect_time),
                    tls_time,
                    processing_time: Some(processing_time),
                    cert_validity_seconds: hop.cert_validity_seconds,
                    http_status: Some(hop.http_status),
                    http_version: Some(hop.http_version),
                    transfer_time: Some(transfer_time),
                    total_probe_time: probe_start.elapsed().as_secs_f64(),
                    redirects: redirects as u32,
                });
            }
        }
    }
}

fn jitter(max_ms: u64) -> Duration {
    Duration::from_millis(rng().random_range(0..max_ms))
}

pub async fn run_probe_loop(
    tenant_name: String,
    org_config: OrganisationConfig,
    resolver: TokioAsyncResolver,
    tls_connector: TokioTlsConnector,
    mimir_endpoint: String,
    max_org_width: usize,
    semaphore: Arc<Semaphore>,
) {
    loop {
        let start_time = Instant::now();
        let mut handles = Vec::with_capacity(org_config.targets.len());

        // Per-probe timeout (separate from polling interval)
        let probe_timeout = Duration::from_secs(10);

        for target in &org_config.targets {
            let semaphore = semaphore.clone();
            let connector = tls_connector.clone();
            let resolver = resolver.clone();
            let target = target.clone();
            let tenant_name = tenant_name.clone();
            let org_id = org_config.organisation_id.clone();
            let mimir_endpoint = mimir_endpoint.clone();

            handles.push(tokio::spawn(async move {
                // Permit is held until the task returns
                let _permit = semaphore.acquire_owned().await.expect("Semaphore closed");

                sleep(jitter(250)).await;

                let result = tokio::time::timeout(probe_timeout, async {
                    handle_target_probe(
                        tenant_name,
                        &org_id,
                        &target,
                        &connector,
                        &resolver,
                        &mimir_endpoint,
                        max_org_width,
                    )
                    .await
                })
                .await;

                if result.is_err() {
                    log::warn!("Probe timed out for {}", target.url);
                }
            }));
        }

        for handle in handles {
            if let Err(join_err) = handle.await {
                log::error!("Task panicked: {:?}", join_err);
            }
        }

        let elapsed = start_time.elapsed().as_secs();
        let wait = org_config.polling_interval_seconds.saturating_sub(elapsed);

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
    let url = &target.url;
    let result = probe_url(tls_connector, resolver, url).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let padded_tenant = to_fixed_width(&tenant, max_width);

    let labels = target.labels.as_ref().map(|l| {
        l.iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect::<Vec<(&str, &str)>>()
    });

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

            let metrics = create_probe_metrics(&probe, accepted, labels);

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
                redirects: 0,
            };
            let metrics = create_probe_metrics(&probe, false, labels);
            if let Err(e) = send_to_mimir(mimir_target, Some(org_id), metrics).await {
                log::error!("[{padded_tenant}] Failed to send error metrics for {url}: {e}");
            }
        }
    }
}
