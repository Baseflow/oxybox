pub struct ProbeResult {
    pub url: String,
    pub dns_time: Option<f64>,
    pub http_status: Option<u16>,
    pub http_version: Option<String>,
    pub cert_validity_days: Option<i64>,
    pub http_time: f64,
}
