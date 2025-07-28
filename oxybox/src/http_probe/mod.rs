pub mod probe;
pub mod result;

pub mod prelude {
    pub use super::probe::probe_url;
    pub use super::result::ProbeResult;
}
