use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OrganisationConfig {
    pub organisation_id: String,
    pub polling_interval_seconds: u64,
    pub targets: Vec<TargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    pub url: String,

    #[serde(default = "default_status_codes")]
    pub accepted_status_codes: Vec<u16>,
}

fn default_status_codes() -> Vec<u16> {
    vec![200]
}

pub type Config = std::collections::HashMap<String, OrganisationConfig>;
