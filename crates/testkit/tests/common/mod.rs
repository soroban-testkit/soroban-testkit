//! Shared helpers for the repository-level regression tests.
//!
//! These tests guard wiring between `Cargo.toml`, the CI workflows, and
//! the documentation — guarantees that compiling the crate alone cannot
//! check.

use std::fs;
use std::path::PathBuf;

/// Read a file relative to the repository root, panicking with its path
/// and the underlying error if it is missing.
///
/// A missing file is itself a broken guarantee here, never a skip: every
/// file these tests read is committed on `main`.
pub fn read_repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}
