//! Curated provider/model catalog generation.
//!
//! The pipeline is exposed here so it can be tested without spawning the
//! binary: every stage is a pure function of its upstream bodies, and
//! the tests in `tests/` drive them over recorded fixtures.

pub mod build;
pub mod catalog;
pub mod commandcode;
pub mod fetch;
pub mod kenari;
pub mod layout;
pub mod sources;

use std::path::PathBuf;

/// The repository root, resolved from this crate's manifest so every
/// path is the same from any working directory.
#[must_use]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}
