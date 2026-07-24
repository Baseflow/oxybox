use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

fn deserialize_labels<'de, D>(deserializer: D) -> Result<Option<Vec<(String, String)>>, D::Error>
where
    D: Deserializer<'de>,
{
    let map = Option::<HashMap<String, String>>::deserialize(deserializer)?;
    Ok(map.map(|m| m.into_iter().collect()))
}

/// An organisation configuration for the OxyBox service.
/// Contains the organisation ID, the polling interval in seconds, and a list of target configurations.
#[derive(Debug, Clone, Deserialize)]
pub struct OrganisationConfig {
    /// The organisation ID for which this configuration applies.
    /// This translates to the 'Org-Id' header in the Mimir requests.
    pub organisation_id: String,

    /// The polling interval in seconds for the OxyBox service.
    pub polling_interval_seconds: u64,

    /// A list of target configurations for the OxyBox service.
    pub targets: Vec<TargetConfig>,
}

/// A target configuration for the OxyBox service.
/// Contains the target URL and a list of accepted HTTP status codes.
#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    /// The URL of the target service to be monitored.
    pub url: String,

    /// The accepted HTTP status codes for the target service.
    /// Defaults to 200 if not specified.
    #[serde(default = "default_status_codes")]
    pub accepted_status_codes: Vec<u16>,

    /// Additional labels to be included in the Mimir metrics for this target.
    /// This is an optional field that can contain a list of key-value pairs representing the labels.
    /// For example, you could include labels like `env: production` or `region: us-east-1` to provide more context about the target service in the metrics.
    #[serde(default, deserialize_with = "deserialize_labels")]
    pub labels: Option<Vec<(String, String)>>,

    /// Probe this target over HTTP/3 (QUIC) instead of HTTP/1.1 or HTTP/2 over TCP.
    /// Requires an `https` URL. Defaults to `false`.
    #[serde(default)]
    pub http3: bool,

    /// Per-target TCP/QUIC connect timeout in seconds. Overrides the global
    /// `CONNECT_TIMEOUT_SECONDS` for this target when set.
    #[serde(default)]
    pub connect_timeout_seconds: Option<u64>,

    /// Per-target overall probe timeout in seconds (covers the whole redirect
    /// chain). Overrides the global `PROBE_TIMEOUT_SECONDS` for this target when set.
    #[serde(default)]
    pub probe_timeout_seconds: Option<u64>,
}

fn default_status_codes() -> Vec<u16> {
    vec![200]
}

pub type Config = std::collections::HashMap<String, OrganisationConfig>;

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_default_status_codes() {
        let default_codes = default_status_codes();
        assert_eq!(default_codes, vec![200]);
    }

    #[test]
    fn test_target_config_deserialization() {
        let yaml = r#"
                    demo:
                        organisation_id: demo
                        polling_interval_seconds: 10
                        targets:
                            - url: https://www.google.com
                            - url: https://www.github.com
                              accepted_status_codes: [200, 301]

                    organisationX:
                        organisation_id: 1
                        polling_interval_seconds: 20
                        targets:
                            - url: http://www.example.com
                                    "#;

        let config: Config = serde_yaml::from_str(yaml).expect("Invalid YAML");
        assert!(config.contains_key("demo"));
        assert!(config.contains_key("organisationX"));
        let demo_config = config.get("demo").expect("Demo config not found");
        assert_eq!(demo_config.organisation_id, "demo");
        assert_eq!(demo_config.polling_interval_seconds, 10);
        assert_eq!(demo_config.targets.len(), 2);
        assert_eq!(demo_config.targets[0].url, "https://www.google.com");
        assert_eq!(demo_config.targets[1].url, "https://www.github.com");
        assert_eq!(demo_config.targets[1].accepted_status_codes, vec![200, 301]);
        let org_x_config = config
            .get("organisationX")
            .expect("OrganisationX config not found");
        assert_eq!(org_x_config.organisation_id, "1");
        assert_eq!(org_x_config.polling_interval_seconds, 20);
        assert_eq!(org_x_config.targets.len(), 1);
        assert_eq!(org_x_config.targets[0].url, "http://www.example.com");
        // check default status codes
        assert_eq!(org_x_config.targets[0].accepted_status_codes, vec![200]);
    }

    #[test]
    fn test_timeout_overrides_default_and_parse() {
        let yaml = r#"
                    demo:
                        organisation_id: demo
                        polling_interval_seconds: 10
                        targets:
                            - url: https://slow.example
                              connect_timeout_seconds: 15
                              probe_timeout_seconds: 30
                            - url: https://default.example
                    "#;

        let config: Config = serde_yaml::from_str(yaml).expect("Invalid YAML");
        let demo = config.get("demo").expect("Demo config not found");
        assert_eq!(demo.targets[0].connect_timeout_seconds, Some(15));
        assert_eq!(demo.targets[0].probe_timeout_seconds, Some(30));
        assert_eq!(demo.targets[1].connect_timeout_seconds, None);
        assert_eq!(demo.targets[1].probe_timeout_seconds, None);
    }

    #[test]
    fn test_http3_flag_defaults_and_parses() {
        let yaml = r#"
                    demo:
                        organisation_id: demo
                        polling_interval_seconds: 10
                        targets:
                            - url: https://quic.example
                              http3: true
                            - url: https://tcp.example
                    "#;

        let config: Config = serde_yaml::from_str(yaml).expect("Invalid YAML");
        let demo = config.get("demo").expect("Demo config not found");
        assert!(demo.targets[0].http3, "http3 should parse as true");
        assert!(!demo.targets[1].http3, "http3 should default to false");
    }
}
