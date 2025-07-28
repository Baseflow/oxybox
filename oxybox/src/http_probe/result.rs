pub struct ProbeResult {
    pub url: String,
    pub dns_time: Option<f64>,
    pub connect_time: Option<f64>,
    pub tls_time: Option<f64>,
    pub http_status: Option<u16>,
    pub http_version: Option<f64>,
    pub cert_validity_seconds: Option<f64>,
    pub processing_time: Option<f64>,
    pub transfer_time: Option<f64>,
    pub total_probe_time: f64,
}
