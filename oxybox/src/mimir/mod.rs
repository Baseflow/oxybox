use client::prometheus::prompb;

pub mod client;

const INSTANCE_LABEL: &str = "instance";
const JOB_LABEL: &str = "job";
const MODULE_LABEL: &str = "module";
const TARGET_LABEL: &str = "target";
const PROBE_SUCCESS_METRIC: &str = "probe_success";
const PROBE_DURATION_METRIC: &str = "probe_duration_seconds";
const PROBE_HTTP_STATUS_METRIC: &str = "probe_http_status_code";
const PROBE_DNS_LOOKUP_TIME_METRIC: &str = "probe_dns_lookup_time_seconds";

const PROBE_TLS_VERSION_METRIC: &str = "probe_tls_version_info";
const PROBE_CONNECT_TIME_METRIC: &str = "probe_connect_time_seconds";
const PROBE_TLS_HANDSHAKE_TIME_METRIC: &str = "probe_tls_handshake_time_seconds";
const PROBE_REQUEST_DURATION_METRIC: &str = "probe_request_duration_seconds";
const PROBE_RESPONSE_DURATION_METRIC: &str = "probe_response_duration_seconds";
const PROBE_TOTAL_DURATION_METRIC: &str = "probe_total_duration_seconds";

const BLACKBOX_JOB: &str = "blackbox";
const HTTP_MODULE: &str = "http_2xx";

// probe_success	gauge	1 if the probe was successful, 0 otherwise
// probe_duration_seconds	gauge	Total time taken for the probe
// probe_http_status_code	gauge	The HTTP status code of the response
// probe_tls_version_info	gauge	TLS version info (with labels only)
// probe_dns_lookup_time_seconds	gauge	Time taken for DNS resolution
// probe_connect_time_seconds	gauge	Time taken to connect
// probe_tls_handshake_time_seconds	gauge	Time taken for TLS handshake
// probe_request_duration_seconds	gauge	Time between request and first byte received
// probe_response_duration_seconds	gauge	Time between first and last byte received
// probe_total_duration_seconds	gauge	End-to-end duration including DNS, connect, TLS, request, and response
fn create_time_series(metric_name: &str, instance: &str, value: f64) -> prompb::TimeSeries {
    let labels = &[
        (INSTANCE_LABEL, instance),
        (JOB_LABEL, BLACKBOX_JOB),
        (MODULE_LABEL, HTTP_MODULE),
        (TARGET_LABEL, instance),
    ];
    client::create_time_series(metric_name, labels, value, None)
}

pub fn create_probe_metrics(
    probe_result: &crate::ProbeResult) -> Vec<prompb::TimeSeries> {
    let mut metrics = Vec::new();
    let value = if probe_result.http_status.unwrap_or_default() == 200 { 1.0 } else { 0.0 };
    metrics.push(create_time_series(PROBE_SUCCESS_METRIC, &probe_result.url, value));
    metrics.push(create_time_series(PROBE_DURATION_METRIC, &probe_result.url, probe_result.http_time));
    if let Some(http_status) = probe_result.http_status {
        metrics.push(create_time_series(PROBE_HTTP_STATUS_METRIC, &probe_result.url, http_status as f64));
    }
    // if let Some(tls_version) = &probe_result.tls_version {
    //     metrics.push(create_time_series(PROBE_TLS_VERSION_METRIC, &probe_result.url, tls_version.clone() as f64));
    // }
    if let Some(dns_time) = probe_result.dns_time {
        metrics.push(create_time_series(PROBE_DNS_LOOKUP_TIME_METRIC, &probe_result.url, dns_time));
    }
    metrics
}



