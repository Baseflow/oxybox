use client::prometheus::prompb;

use crate::http_probe::result::ProbeResult;

pub mod client;

const INSTANCE_LABEL: &str = "instance";
const JOB_LABEL: &str = "job";
const MODULE_LABEL: &str = "module";
const TARGET_LABEL: &str = "target";
const PROBE_SUCCESS_METRIC: &str = "probe_success";
const PROBE_DURATION_METRIC: &str = "probe_duration_seconds";
const PROBE_HTTP_STATUS_METRIC: &str = "probe_http_status_code";
const PROBE_HTTP_DURATION_METRIC: &str = "probe_http_duration_seconds";
const PROBE_DNS_LOOKUP_TIME_METRIC: &str = "probe_dns_lookup_time_seconds";
const PROBE_HTTP_SSL_ENABLED_METRIC: &str = "probe_http_ssl";
const PROBE_HTTP_VERSION_METRIC: &str = "probe_http_version";
const PROBE_HTTP_SSL_EARLIEST_EXPIERY_METRIC: &str = "probe_ssl_earliest_cert_expiry";

const BLACKBOX_JOB: &str = "oxybox";
const HTTP_MODULE: &str = "http_probe";

fn create_time_series(
    metric_name: &str,
    instance: &str,
    value: f64,
    additional_labels: Option<Vec<(&str, &str)>>,
) -> prompb::TimeSeries {
    let mut labels: Vec<(&str, &str)> = vec![
        (INSTANCE_LABEL, instance),
        (JOB_LABEL, BLACKBOX_JOB),
        (MODULE_LABEL, HTTP_MODULE),
        (TARGET_LABEL, instance),
    ];
    if let Some(iter) = additional_labels {
        let _ = &labels.extend(iter);
    };

    client::create_time_series(metric_name, &labels, value, None)
}

/// Creates a vector of TimeSeries metrics for the given probe result.
/// The metrics include:
///    - `probe_success`: Indicates if the probe was successful (1.0 for success, 0.0 for failure).
///    - `probe_duration_seconds`: Total time taken for the probe.
///    - `probe_http_status_code`: HTTP status code received from the probe.
///    - `probe_http_duration_seconds`: Duration of various phases of the HTTP probe (resolve, connect, tls, processing, transfer).
///    - `probe_dns_lookup_time_seconds`: Time taken for DNS lookup.
///    - `probe_http_ssl`: Indicates if SSL was enabled (1.0 for enabled, 0.0 for not).
///    - `probe_ssl_earliest_cert_expiry`: Earliest expiry time of the SSL certificate in seconds.
///    - `probe_http_version`: HTTP version used for the probe (e.g., 1.0, 1.1, 2.0, 3.0).
/// ## Arguments:
///     - `probe_result`: A reference to the `ProbeResult` struct containing the results of the probe.
///     - `probe_success`: A boolean indicating whether the probe was successful or not.
/// ## Returns:
///     A vector of `prompb::TimeSeries` metrics representing the probe results, which can be sent
///     to a Prometheus-compatible monitoring system.
pub fn create_probe_metrics(
    probe_result: &ProbeResult,
    probe_success: bool,
) -> Vec<prompb::TimeSeries> {
    let mut metrics = Vec::new();
    let probe_successful = match probe_success {
        true => 1.0,
        false => 0.0,
    };

    metrics.push(create_time_series(
        PROBE_SUCCESS_METRIC,
        &probe_result.url,
        probe_successful,
        None,
    ));

    let phases = [
        (probe_result.dns_time, "resolve"),
        (probe_result.connect_time, "connect"),
        (probe_result.tls_time, "tls"),
        (probe_result.processing_time, "processing"),
        (probe_result.transfer_time, "transfer"),
    ];

    for (duration_opt, phase) in phases.iter() {
        if let Some(duration) = duration_opt {
            metrics.push(create_time_series(
                PROBE_HTTP_DURATION_METRIC,
                &probe_result.url,
                *duration,
                Some(vec![("phase", *phase)]),
            ));
        }
    }

    metrics.push(create_time_series(
        PROBE_DURATION_METRIC,
        &probe_result.url,
        probe_result.total_probe_time,
        None,
    ));
    if let Some(http_status) = probe_result.http_status {
        metrics.push(create_time_series(
            PROBE_HTTP_STATUS_METRIC,
            &probe_result.url,
            http_status as f64,
            None,
        ));
    }

    if let Some(dns_time) = probe_result.dns_time {
        metrics.push(create_time_series(
            PROBE_DNS_LOOKUP_TIME_METRIC,
            &probe_result.url,
            dns_time,
            None,
        ));
    }

    let ssl_enabled = match probe_result.cert_validity_seconds {
        Some(_) => 1.0,
        None => 0.0,
    };

    metrics.push(create_time_series(
        PROBE_HTTP_SSL_ENABLED_METRIC,
        &probe_result.url,
        ssl_enabled,
        None,
    ));

    if let Some(cert_validity_seconds) = probe_result.cert_validity_seconds {
        metrics.push(create_time_series(
            PROBE_HTTP_SSL_EARLIEST_EXPIERY_METRIC,
            &probe_result.url,
            cert_validity_seconds,
            None,
        ));
    }
    if let Some(http_version) = probe_result.http_version {
        metrics.push(create_time_series(
            PROBE_HTTP_VERSION_METRIC,
            &probe_result.url,
            http_version,
            None,
        ));
    }

    metrics
}
