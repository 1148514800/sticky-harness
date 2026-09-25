//! The in-memory harness registry: the single owner of "what is running now".
//!
//! The registry keeps only the latest snapshot per harness. It is deliberately
//! not a history and deliberately not persisted: harness state describes live
//! processes, so after a restart an empty registry is the truth rather than
//! data loss. Nothing else in the app may hold harness state, and React only
//! ever reads it through a command.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;

use super::protocol::{
    now_millis, HarnessSnapshot, HarnessStatus, ProtocolError, ValidatedSnapshot,
    DEFAULT_STALE_TIMEOUT_MILLIS,
};



/// One active task plus the harness it belongs to, flattened for display.
///
/// This is the view model Phase 5 should be built on. It carries only what the
/// note shows - who is running, what they are doing, when they started and in
/// what state - and nothing from a future, larger protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActiveHarnessTask {
    pub harness_id: String,
    pub harness_name: String,
    pub task_id: String,
    pub title: String,
    pub status: HarnessStatus,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The registry itself. Managed by Tauri as `State<HarnessRegistry>`.
#[derive(Debug)]
pub struct HarnessRegistry {
    snapshots: Mutex<HashMap<String, ValidatedSnapshot>>,
    stale_timeout: Duration,
}

impl Default for HarnessRegistry {
    fn default() -> Self {
        Self::with_stale_timeout(Duration::from_millis(DEFAULT_STALE_TIMEOUT_MILLIS))
    }
}

impl HarnessRegistry {
    /// A registry with an explicit staleness timeout. Tests use this to avoid
    /// sleeping; production uses [`Default`].
    pub fn with_stale_timeout(stale_timeout: Duration) -> Self {
        Self {
            snapshots: Mutex::new(HashMap::new()),
            stale_timeout,
        }
    }

    /// A poisoned lock must not take the app down, so recover the data instead.
    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Validate and store a snapshot, replacing that harness's previous one.
    ///
    /// This is the only way state enters the registry. Replacement is by
    /// `harness_id`: a harness that reports again is updated, never duplicated,
    /// which is what keeps "how many harnesses are running" honest. A rejected
    /// snapshot leaves the registry exactly as it was.
    pub fn upsert(&self, snapshot: HarnessSnapshot) -> Result<ValidatedSnapshot, ProtocolError> {
        let snapshot = snapshot.validate()?;
        let stored = ValidatedSnapshot {
            snapshot,
            received_at: now_millis(),
        };

        let mut snapshots = Self::lock(&self.snapshots);
        snapshots.insert(stored.snapshot.harness_id.clone(), stored.clone());
        Ok(stored)
    }

    /// Install an already-validated snapshot, skipping validation.
    ///
    /// Used by tests and by an eventual trusted adapter, which has already had
    /// its output validated once. Not called by the app in this phase, so the
    /// dead-code lint is silenced deliberately instead of deleting the seam.
    #[allow(dead_code)]
    pub fn store(&self, snapshot: ValidatedSnapshot) {
        let mut snapshots = Self::lock(&self.snapshots);
        snapshots.insert(snapshot.snapshot.harness_id.clone(), snapshot);
    }

    /// Every stored snapshot, ordered by harness id so output is stable.
    pub fn list(&self) -> Vec<ValidatedSnapshot> {
        let snapshots = Self::lock(&self.snapshots);
        let mut all: Vec<ValidatedSnapshot> = snapshots.values().cloned().collect();
        all.sort_by(|left, right| left.snapshot.harness_id.cmp(&right.snapshot.harness_id));
        all
    }

    /// One harness's latest snapshot, if it has ever reported.
    pub fn get(&self, harness_id: &str) -> Option<ValidatedSnapshot> {
        let snapshots = Self::lock(&self.snapshots);
        snapshots.get(harness_id).cloned()
    }

    /// Every task that is currently work in progress, across all harnesses.
    ///
    /// This is the whole contract Phase 5 needs: the note asks for active tasks
    /// and never scans or filters the registry itself, so the meaning of
    /// "active" stays in [`HarnessStatus::is_active`].
    ///
    /// Ordered by harness id, then start time, then task id, so a repeated call
    /// with unchanged state returns an unchanged list.
    pub fn list_active_tasks(&self) -> Vec<ActiveHarnessTask> {
        let mut active = Vec::new();

        for stored in self.list() {
            for task in &stored.snapshot.tasks {
                if !task.is_active() {
                    continue;
                }
                active.push(ActiveHarnessTask {
                    harness_id: stored.snapshot.harness_id.clone(),
                    harness_name: stored.snapshot.harness_name.clone(),
                    task_id: task.task_id.clone(),
                    title: task.title.clone(),
                    status: task.status,
                    started_at: task.started_at,
                    updated_at: task.updated_at,
                    message: task.message.clone(),
                });
            }
        }

        active.sort_by(|left, right| {
            left.harness_id
                .cmp(&right.harness_id)
                .then(left.started_at.cmp(&right.started_at))
                .then(left.task_id.cmp(&right.task_id))
        });

        active
    }

    /// Whether one harness's snapshot has gone quiet.
    ///
    /// A harness that never reported is not stale, it is absent, so this
    /// returns `false` for an unknown id. Judging is left to the caller: this
    /// phase never deletes a harness on its own, because a harness that is slow
    /// to report is far more common than one that is genuinely gone.
    pub fn is_stale(&self, harness_id: &str) -> bool {
        let Some(stored) = self.get(harness_id) else {
            return false;
        };
        stored.is_stale(now_millis(), self.stale_timeout)
    }

    /// The harness ids whose snapshots have gone quiet.
    ///
    /// Reported rather than acted on: this phase never deletes a harness by
    /// itself, so nothing in the app calls this yet. It exists so a later
    /// decision about stale harnesses has one place to start from.
    #[allow(dead_code)]
    pub fn stale_harness_ids(&self) -> Vec<String> {
        let now = now_millis();
        let mut stale: Vec<String> = self
            .list()
            .into_iter()
            .filter(|stored| stored.is_stale(now, self.stale_timeout))
            .map(|stored| stored.snapshot.harness_id)
            .collect();
        stale.sort();
        stale
    }

    /// Forget one harness, for an explicit "this is gone" signal.
    ///
    /// Not called by staleness: Phase 4 exposes it so a harness can be removed
    /// deliberately, and so tests can show removal does not need a restart.
    #[allow(dead_code)]
    pub fn remove(&self, harness_id: &str) -> bool {
        let mut snapshots = Self::lock(&self.snapshots);
        snapshots.remove(harness_id).is_some()
    }

    /// How many harnesses have reported.
    pub fn len(&self) -> usize {
        Self::lock(&self.snapshots).len()
    }

    /// Whether no harness has reported.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        Self::lock(&self.snapshots).is_empty()
    }
}
