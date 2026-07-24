use std::future::poll_fn;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Method, Request, Version, header};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rand::{Rng, rng};
use rustls::pki_types::CertificateDer;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::{Duration, sleep, timeout};
use url::Url;

use x509_parser::parse_x509_certificate;

use crate::config::app_config::{ProbeClients, Timeouts};
use crate::config::probe_config::{OrganisationConfig, TargetConfig};
use crate::mimir::client::send_to_mimir;
use crate::mimir::create_probe_metrics;

use super::result::ProbeResult;

const USER_AGENT_VALUE: &str = "oxybox-probe/1.0";

trait IoStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> IoStream for T {}

type DynStream = Box<dyn IoStream>;

/// Returns the earliest certificate expiry (as a Unix timestamp) across a
/// presented chain, matching the Blackbox `probe_ssl_earliest_cert_expiry`
/// semantics where an intermediate can expire before the leaf.
fn earliest_cert_expiry(certs: &[CertificateDer<'_>]) -> Option<f64> {
    certs
        .iter()
        .filter_map(|cert| {
            parse_x509_certificate(cert.as_ref())
                .ok()
                .map(|(_, parsed)| parsed.validity().not_after.timestamp())
        })
        .min()
        .map(|timestamp| timestamp as f64)
}

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
    clients: &ProbeClients,
    url: &str,
    http3: bool,
    connect_timeout: Duration,
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
    let ip = clients
        .resolver
        .lookup_ip(host.as_str())
        .await
        .map_err(|e| format!("DNS resolution failed for host {host}: {e}"))?
        .iter()
        .next()
        .ok_or_else(|| format!("No IP addresses found for host {host}"))?;
    let dns_time = dns_start.elapsed().as_secs_f64();

    if http3 {
        if !is_https {
            return Err(format!("HTTP/3 requires an https URL, got {url}"));
        }
        return probe_hop_h3(clients, url, &host, ip, port, dns_time, connect_timeout).await;
    }

    // step two: TCP connection
    let connect_start = Instant::now();
    let socket_addr = SocketAddr::new(ip, port);
    let tcp = match timeout(connect_timeout, TcpStream::connect(socket_addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("Failed to connect to {host}:{port}: {e}")),
        Err(_) => {
            return Err(format!(
                "Connect timeout after {connect_timeout:?} to {host}:{port}"
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
        let server_name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| format!("Invalid server name {host}: {e}"))?;
        let tls_stream = clients
            .tls_connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| format!("Failed to establish TLS connection for host {host}: {e}"))?;
        tls_time = Some(tls_start.elapsed().as_secs_f64());

        {
            let (_, session) = tls_stream.get_ref();
            alpn_h2 = session.alpn_protocol() == Some(&b"h2"[..]);
            if let Some(certs) = session.peer_certificates() {
                cert_validity_seconds = earliest_cert_expiry(certs);
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

/// Performs a single HTTP/3 request/response over a freshly established QUIC
/// connection, timing every phase. DNS resolution is done by the caller and its
/// timing passed in so the redirect loop can account for it uniformly.
///
/// QUIC folds transport and TLS 1.3 into one handshake, so `connect` measures the
/// (near-instant) endpoint/socket setup and `tls` measures the handshake itself.
async fn probe_hop_h3(
    clients: &ProbeClients,
    url: &str,
    host: &str,
    ip: IpAddr,
    port: u16,
    dns_time: f64,
    connect_timeout: Duration,
) -> Result<HopResult, String> {
    // step two: create a QUIC endpoint bound to the resolved address family
    let bind_addr: SocketAddr = if ip.is_ipv6() {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    };
    let connect_start = Instant::now();
    let mut endpoint = quinn::Endpoint::client(bind_addr)
        .map_err(|e| format!("Failed to create QUIC endpoint for {host}: {e}"))?;
    endpoint.set_default_client_config(clients.quic_config.clone());
    let connecting = endpoint
        .connect(SocketAddr::new(ip, port), host)
        .map_err(|e| format!("Failed to start QUIC connection to {host}:{port}: {e}"))?;
    let connect_time = connect_start.elapsed().as_secs_f64();

    // step three: QUIC + TLS 1.3 handshake, capturing certificate expiry
    let tls_start = Instant::now();
    let connection = match timeout(connect_timeout, connecting).await {
        Ok(Ok(conn)) => conn,
        Ok(Err(e)) => return Err(format!("QUIC handshake failed for {host}: {e}")),
        Err(_) => {
            return Err(format!(
                "QUIC handshake timeout after {connect_timeout:?} to {host}:{port}"
            ));
        }
    };
    let tls_time = Some(tls_start.elapsed().as_secs_f64());

    let cert_validity_seconds = connection
        .peer_identity()
        .and_then(|identity| identity.downcast::<Vec<CertificateDer<'static>>>().ok())
        .and_then(|certs| earliest_cert_expiry(&certs));

    // step four: HTTP/3 request, measuring time-to-first-byte and body transfer
    let h3_conn = h3_quinn::Connection::new(connection);
    let http_start = Instant::now();
    let (mut driver, mut send_request) = h3::client::new(h3_conn)
        .await
        .map_err(|e| format!("HTTP/3 connection setup failed for {host}: {e}"))?;
    let driver_task = tokio::spawn(async move { poll_fn(|cx| driver.poll_close(cx)).await });

    let req = Request::builder()
        .method(Method::GET)
        .uri(url)
        .header(header::USER_AGENT, USER_AGENT_VALUE)
        .body(())
        .map_err(|e| format!("Failed to build request for {url}: {e}"))?;

    let mut stream = send_request
        .send_request(req)
        .await
        .map_err(|e| format!("HTTP/3 request failed for {url}: {e}"))?;
    stream
        .finish()
        .await
        .map_err(|e| format!("HTTP/3 request send failed for {url}: {e}"))?;
    let resp = stream
        .recv_response()
        .await
        .map_err(|e| format!("HTTP/3 response failed for {url}: {e}"))?;
    let processing_time = http_start.elapsed().as_secs_f64();

    let http_status = resp.status().as_u16();
    let location = resp
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // step five: read the response body
    let transfer_start = Instant::now();
    while stream
        .recv_data()
        .await
        .map_err(|e| format!("Failed to read HTTP/3 body for {url}: {e}"))?
        .is_some()
    {}
    let transfer_time = transfer_start.elapsed().as_secs_f64();

    endpoint.close(0u32.into(), b"probe complete");
    endpoint.wait_idle().await;
    driver_task.abort();

    Ok(HopResult {
        dns_time,
        connect_time,
        tls_time,
        processing_time,
        transfer_time,
        cert_validity_seconds,
        http_status,
        http_version: 3.0,
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
    clients: &ProbeClients,
    url: &str,
    http3: bool,
    connect_timeout: Duration,
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
        let hop = probe_hop(clients, &current, http3, connect_timeout).await?;

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
    clients: ProbeClients,
    mimir_endpoint: String,
    max_org_width: usize,
    semaphore: Arc<Semaphore>,
    timeouts: Timeouts,
) {
    loop {
        let start_time = Instant::now();
        let mut handles = Vec::with_capacity(org_config.targets.len());

        for target in &org_config.targets {
            let semaphore = semaphore.clone();
            let clients = clients.clone();
            let target = target.clone();
            let tenant_name = tenant_name.clone();
            let org_id = org_config.organisation_id.clone();
            let mimir_endpoint = mimir_endpoint.clone();

            // Effective timeouts: per-target override, else the global default.
            let connect_timeout = target
                .connect_timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(timeouts.connect);
            let probe_timeout = target
                .probe_timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(timeouts.probe);

            handles.push(tokio::spawn(async move {
                // Jitter before taking a permit so the delay spreads probe starts
                // without occupying a concurrency slot.
                sleep(jitter(250)).await;

                // Permit is held until the task returns
                let _permit = semaphore.acquire_owned().await.expect("Semaphore closed");

                let result = tokio::time::timeout(probe_timeout, async {
                    handle_target_probe(
                        tenant_name,
                        &org_id,
                        &target,
                        &clients,
                        &mimir_endpoint,
                        max_org_width,
                        connect_timeout,
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

        let interval = Duration::from_secs(org_config.polling_interval_seconds);
        let wait = interval.saturating_sub(start_time.elapsed());

        sleep(wait).await;
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
///     * `clients` - The shared resolver, TLS connector and QUIC configuration.
///     * `mimir_target` - The Mimir endpoint to send metrics to.
///     * `max_width` - The maximum width for tenant name formatting in logs.
///     * `connect_timeout` - The per-hop TCP/QUIC connect timeout for this target.
async fn handle_target_probe(
    tenant: String,
    org_id: &str,
    target: &TargetConfig,
    clients: &ProbeClients,
    mimir_target: &str,
    max_width: usize,
    connect_timeout: Duration,
) {
    let url = &target.url;
    let result = probe_url(clients, url, target.http3, connect_timeout).await;

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
