//! The vendor-neutral harness protocol: model, validation and constants.
//!
//! This module is the only place that decides what a harness snapshot *is*.
//! Nothing here knows about Codex, DeepSeek or any other harness: an external
//! producer speaks this shape, or a future adapter translates its own format
//! into it. That is the whole point of the protocol - a vendor adapter goes in
//! front of this model, never inside it.
//!
//! Everything in this file is pure: it takes values, returns values and
//! performs no I/O and no locking, which is what makes it directly testable.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The local HTTP port the push endpoint listens on.
///
/// Kept in one place so a future settings screen can change it without hunting
/// through the code. It only ever binds `127.0.0.1`.
pub const HARNESS_API_PORT: u16 = 17899;

/// The largest snapshot body the push endpoint will read, in bytes.
///
/// A snapshot is a summary of what is running, not a transcript, so anything
/// past this is a mistake or an attack rather than real data.
pub const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;

/// The largest number of tasks accepted in a single snapshot.
pub const MAX_TASKS_PER_SNAPSHOT: usize = 256;

/// The shortest allowed identifier length.
pub const MIN_ID_CHARS: usize = 1;

/// The longest allowed identifier length, for both harness and task ids.
///
/// Ids are never turned into file paths in this phase; the limit exists so a
/// hostile payload cannot make the app hold or log something enormous.
pub const MAX_ID_CHARS: usize = 128;

/// The longest allowed human-readable label (harness name, task title, message).
pub const MAX_LABEL_CHARS: usize = 512;

/// A harness snapshot older than this is treated as stale.
///
/// Five minutes is long enough that a harness reporting every few seconds is
/// never wrongly called dead, and short enough that one which died mid-task
/// stops looking active before the user notices it on the note.
pub const DEFAULT_STALE_TIMEOUT_MILLIS: u64 = 5 * 60 * 1000;

/// The status of one harness task.
///
/// Deliberately small. `Unknown` exists for one reason only: a future adapter
/// may meet a producer state it cannot map, and reporting that honestly beats
/// guessing `Running` (which would claim work is happening) or `Failed` (which
/// would claim it broke).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessStatus {
    Running,
    Waiting,
    Failed,
    Completed,
    Cancelled,
    Unknown,
}

impl HarnessStatus {
    /// Whether a task in this state counts as work in progress.
    ///
    /// This is the one definition of "active" in the app. Phase 5's note must
    /// ask this rather than re-implementing the rule, so the meaning of active
    /// can only ever change in one place.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Waiting)
    }

    /// The protocol spelling of this status, which is also what it parses from.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for HarnessStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for HarnessStatus {
    type Err = String;

    /// Parse a status without ever panicking.
    ///
    /// Accepts the protocol spelling, case-insensitively and without surrounding
    /// whitespace, because a harness that sends `"Running"` is not wrong enough
    /// to reject. Anything else is an error naming the value, so the producer
    /// gets a usable message instead of a silent drop.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "failed" => Ok(Self::Failed),
            "completed" => Ok(Self::Completed),
            "cancelled" | "canceled" => Ok(Self::Cancelled),
            "unknown" => Ok(Self::Unknown),
            other => Err(format!(
                "unknown status \"{other}\"; expected one of running, waiting, failed, completed, cancelled, unknown"
            )),
        }
    }
}

/// Where a snapshot came from.
///
/// `type` names the transport, not the vendor: a Codex harness and a DeepSeek
/// harness both arrive as `Push`. `name` is the free-form detail a producer may
/// add, such as `"codex-local"` or a file path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSource {
    #[serde(rename = "type", default = "default_source_type")]
    pub source_type: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// The transport used when a producer does not say.
pub fn default_source_type() -> String {
    "push".to_string()
}

impl HarnessSource {
    /// The source of a snapshot that arrived over HTTP.
    pub fn push() -> Self {
        Self {
            source_type: default_source_type(),
            name: None,
        }
    }
}

/// One task inside a harness snapshot.
///
/// Times are Unix milliseconds so the UI can compute elapsed time itself; the
/// protocol never carries a pre-rendered "running for 5 minutes" string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessTask {
    pub task_id: String,
    pub title: String,
    pub status: HarnessStatus,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub message: Option<String>,
}

impl HarnessTask {
    /// Whether this task is work in progress.
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }
}

/// Everything one harness is doing, as of `updated_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSnapshot {
    pub harness_id: String,
    pub harness_name: String,
    #[serde(default)]
    pub source: HarnessSource,
    /// Unix milliseconds. The producer's own clock, not when we received it.
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub tasks: Vec<HarnessTask>,
}

/// A snapshot that has been checked and accepted, with its arrival time.
///
/// The registry stores only this, so an unvalidated payload can never reach
/// application state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSnapshot {
    pub snapshot: HarnessSnapshot,
    /// Unix milliseconds when this process accepted the snapshot.
    pub received_at: u64,
}

impl ValidatedSnapshot {
    /// Whether this snapshot has gone quiet for longer than `timeout`.
    ///
    /// Staleness is judged from the newest timestamp we have: normally the
    /// producer's `updated_at`, but never earlier than when we received it, so
    /// a producer with a badly wrong clock cannot make a snapshot look stale
    /// the moment it arrives.
    pub fn is_stale(&self, now: u64, timeout: Duration) -> bool {
        let fresh_enough = self.last_seen().max(self.received_at);
        now.saturating_sub(fresh_enough) > timeout.as_millis() as u64
    }

    /// The newest timestamp this snapshot carries, ignoring `received_at`.
    fn last_seen(&self) -> u64 {
        let newest_task = self
            .snapshot
            .tasks
            .iter()
            .map(|task| task.updated_at)
            .max()
            .unwrap_or(0);
        self.snapshot.updated_at.max(newest_task)
    }

    /// The snapshot's content as of `now`, for a consumer that does not care
    /// about staleness. Stale snapshots are still returned: this phase does not
    /// silently delete a harness that stopped reporting.
    pub fn snapshot(&self) -> &HarnessSnapshot {
        &self.snapshot
    }
}

/// The error type for every protocol-level rejection.
///
/// A single string type keeps the HTTP layer trivial: whatever a validator
/// rejects becomes a 4xx body, and no failure path needs a panic.
pub type ProtocolError = String;

impl HarnessTask {
    /// Check one task, returning the first problem found.
    fn validate(&self) -> Result<(), ProtocolError> {
        validate_id("task_id", &self.task_id)?;

        if self.title.trim().is_empty() {
            return Err("task title must not be empty".to_string());
        }
        validate_label("task title", &self.title)?;

        if let Some(message) = &self.message {
            validate_label("task message", message)?;
        }

        // A task that finished before it started is a producer bug worth
        // reporting, not something to quietly fix up here.
        if self.updated_at < self.started_at {
            return Err(format!(
                "task \"{}\" has updated_at before started_at",
                self.task_id
            ));
        }

        Ok(())
    }
}

impl HarnessSnapshot {
    /// Check a snapshot that came from outside the app.
    ///
    /// Returns the snapshot unchanged so the caller can chain validation into
    /// storage. Everything rejected here is rejected before it can touch
    /// application state.
    pub fn validate(self) -> Result<Self, ProtocolError> {
        validate_id("harness_id", &self.harness_id)?;

        if self.harness_name.trim().is_empty() {
            return Err("harness_name must not be empty".to_string());
        }
        validate_label("harness_name", &self.harness_name)?;

        if let Some(name) = &self.source.name {
            validate_label("source name", name)?;
        }

        if self.tasks.len() > MAX_TASKS_PER_SNAPSHOT {
            return Err(format!(
                "snapshot has {} tasks; the limit is {MAX_TASKS_PER_SNAPSHOT}",
                self.tasks.len()
            ));
        }

        let mut seen = std::collections::HashSet::new();
        for task in &self.tasks {
            task.validate()?;
            if !seen.insert(task.task_id.as_str()) {
                return Err(format!(
                    "snapshot repeats task_id \"{}\"; ids must be unique within a snapshot",
                    task.task_id
                ));
            }
        }

        Ok(self)
    }

    /// Whether this snapshot has gone quiet, judged the same way the registry
    /// judges a stored one.
    ///
    /// Public API for an adapter that wants to judge its own snapshot before
    /// offering it; not called by the app in this phase.
    #[allow(dead_code)]
    pub fn is_stale_at(&self, now: u64, timeout: Duration) -> bool {
        let newest_task = self
            .tasks
            .iter()
            .map(|task| task.updated_at)
            .max()
            .unwrap_or(0);
        now.saturating_sub(self.updated_at.max(newest_task)) > timeout.as_millis() as u64
    }
}

/// Reject an identifier that is empty, blank, oversized or has control characters.
fn validate_id(field: &str, value: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    let length = value.chars().count();
    if length < MIN_ID_CHARS || length > MAX_ID_CHARS {
        return Err(format!(
            "{field} must be between {MIN_ID_CHARS} and {MAX_ID_CHARS} characters, got {length}"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

/// Reject a human-readable label that is empty or oversized.
fn validate_label(field: &str, value: &str) -> Result<(), ProtocolError> {
    if value.chars().count() > MAX_LABEL_CHARS {
        return Err(format!(
            "{field} must be at most {MAX_LABEL_CHARS} characters"
        ));
    }
    Ok(())
}

/// Unix milliseconds, or 0 when the system clock is before the epoch.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

use std::time::Duration;
