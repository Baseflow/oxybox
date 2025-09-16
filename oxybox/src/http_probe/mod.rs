pub mod probe;
pub mod result;

use std::fmt::Write;

fn report(mut err: &(dyn std::error::Error + 'static)) -> String {
    let mut s = format!("{}", err);
    while let Some(src) = err.source() {
        let _ = write!(s, "\n\nCaused by: {}", src);
        err = src;
    }
    s
}
