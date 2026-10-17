//! HTTP and local fetching of the upstream catalogs.

use std::fs;
use std::io::Read as _;
use std::time::Duration;

/// Hard cap on a downloaded body. models.dev is ~5.7 MB today, so this
/// leaves generous headroom while still failing loudly rather than
/// filling memory if a URL starts serving something unbounded.
const DOWNLOAD_LIMIT: usize = 32 * 1024 * 1024;

/// How long the whole request may take.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The `kage-models` version, sent as the user agent so an upstream
/// operator can tell who is reading.
const USER_AGENT: &str = concat!("kage-models/", env!("CARGO_PKG_VERSION"));

/// Read `url` as text. A `file://` URL reads a local path instead, which
/// is how the tests and an offline run feed recorded fixtures.
///
/// # Errors
///
/// The request fails, the body exceeds [`DOWNLOAD_LIMIT`], the body is
/// not UTF-8, or a `file://` path cannot be read.
pub fn fetch(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let response = agent
        .get(url)
        .header("user-agent", USER_AGENT)
        .call()
        .map_err(|e| format!("get {url}: {e}"))?;
    read_limited(response.into_body().into_reader(), DOWNLOAD_LIMIT)
}

/// `reader` decoded as UTF-8, failing once it exceeds `limit` bytes
/// instead of silently truncating mid-frame.
fn read_limited(reader: impl std::io::Read, limit: usize) -> Result<String, String> {
    let mut reader = reader;
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read body: {e}"))?;
    if bytes.len() > limit {
        return Err(format!("body exceeds {} MiB", limit / (1024 * 1024)));
    }
    String::from_utf8(bytes).map_err(|e| format!("body is not utf-8: {e}"))
}
