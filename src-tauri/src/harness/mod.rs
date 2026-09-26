//! The harness protocol: a vendor-neutral way for a local AI harness to say
//! what it is running.
//!
//! The shape of this module is the design:
//!
//! - [`protocol`] owns the model, the status enum and every validation rule.
//!   It is pure, so the protocol can be tested without a running app.
//! - [`registry`] owns the in-memory state and is the only place that decides
//!   what "active" means. Nothing here is persisted: harness state describes
//!   live processes and is therefore ephemeral by nature.
//! - [`server`] is the push transport, a loopback-only HTTP endpoint that
//!   translates requests into registry calls and nothing more.
//! - [`adapter`] is the pull seam. Only a reference local-JSON adapter exists,
//!   because Phase 4 must prove the architecture without integrating a vendor.
//!
//! Why the boundary sits here: a Codex or DeepSeek adapter belongs *in front of*
//! this model, translating its own format into [`protocol::HarnessSnapshot`].
//! If the core had to understand a vendor, every new harness would mean
//! changing the registry, the note and the tests. Instead a new harness is a
//! new adapter and nothing else moves.

pub mod adapter;
pub mod protocol;
pub mod registry;
pub mod server;

use std::sync::Arc;

use tauri::State;

use protocol::HarnessSnapshot;
use registry::{ActiveHarnessTask, HarnessRegistry};

/// The registry Tauri manages, shared with the HTTP server thread.
///
/// A newtype because `Arc<HarnessRegistry>` cannot be `State` and an owned
/// handle at the same time: the server needs one clone while commands borrow
/// the other.
pub struct HarnessState(pub Arc<HarnessRegistry>);

impl Default for HarnessState {
    fn default() -> Self {
        Self(Arc::new(HarnessRegistry::default()))
    }
}

impl HarnessState {
    /// The shared registry.
    pub fn registry(&self) -> &Arc<HarnessRegistry> {
        &self.0
    }
}

/// Install the registry and start the push endpoint.
///
/// Called from `setup`. The returned state is what the app should manage; the
/// server is told to bind on the shared registry. A bind failure is logged and
/// the app carries on, which is the whole reason this returns a value instead
/// of failing.
pub fn init() -> HarnessState {
    let registry = Arc::new(HarnessRegistry::default());
    let state = HarnessState(Arc::clone(&registry));

    match server::start(Arc::clone(&registry), server::default_port()) {
        server::ServerState::Listening(address) => {
            println!(
                "[sticky-harness] harness push endpoint: http://{address}/api/harness/snapshot"
            );
        }
        server::ServerState::Unavailable(reason) => {
            // Notes and the tray must keep working with no harness API at all,
            // so a busy port is logged and the app carries on.
            eprintln!("[sticky-harness] harness push endpoint disabled: {reason}");
        }
    }

    state
}

/// Every active harness task, including ones from a harness that went quiet.
///
/// Kept because it is the honest "everything active" answer and the Phase 4
/// checks use it. The note must not: it uses [`list_live_active_harness_tasks`],
/// which also drops stale harnesses.
#[tauri::command]
pub async fn list_active_harness_tasks(
    state: State<'_, HarnessState>,
) -> Result<Vec<ActiveHarnessTask>, String> {
    Ok(state.registry().list_active_tasks())
}

/// Every active task from a harness that is still reporting.
///
/// This is what the Harness Task Note renders. Both rules - the task is active
/// and the harness is not stale - are applied inside the registry, so the UI
/// never computes staleness or filters snapshots itself.
#[tauri::command]
pub async fn list_live_active_harness_tasks(
    state: State<'_, HarnessState>,
) -> Result<Vec<ActiveHarnessTask>, String> {
    Ok(state.registry().list_live_active_tasks())
}

/// Every stored snapshot, for diagnostics and for the runtime checks.
#[tauri::command]
pub async fn list_harness_snapshots(
    state: State<'_, HarnessState>,
) -> Result<Vec<HarnessSnapshot>, String> {
    Ok(state
        .registry()
        .list()
        .into_iter()
        .map(|stored| stored.snapshot().clone())
        .collect())
}

/// The port the push endpoint is configured to use.
#[tauri::command]
pub async fn harness_api_port() -> Result<u16, String> {
    Ok(server::default_port())
}

/// Whether one harness has gone quiet, using the app's own stale timeout.
#[tauri::command]
pub async fn harness_is_stale(state: State<'_, HarnessState>, harness_id: String) -> Result<bool, String> {
    Ok(state.registry().is_stale(&harness_id))
}

#[cfg(test)]
mod tests;
