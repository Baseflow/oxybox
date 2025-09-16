use std::{net::IpAddr, time::Duration};
use std::env;

use tokio_native_tls::TlsConnector as TokioTlsConnector;
use trust_dns_resolver::{
    TokioAsyncResolver,
    config::{NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig, ResolverOpts},
};

use super::probe_config::Config;

pub struct AppConfig {
    pub config: Config,
    pub mimir_endpoint: String,
    pub dns_hosts: Vec<String>,
    pub max_org_width: usize,
}

/// Load the application configuration from a YAML file and environment variables
/// This function reads the configuration file specified by the `CONFIG_FILE` environment variable,
/// parses it into a `Config` struct, and overrides certain values with environment variables.
/// It also sets up the DNS hosts and Mimir endpoint.
pub fn load_config() -> AppConfig {

    let config_file_location =
        env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yml".to_string());
    let config_str = std::fs::read_to_string(&config_file_location)
        .expect("Failed to read config.yaml");

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

    AppConfig {
        config,
        mimir_endpoint,
        dns_hosts,
        max_org_width,
    }
}

/// Setup a TLS connector that accepts invalid certificates
pub fn setup_tls_connector() -> Result<TokioTlsConnector, native_tls::Error> {
    let mut builder = native_tls::TlsConnector::builder();
    builder.danger_accept_invalid_certs(true);
    let connector = builder.build()?;
    Ok(TokioTlsConnector::from(connector))
}

/// Setup a DNS resolver using the provided DNS hosts
/// This function creates a `TokioAsyncResolver` configured with the specified DNS hosts.
/// It sets the resulver options to have 2 attempts, a timeout of 100 milliseconds, and a cache size of 1024 for quick DNS lookups.
/// # Arguments
///     * `dns_hosts` - A slice of strings representing DNS host IPs (e.g., "
/// # Returns
///     A `Result` containing a `TokioAsyncResolver` if successful, or an error if the setup fails.
pub fn setup_resolver(dns_hosts: &[String]) -> Result<TokioAsyncResolver, Box<dyn std::error::Error>> {
    let mut opts = ResolverOpts::default();
    opts.attempts = 2;
    opts.timeout = Duration::from_millis(100);
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
