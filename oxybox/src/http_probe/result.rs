/// Struct to hold the results of an HTTP probe.
pub struct ProbeResult {
    /// The URL that was probed.
    pub url: String,

    /// The time taken for DNS resolution, in seconds.
    /// This is the time from when the request was sent until the DNS lookup was completed.
    /// DNS resolution is performed by the configured DNS resolvers.
    pub dns_time: Option<f64>,

    /// The time taken to establish a connection, in seconds.
    /// This is the time from when the request was sent until the TCP connection was established.
    pub connect_time: Option<f64>,

    /// The time taken to establish a TLS connection, in seconds.
    /// This is `None` if the URL is not HTTPS.
    pub tls_time: Option<f64>,

    /// The HTTP status code returned by the server, if available.
    /// If the status code is not available (e.g., in case of a connection error), this will be `None`.
    pub http_status: Option<u16>,

    /// The HTTP version used for the request, represented as a float (e.g., 1.1, 2.0).
    /// If the version is unknown, this will be `None`.
    pub http_version: Option<f64>,

    /// The validity period of the SSL certificate, in seconds.
    /// This is the unix timestamp of the certificate's expiration.
    pub cert_validity_seconds: Option<f64>,

    /// The time taken to process the request, in seconds.
    /// Also known as Time to First Byte (TTFB).
    pub processing_time: Option<f64>,

    /// The time taken to transfer the response, in seconds.
    /// This is the time from when the request was sent until the last byte of the response was received.
    pub transfer_time: Option<f64>,

    /// The total time taken for the probe, in seconds.
    /// This is the sum of all phases: DNS resolution, connection, TLS handshake, processing, and transfer.
    pub total_probe_time: f64,
}
