use serde::Deserialize;

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
}
