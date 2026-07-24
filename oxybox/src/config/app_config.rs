use std::env;
use std::sync::Arc;
use std::{net::IpAddr, time::Duration};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio_rustls::TlsConnector as TokioTlsConnector;
use trust_dns_resolver::{
    TokioAsyncResolver,
    config::{NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig, ResolverOpts},
};

use super::probe_config::Config;

/// Shared, cheaply-cloneable clients used by every probe: the DNS resolver, the
/// rustls connector for HTTP/1.1 and HTTP/2 over TCP, and the QUIC client
/// configuration for HTTP/3.
#[derive(Clone)]
pub struct ProbeClients {
    pub resolver: TokioAsyncResolver,
    pub tls_connector: TokioTlsConnector,
    pub quic_config: quinn::ClientConfig,
}

const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 5;
const DEFAULT_PROBE_TIMEOUT_SECONDS: u64 = 10;

/// Default (global) probe timeouts, overridable per target in the probe config.
///
/// `connect` bounds establishing the TCP/QUIC connection for a single hop;
/// `probe` bounds the whole probe including every redirect hop.
#[derive(Clone, Copy)]
pub struct Timeouts {
    pub connect: Duration,
    pub probe: Duration,
}

pub struct AppConfig {
    pub config: Config,
    pub mimir_endpoint: String,
    pub dns_hosts: Vec<String>,
    pub max_org_width: usize,
    pub max_concurrent_probes: Option<usize>,
    pub timeouts: Timeouts,
}

/// Reads a duration (in whole seconds) from an environment variable, falling back
/// to `default_seconds` when unset or unparseable.
fn env_duration_seconds(name: &str, default_seconds: u64) -> Duration {
    match env::var(name) {
        Ok(val) => match val.trim().parse::<u64>() {
            Ok(seconds) => Duration::from_secs(seconds),
            Err(_) => {
                log::warn!("Invalid {name}='{val}', falling back to {default_seconds}s");
                Duration::from_secs(default_seconds)
            }
        },
        Err(_) => Duration::from_secs(default_seconds),
    }
}

/// Load the application configuration from a YAML file and environment variables
/// This function reads the configuration file specified by the `CONFIG_FILE` environment variable,
/// parses it into a `Config` struct, and overrides certain values with environment variables.
/// It also sets up the DNS hosts and Mimir endpoint.
pub fn load_config() -> AppConfig {
    let config_file_location = env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yml".to_string());
    let config_str =
        std::fs::read_to_string(&config_file_location).expect("Failed to read config.yaml");

    let config: Config = serde_yaml::from_str(&config_str).expect("Invalid YAML");

    let dns_hosts = env::var("DNS_HOSTS")
        .unwrap_or_else(|_| "1.1.1.1,8.8.8.8".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    log::info!("Using DNS hosts: {:?}", dns_hosts);

    let mimir_endpoint =
        env::var("MIMIR_ENDPOINT").unwrap_or_else(|_| "http://localhost:9009".to_string());

    let max_org_width = config.keys().map(|org| org.len()).max().unwrap_or(10);

    let max_concurrent_probes = match env::var("MAX_CONCURRENT_PROBES") {
        Ok(val) => match val.trim().parse::<usize>() {
            Ok(0) => {
                log::warn!("MAX_CONCURRENT_PROBES=0 would stall all probes; using the default");
                None
            }
            Ok(n) => Some(n),
            Err(_) => {
                log::warn!("Invalid MAX_CONCURRENT_PROBES='{val}'; using the default");
                None
            }
        },
        Err(_) => None,
    };

    let timeouts = Timeouts {
        connect: env_duration_seconds("CONNECT_TIMEOUT_SECONDS", DEFAULT_CONNECT_TIMEOUT_SECONDS),
        probe: env_duration_seconds("PROBE_TIMEOUT_SECONDS", DEFAULT_PROBE_TIMEOUT_SECONDS),
    };
    log::info!(
        "Timeouts: connect={}s probe={}s (per-target overrides may apply)",
        timeouts.connect.as_secs(),
        timeouts.probe.as_secs()
    );

    AppConfig {
        config,
        mimir_endpoint,
        dns_hosts,
        max_org_width,
        max_concurrent_probes,
        timeouts,
    }
}

/// Setup a rustls TLS connector for HTTP/1.1 and HTTP/2 over TCP.
/// Advertises the `h2` and `http/1.1` ALPN protocols and accepts invalid
/// certificates (probes should still report on expired/self-signed endpoints).
pub fn setup_tls_connector() -> Result<TokioTlsConnector, Box<dyn std::error::Error>> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());

    let mut config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAllCerts(provider)))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(TokioTlsConnector::from(Arc::new(config)))
}

/// A rustls certificate verifier that accepts any certificate, so probes still
/// report on endpoints with expired or self-signed certificates.
#[derive(Debug)]
struct AcceptAllCerts(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAllCerts {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Setup a QUIC client configuration for HTTP/3 probes.
/// Uses TLS 1.3 (required by QUIC), advertises the `h3` ALPN protocol, and accepts
/// invalid certificates to match the behaviour of the TCP TLS connector.
pub fn setup_quic_client_config() -> Result<quinn::ClientConfig, Box<dyn std::error::Error>> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());

    let mut tls_config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAllCerts(provider)))
        .with_no_client_auth();
    tls_config.alpn_protocols = vec![b"h3".to_vec()];

    let quic_tls = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)?;
    Ok(quinn::ClientConfig::new(Arc::new(quic_tls)))
}

/// Setup a DNS resolver using the provided DNS hosts.
/// The per-attempt timeout (`DNS_TIMEOUT_MS`, default 100) and attempt count
/// (`DNS_ATTEMPTS`, default 2) are configurable via environment variables so the
/// aggressive default can be relaxed for slower resolvers.
/// # Arguments
///     * `dns_hosts` - A slice of strings representing DNS host IPs.
/// # Returns
///     A `Result` containing a `TokioAsyncResolver` if successful, or an error if the setup fails.
pub fn setup_resolver(
    dns_hosts: &[String],
) -> Result<TokioAsyncResolver, Box<dyn std::error::Error>> {
    let timeout_ms = env::var("DNS_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(100);
    let attempts = env::var("DNS_ATTEMPTS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(2);

    let mut opts = ResolverOpts::default();
    opts.attempts = attempts;
    opts.timeout = Duration::from_millis(timeout_ms);
    opts.cache_size = 1024;

    let mut name_servers = NameServerConfigGroup::new();

    for host in dns_hosts {
        let ip: IpAddr = host.parse()?;
        name_servers.push(NameServerConfig {
            socket_addr: (ip, 53).into(),
            protocol: Protocol::Tcp,
            tls_dns_name: None,
            trust_negative_responses: false,
            bind_addr: None,
        });
    }

    let resolver_config = ResolverConfig::from_parts(None, vec![], name_servers);
    Ok(TokioAsyncResolver::tokio(resolver_config, opts))
}
