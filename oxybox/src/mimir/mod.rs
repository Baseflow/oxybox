use client::prometheus::prompb;

use crate::http_probe::result::ProbeResult;

pub mod client;

const NAME_LABEL: &str = "__name__";
const INSTANCE_LABEL: &str = "instance";
const JOB_LABEL: &str = "job";
const MODULE_LABEL: &str = "module";
const TARGET_LABEL: &str = "target";
const PHASE_LABEL: &str = "phase";

/// Label names owned by Oxybox. A user-supplied label reusing one of these would
/// produce a duplicate label name, which Mimir rejects, so such labels are dropped.
const RESERVED_LABELS: [&str; 6] = [
    NAME_LABEL,
    INSTANCE_LABEL,
    JOB_LABEL,
    MODULE_LABEL,
    TARGET_LABEL,
    PHASE_LABEL,
];
const PROBE_SUCCESS_METRIC: &str = "probe_success";
const PROBE_DURATION_METRIC: &str = "probe_duration_seconds";
const PROBE_HTTP_STATUS_METRIC: &str = "probe_http_status_code";
const PROBE_HTTP_DURATION_METRIC: &str = "probe_http_duration_seconds";
const PROBE_DNS_LOOKUP_TIME_METRIC: &str = "probe_dns_lookup_time_seconds";
const PROBE_HTTP_SSL_ENABLED_METRIC: &str = "probe_http_ssl";
const PROBE_HTTP_VERSION_METRIC: &str = "probe_http_version";
const PROBE_HTTP_REDIRECTS_METRIC: &str = "probe_http_redirects";
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
///    - `probe_http_redirects`: Number of redirects followed before the final response.
/// ## Arguments:
///     - `probe_result`: A reference to the `ProbeResult` struct containing the results of the probe.
///     - `probe_success`: A boolean indicating whether the probe was successful or not.
/// ## Returns:
///     A vector of `prompb::TimeSeries` metrics representing the probe results, which can be sent
///     to a Prometheus-compatible monitoring system.
pub fn create_probe_metrics(
    probe_result: &ProbeResult,
    probe_success: bool,
    labels: Option<Vec<(&str, &str)>>,
) -> Vec<prompb::TimeSeries> {
    let custom: Vec<(&str, &str)> = labels
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| {
            let reserved = RESERVED_LABELS.contains(key);
            if reserved {
                log::warn!(
                    "Ignoring custom label '{key}' for {}: it collides with a reserved label",
                    probe_result.url
                );
            }
            !reserved
        })
        .collect();
    let series = |metric: &str, value: f64, phase: Option<&str>| {
        let mut extra = custom.clone();
        if let Some(phase) = phase {
            extra.push((PHASE_LABEL, phase));
        }
        create_time_series(metric, &probe_result.url, value, Some(extra))
    };

    let probe_successful = if probe_success { 1.0 } else { 0.0 };
    let mut metrics = vec![series(PROBE_SUCCESS_METRIC, probe_successful, None)];

    let phases = [
        (probe_result.dns_time, "resolve"),
        (probe_result.connect_time, "connect"),
        (probe_result.tls_time, "tls"),
        (probe_result.processing_time, "processing"),
        (probe_result.transfer_time, "transfer"),
    ];
    for (duration_opt, phase) in phases.iter() {
        if let Some(duration) = duration_opt {
            metrics.push(series(PROBE_HTTP_DURATION_METRIC, *duration, Some(*phase)));
        }
    }

    metrics.push(series(
        PROBE_DURATION_METRIC,
        probe_result.total_probe_time,
        None,
    ));

    if let Some(http_status) = probe_result.http_status {
        metrics.push(series(PROBE_HTTP_STATUS_METRIC, http_status as f64, None));
    }

    if let Some(dns_time) = probe_result.dns_time {
        metrics.push(series(PROBE_DNS_LOOKUP_TIME_METRIC, dns_time, None));
    }

    let ssl_enabled = if probe_result.cert_validity_seconds.is_some() {
        1.0
    } else {
        0.0
    };
    metrics.push(series(PROBE_HTTP_SSL_ENABLED_METRIC, ssl_enabled, None));

    if let Some(cert_validity_seconds) = probe_result.cert_validity_seconds {
        metrics.push(series(
            PROBE_HTTP_SSL_EARLIEST_EXPIERY_METRIC,
            cert_validity_seconds,
            None,
        ));
    }

    if let Some(http_version) = probe_result.http_version {
        metrics.push(series(PROBE_HTTP_VERSION_METRIC, http_version, None));
    }

    metrics.push(series(
        PROBE_HTTP_REDIRECTS_METRIC,
        probe_result.redirects as f64,
        None,
    ));

    metrics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_probe::result::ProbeResult;

    fn sample_probe() -> ProbeResult {
        ProbeResult {
            url: "https://example.com".to_string(),
            dns_time: Some(0.01),
            connect_time: Some(0.02),
            tls_time: Some(0.03),
            processing_time: Some(0.04),
            transfer_time: Some(0.05),
            cert_validity_seconds: Some(1_700_000_000.0),
            http_status: Some(200),
            http_version: Some(2.0),
            total_probe_time: 0.15,
            redirects: 1,
        }
    }

    fn metric_name(ts: &prompb::TimeSeries) -> String {
        ts.labels
            .iter()
            .find(|l| l.name == "__name__")
            .map(|l| l.value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn custom_labels_apply_to_every_series() {
        let labels = Some(vec![("environment", "production"), ("region", "eu")]);
        let metrics = create_probe_metrics(&sample_probe(), true, labels);

        assert!(metrics.len() > 1);
        for ts in &metrics {
            for (key, value) in [("environment", "production"), ("region", "eu")] {
                assert!(
                    ts.labels.iter().any(|l| l.name == key && l.value == value),
                    "series `{}` is missing custom label {key}={value}",
                    metric_name(ts),
                );
            }
        }
    }

    #[test]
    fn phase_series_keep_their_phase_label_alongside_custom_labels() {
        let labels = Some(vec![("environment", "production")]);
        let metrics = create_probe_metrics(&sample_probe(), true, labels);

        let phase_series: Vec<_> = metrics
            .iter()
            .filter(|ts| metric_name(ts) == PROBE_HTTP_DURATION_METRIC)
            .collect();
        assert_eq!(phase_series.len(), 5);
        for ts in phase_series {
            assert!(ts.labels.iter().any(|l| l.name == "phase"));
            assert!(
                ts.labels
                    .iter()
                    .any(|l| l.name == "environment" && l.value == "production")
            );
        }
    }

    #[test]
    fn reserved_custom_labels_are_dropped() {
        let labels = Some(vec![
            (INSTANCE_LABEL, "evil"),
            (PHASE_LABEL, "nope"),
            ("environment", "production"),
        ]);
        let metrics = create_probe_metrics(&sample_probe(), true, labels);

        assert!(metrics.len() > 1);
        for ts in &metrics {
            assert_eq!(
                ts.labels.iter().filter(|l| l.name == INSTANCE_LABEL).count(),
                1,
                "series `{}` has a duplicate instance label",
                metric_name(ts),
            );
            assert!(ts.labels.iter().all(|l| l.value != "evil"));
            assert!(ts.labels.iter().all(|l| l.value != "nope"));
            assert!(
                ts.labels
                    .iter()
                    .any(|l| l.name == "environment" && l.value == "production")
            );
        }
    }

    #[test]
    fn no_custom_labels_still_produces_valid_series() {
        let metrics = create_probe_metrics(&sample_probe(), true, None);
        assert!(metrics.iter().any(|ts| metric_name(ts) == PROBE_SUCCESS_METRIC));
        for ts in &metrics {
            assert!(ts.labels.iter().any(|l| l.name == INSTANCE_LABEL));
        }
    }
}
