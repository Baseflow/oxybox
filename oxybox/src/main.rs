use std::time::Duration;
use config::app_config::{load_config, setup_resolver, setup_tls_connector};
use tokio::time::sleep;
pub mod http_probe;
use http_probe::probe::run_probe_loop;
pub mod config;
pub mod mimir;

#[tokio::main]
async fn main() {
    let app_config = load_config();
    let resolver = setup_resolver(&app_config.dns_hosts).expect("Failed to init resolver");
    let tls_connector = setup_tls_connector().expect("Failed to build TLS connector");

    println!("Using Mimir endpoint: {}", app_config.mimir_endpoint);

    for (key, org_config) in app_config.config {
        let resolver = resolver.clone();
        let tls_connector = tls_connector.clone();
        let max_org_width = app_config.max_org_width;
        let mimir_endpoint = app_config.mimir_endpoint.clone();

        tokio::spawn(run_probe_loop(
            key,
            org_config,
            resolver,
            tls_connector,
            mimir_endpoint,
            max_org_width,
        ));
    }

    loop {
        sleep(Duration::from_secs(60)).await;
    }
}



