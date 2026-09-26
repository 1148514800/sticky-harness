//! Tests for the protocol core: validation, the registry and staleness.
//!
//! These run without a running app, a window or a port. The HTTP layer is
//! exercised at runtime instead, because a test that binds a real socket is
//! exactly the kind of thing that fails on a busy machine and teaches nothing.

use std::time::Duration;

use super::adapter::{parse_local_snapshot, HarnessAdapter, LocalJsonAdapter};
use super::protocol::{
    HarnessSnapshot, HarnessSource, HarnessStatus, HarnessTask, MAX_ID_CHARS, MAX_TASKS_PER_SNAPSHOT,
};
use super::registry::HarnessRegistry;

/// A snapshot with one running task, used as the baseline for most tests.
fn running_snapshot(harness_id: &str) -> HarnessSnapshot {
    HarnessSnapshot {
        harness_id: harness_id.to_string(),
        harness_name: format!("Harness {harness_id}"),
        source: HarnessSource::push(),
        updated_at: 1_700_000_000_000,
        tasks: vec![task("task-1", "Run tests", HarnessStatus::Running)],
    }
}

fn task(task_id: &str, title: &str, status: HarnessStatus) -> HarnessTask {
    started(task_id, title, status, 1_700_000_000_000)
}

/// A task that began at a specific time, for ordering assertions.
fn started(task_id: &str, title: &str, status: HarnessStatus, started_at: u64) -> HarnessTask {
    HarnessTask {
        task_id: task_id.to_string(),
        title: title.to_string(),
        status,
        started_at,
        updated_at: started_at,
        message: None,
    }
}

#[test]
fn status_parses_every_protocol_spelling() {
    let cases = [
        ("running", HarnessStatus::Running),
        ("waiting", HarnessStatus::Waiting),
        ("failed", HarnessStatus::Failed),
        ("completed", HarnessStatus::Completed),
        ("cancelled", HarnessStatus::Cancelled),
        ("unknown", HarnessStatus::Unknown),
    ];

    for (raw, expected) in cases {
        assert_eq!(raw.parse::<HarnessStatus>(), Ok(expected), "for {raw}");
    }
}

#[test]
fn status_parsing_is_forgiving_about_case_and_space() {
    assert_eq!("Running".parse::<HarnessStatus>(), Ok(HarnessStatus::Running));
    assert_eq!(
        "  WAITING  ".parse::<HarnessStatus>(),
        Ok(HarnessStatus::Waiting)
    );
    assert_eq!(
        "completed".parse::<HarnessStatus>(),
        Ok(HarnessStatus::Completed)
    );
}

#[test]
fn status_parsing_rejects_unknown_values_with_a_readable_message() {
    let error = "in_progress".parse::<HarnessStatus>().unwrap_err();
    assert!(error.contains("in_progress"), "message was: {error}");
    assert!(error.contains("running"), "message was: {error}");
}

#[test]
fn status_round_trips_through_its_own_spelling() {
    for status in [
        HarnessStatus::Running,
        HarnessStatus::Waiting,
        HarnessStatus::Failed,
        HarnessStatus::Completed,
        HarnessStatus::Cancelled,
        HarnessStatus::Unknown,
    ] {
        assert_eq!(status.as_str().parse::<HarnessStatus>(), Ok(status));
    }
}

#[test]
fn active_is_running_and_waiting_only() {
    assert!(HarnessStatus::Running.is_active());
    assert!(HarnessStatus::Waiting.is_active());
    assert!(!HarnessStatus::Completed.is_active());
    assert!(!HarnessStatus::Cancelled.is_active());
    assert!(!HarnessStatus::Failed.is_active());
    assert!(!HarnessStatus::Unknown.is_active());
}

#[test]
fn a_well_formed_snapshot_validates() {
    assert!(running_snapshot("harness-a").validate().is_ok());
}

#[test]
fn a_snapshot_without_a_harness_id_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.harness_id = "   ".to_string();

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("harness_id"), "message was: {error}");
}

#[test]
fn a_snapshot_without_a_name_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.harness_name = String::new();

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("harness_name"), "message was: {error}");
}

#[test]
fn an_oversized_harness_id_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.harness_id = "h".repeat(MAX_ID_CHARS + 1);

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("harness_id"), "message was: {error}");
}

#[test]
fn an_id_at_the_limit_is_accepted() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.harness_id = "h".repeat(MAX_ID_CHARS);

    assert!(snapshot.validate().is_ok());
}

#[test]
fn an_id_with_control_characters_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.harness_id = "harness\u{0}a".to_string();

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("control"), "message was: {error}");
}

#[test]
fn a_task_without_a_title_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.tasks[0].title = "  ".to_string();

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("title"), "message was: {error}");
}

#[test]
fn a_task_that_updated_before_it_started_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.tasks[0].started_at = 2_000;
    snapshot.tasks[0].updated_at = 1_000;

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("updated_at"), "message was: {error}");
}

#[test]
fn duplicate_task_ids_in_one_snapshot_are_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.tasks.push(task("task-1", "Also running", HarnessStatus::Running));

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("task-1"), "message was: {error}");
}

#[test]
fn an_oversized_task_list_is_rejected() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.tasks = (0..=MAX_TASKS_PER_SNAPSHOT)
        .map(|index| {
            task(
                &format!("task-{index}"),
                "Run tests",
                HarnessStatus::Running,
            )
        })
        .collect();

    let error = snapshot.validate().unwrap_err();
    assert!(error.contains("limit"), "message was: {error}");
}

#[test]
fn a_snapshot_with_no_tasks_is_valid() {
    let mut snapshot = running_snapshot("harness-a");
    snapshot.tasks.clear();

    assert!(snapshot.validate().is_ok());
}

#[test]
fn an_empty_registry_has_nothing_active() {
    let registry = HarnessRegistry::default();

    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(registry.list().is_empty());
    assert!(registry.list_active_tasks().is_empty());
}

#[test]
fn the_first_upsert_registers_a_harness() {
    let registry = HarnessRegistry::default();

    let stored = registry.upsert(running_snapshot("harness-a")).unwrap();

    assert_eq!(stored.snapshot.harness_id, "harness-a");
    assert_eq!(registry.len(), 1);
    assert_eq!(registry.list_active_tasks().len(), 1);
}

#[test]
fn a_second_upsert_replaces_instead_of_appending() {
    let registry = HarnessRegistry::default();
    registry.upsert(running_snapshot("harness-a")).unwrap();

    let mut updated = running_snapshot("harness-a");
    updated.tasks = vec![
        task("task-1", "Run tests", HarnessStatus::Completed),
        task("task-2", "Write report", HarnessStatus::Running),
    ];
    registry.upsert(updated).unwrap();

    // One harness, still, and only the task that is actually running.
    assert_eq!(registry.len(), 1);
    let active = registry.list_active_tasks();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].task_id, "task-2");

    let stored = registry.get("harness-a").unwrap();
    assert_eq!(stored.snapshot.tasks.len(), 2, "history is not kept, but the latest snapshot is complete");
}

#[test]
fn a_rejected_upsert_leaves_the_registry_untouched() {
    let registry = HarnessRegistry::default();
    registry.upsert(running_snapshot("harness-a")).unwrap();

    let mut broken = running_snapshot("harness-a");
    broken.harness_id = String::new();
    assert!(registry.upsert(broken).is_err());

    assert_eq!(registry.len(), 1);
    assert_eq!(
        registry.get("harness-a").unwrap().snapshot.tasks.len(),
        1,
        "the good snapshot must survive a rejected one"
    );
}

#[test]
fn multiple_harnesses_coexist() {
    let registry = HarnessRegistry::default();
    registry.upsert(running_snapshot("harness-a")).unwrap();
    registry.upsert(running_snapshot("harness-b")).unwrap();

    assert_eq!(registry.len(), 2);

    let active = registry.list_active_tasks();
    assert_eq!(active.len(), 2);
    let harnesses: Vec<&str> = active
        .iter()
        .map(|task| task.harness_id.as_str())
        .collect();
    assert_eq!(harnesses, vec!["harness-a", "harness-b"]);
}

#[test]
fn active_filtering_keeps_only_in_progress_tasks() {
    let registry = HarnessRegistry::default();
    let mut snapshot = running_snapshot("harness-a");
    // Distinct start times so this asserts the *filter*, not the id tiebreak:
    // only "running" and "waiting" survive, in the order they started.
    snapshot.tasks = vec![
        started("done", "Finished", HarnessStatus::Completed, 1_000),
        started("running", "Working", HarnessStatus::Running, 2_000),
        started("failed", "Broke", HarnessStatus::Failed, 3_000),
        started("waiting", "Blocked", HarnessStatus::Waiting, 4_000),
        started("cancelled", "Stopped", HarnessStatus::Cancelled, 5_000),
        started("mystery", "No idea", HarnessStatus::Unknown, 6_000),
    ];
    registry.upsert(snapshot).unwrap();

    let active = registry.list_active_tasks();
    let ids: Vec<&str> = active.iter().map(|task| task.task_id.as_str()).collect();

    assert_eq!(ids, vec!["running", "waiting"], "only active tasks, by start time");
}

#[test]
fn active_tasks_with_the_same_start_time_fall_back_to_task_id() {
    let registry = HarnessRegistry::default();
    let mut snapshot = running_snapshot("harness-a");
    snapshot.tasks = vec![
        task("b-task", "Second", HarnessStatus::Running),
        task("a-task", "First", HarnessStatus::Running),
    ];
    registry.upsert(snapshot).unwrap();

    let ids: Vec<String> = registry
        .list_active_tasks()
        .iter()
        .map(|active| active.task_id.clone())
        .collect();

    assert_eq!(ids, vec!["a-task", "b-task"]);
}

#[test]
fn active_tasks_carry_the_harness_they_belong_to() {
    let registry = HarnessRegistry::default();
    registry.upsert(running_snapshot("harness-a")).unwrap();

    let active = registry.list_active_tasks();
    assert_eq!(active[0].harness_id, "harness-a");
    assert_eq!(active[0].harness_name, "Harness harness-a");
    assert_eq!(active[0].title, "Run tests");
    assert_eq!(active[0].started_at, 1_700_000_000_000);
}

#[test]
fn active_task_order_is_stable_across_calls() {
    let registry = HarnessRegistry::default();
    registry.upsert(running_snapshot("harness-b")).unwrap();
    registry.upsert(running_snapshot("harness-a")).unwrap();

    let first = registry.list_active_tasks();
    let second = registry.list_active_tasks();
    assert_eq!(first, second);
}

#[test]
fn a_harness_that_never_reported_is_not_stale() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(1));

    assert!(!registry.is_stale("never-reported"));
    assert!(registry.stale_harness_ids().is_empty());
}

#[test]
fn a_fresh_snapshot_is_not_stale() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));
    let now = super::protocol::now_millis();
    let mut snapshot = running_snapshot("harness-a");
    snapshot.updated_at = now;
    snapshot.tasks[0].started_at = now;
    snapshot.tasks[0].updated_at = now;
    registry.upsert(snapshot).unwrap();

    assert!(!registry.is_stale("harness-a"));
}

#[test]
fn a_quiet_snapshot_goes_stale_after_the_timeout() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(50));
    let mut snapshot = running_snapshot("harness-a");
    // Old enough that any sane timeout has already passed, and the producer's
    // own clock is the only thing that can make it look fresh.
    snapshot.updated_at = 1;
    snapshot.tasks[0].started_at = 1;
    snapshot.tasks[0].updated_at = 1;
    registry.upsert(snapshot).unwrap();

    // Stored "now" is the arrival time, so it is fresh at first...
    assert!(!registry.is_stale("harness-a"), "a just-received snapshot is never instantly stale");

    // ...but judged against a later moment, it has clearly gone quiet.
    let stored = registry.get("harness-a").unwrap();
    assert!(stored.is_stale(stored.received_at + 60_000, Duration::from_millis(50)));
}

#[test]
fn staleness_never_fires_before_the_timeout() {
    let timeout = Duration::from_millis(1_000);
    let mut snapshot = running_snapshot("harness-a");
    snapshot.updated_at = 10_000;
    snapshot.tasks[0].started_at = 10_000;
    snapshot.tasks[0].updated_at = 10_000;

    let registry = HarnessRegistry::default();
    let stored = registry.upsert(snapshot).unwrap();
    let received = stored.received_at;

    assert!(!stored.is_stale(received, timeout));
    assert!(!stored.is_stale(received + 1_000, timeout), "exactly at the timeout is still fresh");
    assert!(stored.is_stale(received + 1_001, timeout));
}

#[test]
fn stale_ids_list_what_has_gone_quiet() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    registry.upsert(running_snapshot("harness-a")).unwrap();
    registry.upsert(running_snapshot("harness-b")).unwrap();

    // With a zero timeout, pushing the clock even 1 ms forward makes both stale.
    let now = super::protocol::now_millis() + 1;
    let mut stale: Vec<String> = registry
        .list()
        .into_iter()
        .filter(|stored| stored.is_stale(now, Duration::from_millis(0)))
        .map(|stored| stored.snapshot.harness_id)
        .collect();
    stale.sort();

    assert_eq!(stale, vec!["harness-a", "harness-b"]);
}

#[test]
fn a_harness_just_received_is_never_stale() {
    // A snapshot cannot be stale the instant it arrives, whatever its own
    // timestamps say: `received_at` is the floor. This is why the check is
    // `max(producer_time, received_at)` and not the producer's clock alone.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    let mut snapshot = running_snapshot("harness-a");
    snapshot.updated_at = 1;
    snapshot.tasks[0].started_at = 1;
    snapshot.tasks[0].updated_at = 1;
    registry.upsert(snapshot).unwrap();

    assert!(!registry.is_stale("harness-a"));
    assert!(registry.stale_harness_ids().is_empty());
}

#[test]
fn a_stale_harness_is_kept_rather_than_deleted() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    let mut snapshot = running_snapshot("harness-a");
    snapshot.updated_at = 1;
    snapshot.tasks[0].started_at = 1;
    snapshot.tasks[0].updated_at = 1;
    let stored = registry.upsert(snapshot).unwrap();

    // Judged a moment later, it has gone quiet. Stale only means "we have not
    // heard from it", so the harness and its task are still there for the note
    // to show: this phase never removes a harness on its own.
    assert!(stored.is_stale(stored.received_at + 1, Duration::from_millis(0)));
    assert_eq!(registry.len(), 1);
    assert_eq!(registry.list_active_tasks().len(), 1);
    assert_eq!(registry.get("harness-a").unwrap().snapshot.tasks.len(), 1);
}

#[test]
fn a_harness_can_be_removed_explicitly() {
    let registry = HarnessRegistry::default();
    registry.upsert(running_snapshot("harness-a")).unwrap();

    assert!(registry.remove("harness-a"));
    assert!(!registry.remove("harness-a"));
    assert!(registry.is_empty());
}

/// A snapshot whose producer clock is far in the past, so staleness can be
/// judged without waiting for a real timeout.
fn stale_snapshot(harness_id: &str) -> HarnessSnapshot {
    let mut snapshot = running_snapshot(harness_id);
    snapshot.updated_at = 1;
    snapshot.tasks[0].started_at = 1;
    snapshot.tasks[0].updated_at = 1;
    snapshot
}

#[test]
fn live_active_tasks_match_active_tasks_when_nothing_is_stale() {
    let registry = HarnessRegistry::default();
    let now = super::protocol::now_millis();
    let mut snapshot = running_snapshot("harness-a");
    snapshot.updated_at = now;
    snapshot.tasks[0].started_at = now;
    snapshot.tasks[0].updated_at = now;
    registry.upsert(snapshot).unwrap();

    assert_eq!(registry.list_live_active_tasks(), registry.list_active_tasks());
}

#[test]
fn an_empty_registry_has_no_live_active_tasks() {
    let registry = HarnessRegistry::default();

    assert!(registry.list_live_active_tasks().is_empty());
}

/// Store a snapshot as if it had arrived long enough ago to have gone quiet.
///
/// `received_at` is the floor that stops a bad producer clock from making a
/// fresh snapshot look dead, so a test that wants real staleness has to age the
/// arrival too - exactly what a long-running app would have done. This is the
/// "inject now" the stale checks are supposed to use, instead of sleeping.
fn store_aged(registry: &HarnessRegistry, snapshot: HarnessSnapshot) {
    let mut stored = registry.upsert(snapshot).unwrap();
    stored.received_at = 1;
    registry.store(stored);
}

#[test]
fn a_just_received_snapshot_is_never_live_stale() {
    // Even with a zero timeout, a snapshot cannot be stale the instant it
    // arrives: this is the received_at floor, restated for the live view.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    registry.upsert(stale_snapshot("harness-a")).unwrap();

    assert_eq!(registry.list_live_active_tasks().len(), 1);
}

#[test]
fn a_stale_harness_is_excluded_from_live_active_tasks_but_kept() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    store_aged(&registry, stale_snapshot("harness-a"));

    // The snapshot is genuinely stale, and is still in the registry...
    assert!(registry.is_stale("harness-a"));
    assert_eq!(registry.len(), 1, "the snapshot itself is never removed");
    assert_eq!(
        registry.get("harness-a").unwrap().snapshot.tasks.len(),
        1,
        "its tasks are never mutated"
    );
    assert_eq!(
        registry.list_active_tasks().len(),
        1,
        "the plain active list ignores staleness"
    );

    // ...but the live view drops it, because that task is not being reported
    // by anything that is still running.
    assert!(
        registry.list_live_active_tasks().is_empty(),
        "a stale harness must not look like a running task"
    );
}

#[test]
fn staleness_excludes_the_whole_harness_not_just_one_task() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    let mut snapshot = stale_snapshot("harness-a");
    snapshot.tasks = vec![
        started("task-1", "First", HarnessStatus::Running, 1),
        started("task-2", "Second", HarnessStatus::Waiting, 2),
    ];
    store_aged(&registry, snapshot);

    assert_eq!(registry.list_active_tasks().len(), 2);
    assert!(registry.list_live_active_tasks().is_empty());
}

#[test]
fn a_fresh_harness_still_shows_while_another_is_stale() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));
    let now = super::protocol::now_millis();

    // Fresh: judged from its own recent timestamps.
    let mut fresh = running_snapshot("harness-fresh");
    fresh.updated_at = now;
    fresh.tasks[0].started_at = now;
    fresh.tasks[0].updated_at = now;
    registry.upsert(fresh).unwrap();

    // Stale: old producer timestamps, aged arrival, so the received_at floor
    // does not mask it.
    store_aged(&registry, stale_snapshot("harness-stale"));

    let live: Vec<String> = registry
        .list_live_active_tasks()
        .into_iter()
        .map(|task| task.harness_id)
        .collect();

    assert_eq!(live, vec!["harness-fresh"]);
    assert_eq!(registry.list_active_tasks().len(), 2);
}

#[test]
fn live_active_tasks_keep_the_core_ordering() {
    let registry = HarnessRegistry::default();
    let now = super::protocol::now_millis();

    for harness_id in ["harness-b", "harness-a"] {
        let mut snapshot = running_snapshot(harness_id);
        snapshot.updated_at = now;
        snapshot.tasks[0].started_at = now;
        snapshot.tasks[0].updated_at = now;
        registry.upsert(snapshot).unwrap();
    }

    let ids: Vec<String> = registry
        .list_live_active_tasks()
        .into_iter()
        .map(|task| task.harness_id)
        .collect();

    assert_eq!(ids, vec!["harness-a", "harness-b"]);
}

#[test]
fn a_harness_that_reports_again_becomes_live_once_more() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_millis(0));
    store_aged(&registry, stale_snapshot("harness-a"));
    assert!(registry.is_stale("harness-a"));
    assert!(registry.list_live_active_tasks().is_empty());

    // A stale harness that comes back is simply a harness reporting again; the
    // live view picks it up with no special handling.
    let mut revived = running_snapshot("harness-a");
    let now = super::protocol::now_millis();
    revived.updated_at = now;
    revived.tasks[0].started_at = now;
    revived.tasks[0].updated_at = now;
    registry.upsert(revived).unwrap();

    assert_eq!(registry.list_live_active_tasks().len(), 1);
}

#[test]
fn local_json_text_parses_into_a_validated_snapshot() {
    let text = r#"
    {
      "harness_id": "demo",
      "harness_name": "Demo Harness",
      "tasks": [
        {
          "task_id": "task-123",
          "title": "Run tests",
          "status": "running",
          "started_at": 1234567890,
          "updated_at": 1234567890
        }
      ]
    }
    "#;

    let snapshot = parse_local_snapshot(text).unwrap();
    assert_eq!(snapshot.harness_id, "demo");
    assert_eq!(snapshot.tasks.len(), 1);
    assert!(snapshot.tasks[0].is_active());
}

#[test]
fn local_json_text_is_validated_not_just_parsed() {
    let text = r#"
    { "harness_id": "", "harness_name": "Demo Harness", "tasks": [] }
    "#;

    let error = parse_local_snapshot(text).unwrap_err();
    assert!(error.contains("harness_id"), "message was: {error}");
}

#[test]
fn malformed_local_json_is_an_error_not_a_panic() {
    let error = parse_local_snapshot("{ not json").unwrap_err();
    assert!(error.contains("malformed"), "message was: {error}");
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    let adapter = LocalJsonAdapter::new("test", "\\\\nonexistent-share\\nope\\missing.json");

    let error = adapter.poll().unwrap_err();
    assert_eq!(adapter.name(), "test");
    assert!(error.contains("test"), "message was: {error}");
}

#[test]
fn snapshots_serialise_with_the_protocol_spelling() {
    let json = serde_json::to_string(&running_snapshot("harness-a")).unwrap();

    assert!(json.contains("\"status\":\"running\""), "json was: {json}");
    assert!(json.contains("\"harness_id\":\"harness-a\""), "json was: {json}");
}

#[test]
fn a_snapshot_can_be_read_from_json_and_written_back() {
    let text = r#"
    {
      "harness_id": "demo",
      "harness_name": "Demo Harness",
      "source": { "type": "push" },
      "updated_at": 1234567890,
      "tasks": [
        {
          "task_id": "task-123",
          "title": "Run tests",
          "status": "waiting",
          "started_at": 1234567890,
          "updated_at": 1234567891,
          "message": "waiting on the database"
        }
      ]
    }
    "#;

    let snapshot: HarnessSnapshot = serde_json::from_str(text).unwrap();
    let round_tripped: HarnessSnapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();

    assert_eq!(snapshot, round_tripped);
    assert_eq!(round_tripped.tasks[0].message.as_deref(), Some("waiting on the database"));
}
