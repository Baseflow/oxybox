use config::app_config::{
    ProbeClients, load_config, setup_quic_client_config, setup_resolver, setup_tls_connector,
};
use dotenvy::dotenv;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Semaphore, time::sleep};
pub mod http_probe;
use http_probe::probe::run_probe_loop;
pub mod config;
pub mod mimir;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();
    let app_config = load_config();
    let resolver = setup_resolver(&app_config.dns_hosts).expect("Failed to init resolver");
    let tls_connector = setup_tls_connector().expect("Failed to build TLS connector");
    let quic_config = setup_quic_client_config().expect("Failed to build QUIC client config");
    let clients = ProbeClients {
        resolver,
        tls_connector,
        quic_config,
    };

    log::info!("Using Mimir endpoint: {}", app_config.mimir_endpoint);

    let max_concurrent_probes = app_config.max_concurrent_probes.unwrap_or(32);
    log::info!(
        "Max concurrent probes set to: {}",
        max_concurrent_probes
    );
    let semaphore = Arc::new(Semaphore::new(max_concurrent_probes));

    let timeouts = app_config.timeouts;

    for (key, org_config) in app_config.config {
        let clients = clients.clone();
        let max_org_width = app_config.max_org_width;
        let mimir_endpoint = app_config.mimir_endpoint.clone();

        // create a new handle to the semaphore for each task
        let semaphore = semaphore.clone();

        tokio::spawn(run_probe_loop(
            key,
            org_config,
            clients,
            mimir_endpoint,
            max_org_width,
            semaphore,
            timeouts,
        ));
    }

    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
