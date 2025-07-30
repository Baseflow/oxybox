use client::prometheus::prompb;

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

// const PROBE_TLS_VERSION_METRIC: &str = "probe_tls_version_info";
// const PROBE_CONNECT_TIME_METRIC: &str = "probe_connect_time_seconds";
// const PROBE_TLS_HANDSHAKE_TIME_METRIC: &str = "probe_tls_handshake_time_seconds";
// const PROBE_REQUEST_DURATION_METRIC: &str = "probe_request_duration_seconds";
// const PROBE_RESPONSE_DURATION_METRIC: &str = "probe_response_duration_seconds";
// const PROBE_TOTAL_DURATION_METRIC: &str = "probe_total_duration_seconds";
const PROBE_HTTP_SSL_EARLIEST_EXPIERY_METRIC: &str = "probe_ssl_earliest_cert_expiry";

const BLACKBOX_JOB: &str = "oxybox";
const HTTP_MODULE: &str = "http_probe";

fn create_time_series(
    metric_name: &str,
    instance: &str,
    value: f64,
    additional_labels: Option<Vec<(&str, &str)>>,
) -> prompb::TimeSeries {
    // combine the mandatory labels with any additional labels
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

pub fn create_probe_metrics(probe_result: &crate::ProbeResult, probe_success: bool) -> Vec<prompb::TimeSeries> {
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

    if let Some(dns_time) = probe_result.dns_time {
        metrics.push(create_time_series(
            PROBE_HTTP_DURATION_METRIC,
            &probe_result.url,
            dns_time,
            Some(vec![("phase", "resolve")]),
        ));
    };
    if let Some(connect_time) = probe_result.connect_time {
        metrics.push(create_time_series(
            PROBE_HTTP_DURATION_METRIC,
            &probe_result.url,
            connect_time,
            Some(vec![("phase", "connect")]),
        ));
    };
    if let Some(tls_time) = probe_result.tls_time {
        metrics.push(create_time_series(
            PROBE_HTTP_DURATION_METRIC,
            &probe_result.url,
            tls_time,
            Some(vec![("phase", "tls")]),
        ));
    };
    if let Some(processing_time) = probe_result.processing_time {
        metrics.push(create_time_series(
            PROBE_HTTP_DURATION_METRIC,
            &probe_result.url,
            processing_time,
            Some(vec![("phase", "processing")]),
        ));
    };
    if let Some(transfer_time) = probe_result.transfer_time {
        metrics.push(create_time_series(
            PROBE_HTTP_DURATION_METRIC,
            &probe_result.url,
            transfer_time,
            Some(vec![("phase", "transfer")]),
        ));
    };

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

    let ssl_enabled = if probe_result.cert_validity_seconds.is_some() {
        1.0
    } else {
        0.0
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
