//! Tests for the protocol core: validation, the registry, staleness and the
//! adapter layer.
//!
//! These run without a running app and without the push endpoint. The adapter
//! tests do bind a loopback socket, because the HTTP adapter's timeout and body
//! limit are the point of it: a test that never opens a socket would not prove
//! either. They bind port 0, so a busy machine cannot make them fail.

use std::time::Duration;

use super::adapter::{
    parse_local_snapshot, HarnessAdapter, LocalHttpAdapter, LocalJsonAdapter, MAX_ADAPTER_BYTES,
};
use super::manager::{
    AdapterConfig, AdapterKind, AdapterManager, RunState, MAX_ADAPTERS,
};
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
    // timestamps say: `received_at` is the floor, and it is our own clock
    // reading. The producer's clock is never consulted, so a wildly wrong one
    // can neither kill a fresh snapshot nor keep a dead one alive.
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

#[test]
fn a_producer_clock_behind_ours_is_not_stale_on_arrival() {
    // Case 1: a snapshot stamped half an hour in the past must still be accepted
    // as live the moment it arrives. Staleness never compares the producer's
    // clock with ours.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));
    let now = super::protocol::now_millis();

    let mut snapshot = running_snapshot("harness-behind");
    snapshot.updated_at = now - 30 * 60 * 1000;
    snapshot.tasks[0].started_at = now - 30 * 60 * 1000;
    snapshot.tasks[0].updated_at = now - 30 * 60 * 1000;
    let stored = registry.upsert(snapshot).unwrap();

    assert!(!stored.is_stale(stored.received_at, Duration::from_secs(300)));
    assert!(!registry.is_stale("harness-behind"));
    assert_eq!(registry.list_live_active_tasks().len(), 1);

    // Case 2: the same document, polled again and again, is not new evidence.
    // This is the bug the change-aware upsert fixes: without it every poll moved
    // `received_at` and the harness could never retire.
    let mut unchanged = running_snapshot("harness-behind");
    unchanged.updated_at = now - 30 * 60 * 1000;
    unchanged.tasks[0].started_at = now - 30 * 60 * 1000;
    unchanged.tasks[0].updated_at = now - 30 * 60 * 1000;
    let again = registry.upsert(unchanged).unwrap();
    assert_eq!(
        again.received_at, stored.received_at,
        "re-reading identical content must not refresh liveness"
    );

    // Past the timeout it retires, and only the live view notices.
    let later = stored.received_at + 6 * 60 * 1000;
    assert!(stored.is_stale(later, Duration::from_secs(300)));
    assert_eq!(registry.len(), 1, "the snapshot is kept, never deleted");
    assert_eq!(registry.list_active_tasks().len(), 1, "still active, just not live");
}

#[test]
fn a_producer_clock_ahead_of_ours_is_not_stale_on_arrival() {
    // Case 3: a producer an hour in the future is not instantly dead either, and
    // it retires on our arrival clock once we stop seeing new content.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));
    let now = super::protocol::now_millis();

    let mut snapshot = running_snapshot("harness-ahead");
    snapshot.updated_at = now + 60 * 60 * 1000;
    snapshot.tasks[0].started_at = now;
    snapshot.tasks[0].updated_at = now + 60 * 60 * 1000;
    let stored = registry.upsert(snapshot).unwrap();

    assert!(!stored.is_stale(stored.received_at, Duration::from_secs(300)));
    assert!(stored.is_stale(stored.received_at + 6 * 60 * 1000, Duration::from_secs(300)));
}

#[test]
fn new_content_refreshes_liveness() {
    // Case 4: a producer making real progress stays live, however its clock is set.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));
    let now = super::protocol::now_millis();

    let mut first = running_snapshot("harness-a");
    first.updated_at = now - 30 * 60 * 1000;
    first.tasks[0].started_at = now - 30 * 60 * 1000;
    first.tasks[0].updated_at = now - 30 * 60 * 1000;
    let mut stored = registry.upsert(first).unwrap();

    // Age the arrival explicitly. Two upserts in the same millisecond would
    // otherwise share a timestamp, which is about clock resolution rather than
    // about what this test is checking.
    stored.received_at = now - 30 * 60 * 1000;
    registry.store(stored);
    let aged = registry.get("harness-a").unwrap();
    assert!(aged.is_stale(now, Duration::from_secs(300)), "it had gone quiet");

    // The producer reports the same task, still running, with a newer timestamp.
    let mut second = running_snapshot("harness-a");
    second.updated_at = now - 30 * 60 * 1000 + 1_000;
    second.tasks[0].started_at = now - 30 * 60 * 1000;
    second.tasks[0].updated_at = now - 30 * 60 * 1000 + 1_000;
    let refreshed = registry.upsert(second).unwrap();

    assert!(
        refreshed.received_at > aged.received_at,
        "real progress is a heartbeat"
    );
    assert!(!refreshed.is_stale(refreshed.received_at, Duration::from_secs(300)));
}

#[test]
fn a_task_status_change_counts_as_new_evidence() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));

    let first = registry.upsert(running_snapshot("harness-a")).unwrap();

    let mut changed = running_snapshot("harness-a");
    changed.tasks[0].status = HarnessStatus::Waiting;
    let second = registry.upsert(changed).unwrap();

    assert!(second.received_at >= first.received_at);
    assert!(second.differs_from(&first));
}

#[test]
fn identical_content_is_not_new_evidence() {
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));

    let first = registry.upsert(running_snapshot("harness-a")).unwrap();
    let second = registry.upsert(running_snapshot("harness-a")).unwrap();

    assert!(!second.differs_from(&first));
    assert_eq!(second.received_at, first.received_at);
}

#[test]
fn the_source_a_snapshot_arrived_through_is_not_evidence() {
    // Which adapter delivered a snapshot says nothing about whether the harness
    // is alive, so a re-labelled source must not renew it.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));

    let first = registry.upsert(running_snapshot("harness-a")).unwrap();

    let mut relabelled = running_snapshot("harness-a");
    relabelled.source = HarnessSource {
        source_type: "adapter".to_string(),
        name: Some("a-different-adapter".to_string()),
    };
    let second = registry.upsert(relabelled).unwrap();

    assert_eq!(second.received_at, first.received_at);
}

#[test]
fn a_push_can_always_act_as_a_heartbeat() {
    // Case 5: an explicit POST is the producer choosing to speak, so it is new
    // evidence when it says something new - and identical pushes still benefit
    // from the arrival floor rather than ageing the row out mid-conversation.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));

    let first = registry.upsert(running_snapshot("harness-push")).unwrap();
    let repeated = registry.upsert(running_snapshot("harness-push")).unwrap();
    assert_eq!(repeated.received_at, first.received_at, "same content, same heartbeat");

    let mut advanced = running_snapshot("harness-push");
    advanced.updated_at = first.snapshot.updated_at + 1_000;
    advanced.tasks[0].updated_at = first.snapshot.tasks[0].updated_at + 1_000;
    let third = registry.upsert(advanced).unwrap();
    assert!(third.received_at >= first.received_at);
    assert!(!registry.is_stale("harness-push"));
}

#[test]
fn stale_only_hides_a_snapshot_and_never_deletes_it() {
    // Case 6: the live view drops it, the registry keeps it, and it comes back
    // the moment new content arrives.
    let registry = HarnessRegistry::with_stale_timeout(Duration::from_secs(300));
    store_aged(&registry, running_snapshot("harness-a"));

    assert!(registry.is_stale("harness-a"));
    assert!(registry.list_live_active_tasks().is_empty());
    assert_eq!(registry.list_active_tasks().len(), 1);
    assert_eq!(registry.len(), 1, "the snapshot itself is never removed");
    assert_eq!(registry.get("harness-a").unwrap().snapshot.tasks.len(), 1);

    // New content revives it with no special handling.
    let now = super::protocol::now_millis();
    let mut revived = running_snapshot("harness-a");
    revived.updated_at = now;
    revived.tasks[0].started_at = now;
    revived.tasks[0].updated_at = now;
    registry.upsert(revived).unwrap();

    assert_eq!(registry.list_live_active_tasks().len(), 1);
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
/// `received_at` is our own clock reading and is the only input to staleness,
/// so a test that wants real staleness has to age the arrival - exactly what a
/// long-running app would have done. This is the "inject now" the stale checks
/// are supposed to use, instead of sleeping.
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

/// Write a file into a per-test temporary directory, so tests never share state
/// and never leave anything in the user's app data directory.
fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("sticky-harness-adapter-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// A file that does not exist, in a directory that does.
fn missing_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("sticky-harness-adapter-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }
    path
}

/// A snapshot document an external producer would write.
fn snapshot_json(harness_id: &str, task_id: &str, status: &str) -> String {
    format!(
        r#"{{
          "harness_id": "{harness_id}",
          "harness_name": "Harness {harness_id}",
          "updated_at": 1700000000000,
          "tasks": [
            {{
              "task_id": "{task_id}",
              "title": "Run tests",
              "status": "{status}",
              "started_at": 1700000000000,
              "updated_at": 1700000000000
            }}
          ]
        }}"#
    )
}

#[test]
fn the_json_adapter_reads_a_snapshot_from_a_file() {
    let path = temp_file("read.json", &snapshot_json("demo", "task-1", "running"));
    let adapter = LocalJsonAdapter::new("local-json", &path);

    let snapshot = adapter.poll().unwrap();

    assert_eq!(adapter.name(), "local-json");
    assert_eq!(adapter.path(), path);
    assert_eq!(snapshot.harness_id, "demo");
    assert_eq!(snapshot.tasks.len(), 1);
    assert!(snapshot.tasks[0].is_active());
}

#[test]
fn the_json_adapter_picks_up_a_changed_file_without_any_restart() {
    // The whole point of polling a file: an updated document is what the next
    // poll sees, with no filesystem notification and no cached copy.
    let path = temp_file("changed.json", &snapshot_json("demo", "task-1", "running"));
    let adapter = LocalJsonAdapter::new("local-json", &path);
    assert_eq!(adapter.poll().unwrap().tasks[0].task_id, "task-1");

    std::fs::write(&path, snapshot_json("demo", "task-2", "waiting")).unwrap();
    let snapshot = adapter.poll().unwrap();

    assert_eq!(snapshot.tasks[0].task_id, "task-2");
    assert_eq!(snapshot.tasks[0].status, HarnessStatus::Waiting);
}

#[test]
fn the_json_adapter_reports_a_missing_file_without_panicking() {
    let adapter = LocalJsonAdapter::new("local-json", missing_file("gone.json"));

    let error = adapter.poll().unwrap_err();

    assert!(error.contains("local-json"), "message was: {error}");
    assert!(error.contains("gone.json"), "message was: {error}");
}

#[test]
fn the_json_adapter_rejects_a_file_that_is_too_large() {
    // One byte past the limit, and the read is refused before it is buffered.
    let path = missing_file("huge.json");
    std::fs::write(&path, vec![b' '; MAX_ADAPTER_BYTES + 1]).unwrap();

    let error = LocalJsonAdapter::new("local-json", &path).poll().unwrap_err();

    assert!(error.contains("limit"), "message was: {error}");
}

#[test]
fn the_json_adapter_rejects_invalid_json_as_an_error() {
    let path = temp_file("broken.json", "{ not json");
    let error = LocalJsonAdapter::new("local-json", &path).poll().unwrap_err();

    assert!(error.contains("malformed"), "message was: {error}");
}

#[test]
fn the_json_adapter_stamps_its_own_name_as_the_source() {
    // Two adapters may report harnesses with the same id; the source is what
    // makes the stored snapshot say where it came from.
    let path = temp_file("source.json", &snapshot_json("demo", "task-1", "running"));
    let snapshot = LocalJsonAdapter::new("my-harness", &path).poll().unwrap();

    assert_eq!(snapshot.source.source_type, "adapter");
    assert_eq!(snapshot.source.name.as_deref(), Some("my-harness"));
}

/// A one-shot loopback HTTP server for the adapter tests.
///
/// Binds port 0 so a busy machine cannot break the test, then answers exactly
/// one connection however the test tells it to: normally, slowly, or with a
/// body larger than the adapter will accept.
struct TestServer {
    port: u16,
    handle: Option<std::thread::JoinHandle<()>>,
}

enum Reply {
    /// A normal 200 with a JSON body.
    Ok(String),
    /// A 200 whose headers never end, so the adapter's read timeout must fire.
    Silent,
    /// More bytes than the adapter will accept.
    TooLarge,
}

impl TestServer {
    fn start(reply: Reply) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            // One connection is enough; the adapter makes exactly one request per
            // poll, and the test makes one poll.
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut scratch = [0_u8; 4096];
                let _ = stream.read(&mut scratch);

                match reply {
                    Reply::Ok(body) => {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Reply::Silent => {
                        // Headers, no terminator, then a long enough pause that a
                        // one-second read timeout has to be what ends it.
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n");
                        let _ = stream.flush();
                        std::thread::sleep(Duration::from_secs(3));
                    }
                    Reply::TooLarge => {
                        let body = "x".repeat(MAX_ADAPTER_BYTES + 64 * 1024);
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
            }
        });

        Self {
            port,
            handle: Some(handle),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // The silent server sleeps past the timeout; rejoining it would make the
        // suite wait for nothing, so the thread is left to finish on its own.
        if let Some(handle) = self.handle.take() {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

#[test]
fn the_http_adapter_reads_a_snapshot_over_loopback() {
    let body = snapshot_json("demo", "task-1", "running");
    let server = TestServer::start(Reply::Ok(body));
    let adapter = LocalHttpAdapter::new("local-http", server.port, "/api/harness/snapshot");

    let snapshot = adapter.poll().unwrap();

    assert_eq!(adapter.name(), "local-http");
    assert_eq!(adapter.address().ip().to_string(), "127.0.0.1");
    assert_eq!(adapter.path(), "/api/harness/snapshot");
    assert_eq!(snapshot.harness_id, "demo");
    assert!(snapshot.tasks[0].is_active());
}

#[test]
fn the_http_adapter_only_ever_connects_to_loopback() {
    let adapter = LocalHttpAdapter::new("local-http", 1, "/x");

    // The address is built from the port alone, so there is no way to point an
    // adapter at a remote host even if a configuration tried to.
    assert_eq!(adapter.address().to_string(), "127.0.0.1:1");
}

#[test]
fn the_http_adapter_gives_up_after_its_timeout() {
    let server = TestServer::start(Reply::Silent);
    let adapter = LocalHttpAdapter::new("local-http", server.port, "/slow")
        .with_timeout(Duration::from_millis(300));

    let started = std::time::Instant::now();
    let error = adapter.poll().unwrap_err();
    let elapsed = started.elapsed();

    assert!(error.contains("local-http"), "message was: {error}");
    assert!(
        elapsed < Duration::from_secs(3),
        "a timeout must end the poll, not the server's sleep: took {elapsed:?}"
    );
}

#[test]
fn the_http_adapter_refuses_an_oversized_response() {
    let server = TestServer::start(Reply::TooLarge);
    let adapter = LocalHttpAdapter::new("local-http", server.port, "/huge");

    let error = adapter.poll().unwrap_err();

    assert!(error.contains("limit"), "message was: {error}");
}

#[test]
fn the_http_adapter_reports_a_closed_port_without_panicking() {
    // Port 1 on loopback: nothing is listening, and nothing should panic.
    let adapter = LocalHttpAdapter::new("local-http", 1, "/nope")
        .with_timeout(Duration::from_millis(200));

    let error = adapter.poll().unwrap_err();
    assert!(error.contains("local-http"), "message was: {error}");
}

#[test]
fn the_http_adapter_stamps_its_own_name_as_the_source() {
    let body = snapshot_json("demo", "task-1", "waiting");
    let server = TestServer::start(Reply::Ok(body));
    let snapshot = LocalHttpAdapter::new("my-http", server.port, "/api/harness/snapshot")
        .poll()
        .unwrap();

    assert_eq!(snapshot.source.source_type, "adapter");
    assert_eq!(snapshot.source.name.as_deref(), Some("my-http"));
}

/// A minimal in-memory adapter, so the manager can be tested without files.
struct FakeAdapter {
    name: String,
    snapshot: HarnessSnapshot,
}

impl HarnessAdapter for FakeAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&self) -> Result<HarnessSnapshot, super::protocol::ProtocolError> {
        Ok(self.snapshot.clone())
    }
}

/// An adapter that always fails, for failure-isolation tests.
struct BrokenAdapter;

impl HarnessAdapter for BrokenAdapter {
    fn name(&self) -> &str {
        "broken"
    }

    fn poll(&self) -> Result<HarnessSnapshot, super::protocol::ProtocolError> {
        Err("this producer is deliberately unavailable".to_string())
    }
}

#[test]
fn a_configuration_file_round_trips_through_json() {
    let text = r#"
    {
      "adapters": [
        { "name": "mybot", "kind": "local-json", "path": "C:/tmp/mybot.json" },
        { "name": "sidecar", "kind": "local-http", "port": 18001, "http_path": "/status",
          "poll_interval_millis": 2000, "timeout_millis": 500 }
      ]
    }
    "#;

    let config = AdapterConfig::parse(text).unwrap();

    assert_eq!(config.adapters.len(), 2);
    assert!(config.adapters[0].enabled, "enabled defaults to true");
    assert_eq!(config.adapters[0].kind, AdapterKind::LocalJson);
    assert_eq!(config.adapters[1].kind, AdapterKind::LocalHttp);
    assert_eq!(config.adapters[1].interval(), Duration::from_millis(2000));
}

#[test]
fn an_empty_configuration_is_valid_and_runs_nothing() {
    let config = AdapterConfig::parse(r#"{ "adapters": [] }"#).unwrap();

    assert_eq!(config.adapters.len(), 0);
    assert_eq!(config.enabled().count(), 0);
}

#[test]
fn a_configuration_without_an_adapters_key_is_still_valid() {
    // A file someone is halfway through writing should not break startup.
    let config = AdapterConfig::parse("{}").unwrap();
    assert!(config.adapters.is_empty());
}

#[test]
fn a_disabled_adapter_is_not_started() {
    let text = r#"
    {
      "adapters": [
        { "name": "on", "kind": "local-json", "path": "C:/tmp/a.json" },
        { "name": "off", "kind": "local-json", "path": "C:/tmp/b.json", "enabled": false }
      ]
    }
    "#;

    let config = AdapterConfig::parse(text).unwrap();
    let enabled: Vec<&str> = config.enabled().map(|entry| entry.name.as_str()).collect();

    assert_eq!(enabled, vec!["on"], "a disabled adapter never runs");
}

#[test]
fn a_local_json_adapter_without_a_path_is_rejected() {
    let text = r#"{ "adapters": [ { "name": "broken", "kind": "local-json" } ] }"#;

    let error = AdapterConfig::parse(text).unwrap_err();
    assert!(error.contains("local-json needs a path"), "message was: {error}");
    assert!(error.contains("adapter 0"), "the report names which entry: {error}");
}

#[test]
fn a_local_http_adapter_without_a_port_is_rejected() {
    let text = r#"{ "adapters": [ { "name": "broken", "kind": "local-http" } ] }"#;

    let error = AdapterConfig::parse(text).unwrap_err();
    assert!(error.contains("local-http needs a port"), "message was: {error}");
}

#[test]
fn an_unknown_adapter_kind_is_rejected() {
    let text = r#"{ "adapters": [ { "name": "x", "kind": "telepathy", "port": 1 } ] }"#;

    let error = AdapterConfig::parse(text).unwrap_err();
    assert!(error.contains("telepathy") || error.contains("unknown variant"), "message was: {error}");
}

#[test]
fn an_unknown_key_is_rejected_rather_than_ignored() {
    // A typo would otherwise silently leave the field unset, which is worse
    // than a startup line saying exactly what is wrong.
    let text = r#"{ "adapters": [ { "name": "x", "kind": "local-json", "pth": "C:/tmp/a.json" } ] }"#;

    let error = AdapterConfig::parse(text).unwrap_err();
    assert!(error.contains("pth"), "message was: {error}");
}

#[test]
fn two_adapters_cannot_share_a_name() {
    let text = r#"
    {
      "adapters": [
        { "name": "same", "kind": "local-json", "path": "C:/tmp/a.json" },
        { "name": "same", "kind": "local-json", "path": "C:/tmp/b.json" }
      ]
    }
    "#;

    let error = AdapterConfig::parse(text).unwrap_err();
    assert!(error.contains("both named"), "message was: {error}");
}

#[test]
fn a_poll_interval_outside_the_allowed_range_is_rejected() {
    let too_fast = r#"{ "adapters": [ { "name": "x", "kind": "local-json", "path": "C:/tmp/a.json", "poll_interval_millis": 1 } ] }"#;
    let too_slow = r#"{ "adapters": [ { "name": "x", "kind": "local-json", "path": "C:/tmp/a.json", "poll_interval_millis": 99999999 } ] }"#;

    assert!(AdapterConfig::parse(too_fast).unwrap_err().contains("poll_interval_millis"));
    assert!(AdapterConfig::parse(too_slow).unwrap_err().contains("poll_interval_millis"));
}

#[test]
fn too_many_adapters_are_rejected() {
    let entries: Vec<String> = (0..MAX_ADAPTERS + 1)
        .map(|index| format!(r#"{{ "name": "a{index}", "kind": "local-json", "path": "C:/tmp/{index}.json" }}"#))
        .collect();
    let text = format!(r#"{{ "adapters": [{}] }}"#, entries.join(", "));

    let error = AdapterConfig::parse(&text).unwrap_err();
    assert!(error.contains("the limit is"), "message was: {error}");
}

#[test]
fn a_missing_configuration_file_means_no_adapters() {
    let path = missing_file("no-config.json");

    let config = super::manager::load_config(&path);

    assert!(config.adapters.is_empty(), "a missing file is not an error");
}

#[test]
fn a_malformed_configuration_file_treated_as_no_adapters() {
    let path = temp_file("bad-config.json", "{ not json");

    let config = super::manager::load_config(&path);

    assert!(config.adapters.is_empty());
}

#[test]
fn a_valid_configuration_file_loads_from_disk() {
    let path = temp_file(
        "good-config.json",
        r#"{ "adapters": [ { "name": "file-one", "kind": "local-json", "path": "C:/tmp/a.json" } ] }"#,
    );

    let config = super::manager::load_config(&path);

    assert_eq!(config.adapters.len(), 1);
    assert_eq!(config.adapters[0].name, "file-one");
}

#[test]
fn a_manager_with_no_adapters_is_idle() {
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let manager = AdapterManager::start(registry.clone(), &AdapterConfig::default());

    assert_eq!(manager.state(), RunState::Idle);
    assert_eq!(manager.state().adapters(), 0);
    manager.stop();
}

#[test]
fn a_disabled_adapter_leaves_the_registry_empty() {
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let config = AdapterConfig::parse(
        r#"{ "adapters": [ { "name": "off", "kind": "local-json", "path": "C:/tmp/a.json", "enabled": false } ] }"#,
    )
    .unwrap();

    let manager = AdapterManager::start(registry.clone(), &config);
    std::thread::sleep(Duration::from_millis(150));

    assert_eq!(manager.state(), RunState::Idle);
    assert!(registry.is_empty(), "a disabled adapter must never report");
    manager.stop();
}

#[test]
fn one_adapter_fills_the_registry() {
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let adapter = FakeAdapter {
        name: "fake".to_string(),
        snapshot: running_snapshot("harness-a"),
    };
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // One turn of the loop body, without sleeping for an interval.
    super::manager::poll_once_for_test(adapter, registry.clone(), stop);

    assert_eq!(registry.len(), 1);
    let active = registry.list_active_tasks();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].harness_id, "harness-a");
}

#[test]
fn a_failing_adapter_leaves_a_good_snapshot_in_place() {
    // The rule that matters: a producer going away must not erase what the note
    // is showing. The staleness timeout is what retires it, not the failure.
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    super::manager::poll_once_for_test(
        FakeAdapter {
            name: "fake".to_string(),
            snapshot: running_snapshot("harness-a"),
        },
        registry.clone(),
        stop.clone(),
    );
    assert_eq!(registry.len(), 1);

    super::manager::poll_once_for_test(BrokenAdapter, registry.clone(), stop);

    assert_eq!(registry.len(), 1, "a failed poll never removes a harness");
    assert_eq!(registry.list_active_tasks().len(), 1);
    assert!(!registry.is_stale("harness-a"), "and it is still fresh");
}

#[test]
fn a_failing_adapter_does_not_touch_another_adapters_snapshot() {
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    super::manager::poll_once_for_test(
        FakeAdapter {
            name: "good".to_string(),
            snapshot: running_snapshot("harness-good"),
        },
        registry.clone(),
        stop.clone(),
    );

    // A different adapter fails, repeatedly.
    for _ in 0..3 {
        super::manager::poll_once_for_test(BrokenAdapter, registry.clone(), stop.clone());
    }

    assert_eq!(registry.len(), 1);
    assert_eq!(registry.list_active_tasks()[0].harness_id, "harness-good");
}

#[test]
fn an_adapter_that_recovers_is_stored_again() {
    // Failure is a state, not a verdict: the next successful poll is stored
    // exactly like the first one was.
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    super::manager::poll_once_for_test(BrokenAdapter, registry.clone(), stop.clone());
    assert!(registry.is_empty());

    super::manager::poll_once_for_test(
        FakeAdapter {
            name: "recovered".to_string(),
            snapshot: running_snapshot("harness-a"),
        },
        registry.clone(),
        stop,
    );

    assert_eq!(registry.len(), 1);
}

#[test]
fn a_snapshot_the_protocol_rejects_is_not_stored() {
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut broken = running_snapshot("harness-a");
    broken.harness_name = String::new();

    super::manager::poll_once_for_test(
        FakeAdapter {
            name: "bad-producer".to_string(),
            snapshot: broken,
        },
        registry.clone(),
        stop.clone(),
    );

    assert!(registry.is_empty(), "an invalid snapshot never reaches the registry");
    assert!(!stop.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn stopping_the_manager_ends_every_adapter_thread() {
    let registry = std::sync::Arc::new(HarnessRegistry::default());
    let config = AdapterConfig::parse(
        r#"{ "adapters": [ { "name": "slow", "kind": "local-json", "path": "C:/tmp/nope.json", "poll_interval_millis": 60000 } ] }"#,
    )
    .unwrap();

    let manager = AdapterManager::start(registry, &config);
    assert_eq!(manager.state(), RunState::Running(1));

    let started = std::time::Instant::now();
    manager.stop();

    // Stopping only flips a flag, so it cannot block; the adapters notice it
    // within one 50 ms slice rather than one 60 s interval.
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "stop must not wait on a producer: took {:?}",
        started.elapsed()
    );
}

#[test]
fn the_run_state_reports_what_is_running() {
    assert_eq!(RunState::Idle.adapters(), 0);
    assert_eq!(RunState::Running(3).adapters(), 3);
}
