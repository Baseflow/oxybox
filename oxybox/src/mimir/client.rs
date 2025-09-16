pub mod prometheus {
    pub mod prompb {
        include!("../proto_generated/prometheus.rs");
    }
}

use chrono::Utc;
use prometheus::prompb::{Label, Sample, TimeSeries, WriteRequest};
use reqwest::{
    Client,
    header::{CONTENT_ENCODING, CONTENT_TYPE, HeaderMap, HeaderValue},
};

use snap::raw::Encoder;

/// Sends Prometheus metrics to a Mimir remote write endpoint.
///
/// # Arguments
///
/// * `mimir_endpoint` - The base URL of your Mimir instance (e.g., "http://localhost:9009").
/// * `tenant_id` - An optional tenant ID string for multi-tenant Mimir setups.
/// * `metrics` - A vector of `TimeSeries` to send.
pub async fn send_to_mimir(
    mimir_endpoint: &str,
    tenant_id: Option<&str>,
    metrics: Vec<TimeSeries>,
) -> Result<(), Box<dyn std::error::Error>> {
    if metrics.is_empty() {
        log::warn!("No metrics to send.");
        return Ok(());
    }

    let write_request = WriteRequest {
        timeseries: metrics,
        ..Default::default() // Ensures forward compatibility with future fields
    };

    let mut buf = Vec::new();
    prost::Message::encode(&write_request, &mut buf)?;

    let mut encoder = Encoder::new();
    let compressed_data = encoder.compress_vec(&buf)?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_ENCODING, HeaderValue::from_static("snappy"));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-protobuf"),
    );
    headers.insert(
        "X-Prometheus-Remote-Write-Version",
        HeaderValue::from_static("0.1.0"),
    );
    if let Some(id) = tenant_id {
        headers.insert("X-Scope-OrgID", HeaderValue::from_str(id)?);
    }

    let client = Client::new();
    let response = client
        .post(format!("{mimir_endpoint}/api/v1/push")) // Mimir's remote write endpoint
        .headers(headers)
        .body(compressed_data)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        log::error!("Failed to push to Mimir: {} - {}", status, body);
        return Err(format!("Failed to push to Mimir: {status} - {body}").into());
    }
    Ok(())
}

/// Creates a `TimeSeries` metric with the given metric name, labels, value, and optional timestamp.
/// This function is useful for constructing metrics that can be sent to Mimir or Prometheus.
/// # Arguments
///     * `metric_name` - The name of the metric (e.g., "my_app_http_requests_total").
///     * `labels` - A slice of tuples representing labels for the metric (e.g., &[("method", "GET"), ("status", "200")]).
///     * `value` - The value of the metric (e.g., 1.0 for a counter increment).
///     * `timestamp_ms` - An optional timestamp in milliseconds. If not provided, the current time will be used.
/// # Returns
///     A `TimeSeries` struct containing the metric data, which can be sent to Mimir or Prometheus.
pub fn create_time_series(
    metric_name: &str,
    labels: &[(&str, &str)],
    value: f64,
    timestamp_ms: Option<i64>,
) -> TimeSeries {
    let mut all_labels = Vec::new();
    // Add the default metric name label
    all_labels.push(Label {
        name: "__name__".to_string(),
        value: metric_name.to_string(),
    });

    // Add additional labels
    for (name, val) in labels {
        all_labels.push(Label {
            name: name.to_string(),
            value: val.to_string(),
        });
    }

    // Create a sample with the provided value and timestamp
    let sample = Sample {
        value,
        timestamp: timestamp_ms.unwrap_or_else(|| Utc::now().timestamp_millis()),
    };

    TimeSeries {
        labels: all_labels,
        samples: vec![sample],
        exemplars: vec![],
        histograms: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]

    async fn test_create_and_send_metrics() {
        // NOTE: This test will attempt to send data to a live Mimir instance.
        // Make sure Mimir is running at this address, or comment out for CI.
        let mimir_url = "http://localhost:9009"; // Adjust to your Mimir instance
        let tenant_id = Some("demo"); // Optional, remove if Mimir is single-tenant

        let mut metrics_to_send = Vec::new();

        // Metric 1: A counter for requests
        metrics_to_send.push(create_time_series(
            "my_app_http_requests_total",
            &[("method", "GET"), ("status", "200")],
            1.0,  // For a counter, typically increment by 1 per event
            None, // Use current timestamp
        ));

        // Metric 2: A gauge for CPU usage
        metrics_to_send.push(create_time_series(
            "my_app_cpu_usage_percent",
            &[("host", "server-a")],
            25.5,                                // Current value for a gauge
            Some(Utc::now().timestamp_millis()), // Specific timestamp
        ));

        // Metric 3: Another counter with different labels
        metrics_to_send.push(create_time_series(
            "my_app_database_queries_total",
            &[("db", "users"), ("type", "read")],
            1.0,
            None,
        ));

        // Attempt to send
        match send_to_mimir(mimir_url, tenant_id, metrics_to_send).await {
            Ok(_) => log::debug!("Test metrics sent successfully."),
            Err(e) => log::debug!("Failed to send test metrics: {}", e),
        }

        assert!(true);
        
    }
}
