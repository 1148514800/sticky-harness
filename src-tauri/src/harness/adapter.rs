//! The pull side: the seam a future vendor adapter plugs into.
//!
//! Push (a harness POSTs to us) and pull (we ask a harness what it is doing)
//! end in the same place: a [`HarnessSnapshot`] handed to the registry. This
//! module defines that seam and one minimal reference implementation, so the
//! pull architecture is proven to work without any vendor being wired in.
//!
//! There is deliberately no `CodexAdapter` or `DeepSeekAdapter` here. An
//! adapter for a real harness belongs to a later phase and, when it arrives,
//! must not require any change to [`HarnessRegistry`](super::registry::HarnessRegistry).

// Phase 4 defines this seam but deliberately ships no vendor adapter, so
// outside tests nothing calls it yet. Silencing that here keeps the dead-code
// warning meaningful everywhere else, and the alternative - deleting the trait
// and adding it back in Phase 6 - would defeat the point of defining it now.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use super::protocol::{HarnessSnapshot, ProtocolError};

/// A source that can be asked for the current state of one harness.
///
/// The contract is small on purpose:
///
/// - `poll` returns a snapshot or a readable error, and never panics on
///   external data, because everything it reads comes from outside the app.
/// - The snapshot it returns is validated before it is stored, so an adapter
///   may return a permissive draft rather than duplicating the checks.
/// - `poll` may block; callers run it off the UI thread.
pub trait HarnessAdapter: Send + Sync {
    /// A stable name for logs and errors, such as `"local-json"`.
    fn name(&self) -> &str;

    /// Read the harness's current state. Taking `&self` keeps adapters cheap to
    /// call repeatedly from the async runtime.
    fn poll(&self) -> Result<HarnessSnapshot, ProtocolError>;
}

/// A reference adapter that reads one snapshot from a local JSON file.
///
/// This exists to prove the pull path end to end, not to watch files: there is
/// no polling loop and no filesystem notification here. Its caller decides when
/// to poll, and the file is read fresh on every call so an updated file is
/// picked up without restarting anything.
pub struct LocalJsonAdapter {
    name: String,
    path: std::path::PathBuf,
}

impl LocalJsonAdapter {
    /// Point an adapter at one file. The file is not read until `poll`.
    pub fn new(name: impl Into<String>, path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }

    /// The file this adapter reads.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl HarnessAdapter for LocalJsonAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&self) -> Result<HarnessSnapshot, ProtocolError> {
        let text = fs::read_to_string(&self.path).map_err(|error| {
            format!(
                "{} could not read {}: {error}",
                self.name,
                self.path.display()
            )
        })?;
        parse_local_snapshot(&text)
    }
}

/// Parse and validate a snapshot from JSON text.
///
/// Split out from [`LocalJsonAdapter::poll`] so the protocol can be tested
/// without touching the filesystem, which is the part that actually matters:
/// the file is incidental, the shape and the checks are not.
///
/// Both failure modes return a message rather than panicking - malformed JSON
/// from outside the app must never be able to bring the app down.
pub fn parse_local_snapshot(text: &str) -> Result<HarnessSnapshot, ProtocolError> {
    let snapshot: HarnessSnapshot = serde_json::from_str(text)
        .map_err(|error| format!("malformed harness snapshot JSON: {error}"))?;
    snapshot.validate()
}
