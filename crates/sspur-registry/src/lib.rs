//! The SSPUR package registry (ADR 0038): package facts for the publish checks, versions and
//! requirements, the index format, and deterministic source snapshots.

pub mod facts;
pub mod index;
pub mod semver;
pub mod sha256;
pub mod snapshot;
pub mod toml;
