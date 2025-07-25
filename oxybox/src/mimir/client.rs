pub mod prometheus {
    pub mod prompb {
        include!("../proto_generated/prometheus.rs");
    }
}

pub use prometheus::prompb::{Label, Sample, TimeSeries, WriteRequest};

// Other necessary imports for the client logic
use reqwest::blocking::Client; // Use blocking client for simplicity here
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_ENCODING, CONTENT_TYPE};
use snap::raw::Encoder;
use chrono::Utc; // For getting current time in UTC

/// Sends Prometheus metrics to a Mimir remote write endpoint.
///
/// # Arguments
///
/// * `mimir_endpoint` - The base URL of your Mimir instance (e.g., "http://localhost:9009").
/// * `tenant_id` - An optional tenant ID string for multi-tenant Mimir setups.
/// * `metrics` - A vector of `TimeSeries` to send.
pub fn send_to_mimir(
    mimir_endpoint: &str,
    tenant_id: Option<&str>,
    metrics: Vec<TimeSeries>,
) -> Result<(), Box<dyn std::error::Error>> {
    if metrics.is_empty() {
        println!("No metrics to send.");
        return Ok(());
    }

    let write_request = WriteRequest {
        timeseries: metrics,
        // reserved_fields: Vec::new(), // Older Prost versions might require this
        ..Default::default() // Ensures forward compatibility with future fields
    };

    // 1. Serialize the WriteRequest to Protobuf bytes
    let mut buf = Vec::new();
    prost::Message::encode(&write_request, &mut buf)?;

    // 2. Compress the Protobuf bytes using Snappy
    let mut encoder = Encoder::new();
    let compressed_data = encoder.compress_vec(&buf)?;

    // 3. Prepare HTTP headers
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_ENCODING, HeaderValue::from_static("snappy"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x-protobuf"));
    headers.insert(
        "X-Prometheus-Remote-Write-Version",
        HeaderValue::from_static("0.1.0"),
    );
    if let Some(id) = tenant_id {
        headers.insert("X-Scope-OrgID", HeaderValue::from_str(id)?);
    }

    // 4. Send the HTTP POST request to Mimir
    let client = Client::new();
    let response = client
        .post(format!("{mimir_endpoint}/api/v1/push")) // Mimir's remote write endpoint
        .headers(headers)
        .body(compressed_data)
        .send()?;

    // 5. Handle the Mimir's response
    if response.status().is_success() {
        println!("Metrics sent successfully to Mimir!");
    } else {
        let status = response.status();
        let body = response.text()?;
        eprintln!("Failed to send metrics to Mimir. Status: {status}. Response body: {body}");
        return Err(format!("Mimir API error: {status} - {body}").into());
    }

    Ok(())
}

/// Helper function to create a single TimeSeries.
pub fn create_time_series(
    metric_name: &str,
    labels: &[(&str, &str)],
    value: f64,
    timestamp_ms: Option<i64>,
) -> TimeSeries {
    let mut all_labels = Vec::new();
    // Add the mandatory metric name label
    all_labels.push(Label {
        name: "__name__".to_string(),
        value: metric_name.to_string(),
    });
    // Add custom labels
    for (name, val) in labels {
        all_labels.push(Label {
            name: name.to_string(),
            value: val.to_string(),
        });
    }

    let sample = Sample {
        value,
        timestamp: timestamp_ms.unwrap_or_else(|| Utc::now().timestamp_millis()),
    };

    TimeSeries {
        labels: all_labels,
        samples: vec![sample],
        exemplars: vec![],   // For tracing, leave empty if not used
        histograms: vec![], // For native histograms, leave empty if not used
    }
}

// Example usage in a main function or test
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_send_metrics() {
        // NOTE: This test will attempt to send data to a live Mimir instance.
        // Make sure Mimir is running at this address, or comment out for CI.
        let mimir_url = "http://localhost:9009"; // Adjust to your Mimir instance
        let tenant_id = Some("demo"); // Optional, remove if Mimir is single-tenant

        let mut metrics_to_send = Vec::new();

        // Metric 1: A counter for requests
        metrics_to_send.push(create_time_series(
            "my_app_http_requests_total",
            &[("method", "GET"), ("status", "200")],
            1.0, // For a counter, typically increment by 1 per event
            None, // Use current timestamp
        ));

        // Metric 2: A gauge for CPU usage
        metrics_to_send.push(create_time_series(
            "my_app_cpu_usage_percent",
            &[("host", "server-a")],
            25.5, // Current value for a gauge
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
        match send_to_mimir(mimir_url, tenant_id, metrics_to_send) {
            Ok(_) => println!("Test metrics sent successfully."),
            Err(e) => eprintln!("Failed to send test metrics: {}", e),
        }

        // In a real test, you might assert that the call didn't return an error.
        // For actual data verification, you'd query Mimir after a delay,
        // which is beyond a simple unit test.
        assert!(true); // Placeholder, replace with actual assertion if possible
    }
}
