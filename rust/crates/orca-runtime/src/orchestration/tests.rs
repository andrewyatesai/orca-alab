use super::*;

fn msg(id: &str, to: &str, subject: &str) -> NewMessage {
    NewMessage {
        id: id.to_string(),
        from_handle: "coordinator".to_string(),
        to_handle: to.to_string(),
        subject: subject.to_string(),
        body: String::new(),
        message_type: "status".to_string(),
        priority: "normal".to_string(),
        thread_id: None,
        payload: None,
    }
}

fn status_of(db: &OrchestrationDb, id: &str) -> String {
    db.get_task(id).unwrap().unwrap().status
}

fn new_task(db: &OrchestrationDb, id: &str, spec: &str, deps: &[&str]) {
    db.create_task(id, spec, None, deps, None, None, None).unwrap();
}

#[test]
fn creates_schema_on_open() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    assert!(db.get_unread_messages("nobody", None).unwrap().is_empty());
}

#[test]
fn inserts_reads_full_row_then_marks_read() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    let stored = db.send_message(&msg("m1", "worker-a", "do the thing")).unwrap();
    // Full row is returned: read/sequence/created_at populated.
    assert_eq!(stored.read, 0);
    assert!(stored.sequence > 0);
    assert!(!stored.created_at.is_empty());
    db.send_message(&msg("m2", "worker-b", "other thing")).unwrap();

    let inbox = db.get_unread_messages("worker-a", None).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].id, "m1");
    assert_eq!(inbox[0].subject, "do the thing");

    assert_eq!(db.get_message_by_id("m1").unwrap().unwrap().subject, "do the thing");
    assert!(db.get_message_by_id("nope").unwrap().is_none());

    db.mark_as_read(&["m1"]).unwrap();
    assert!(db.get_unread_messages("worker-a", None).unwrap().is_empty());
    assert_eq!(db.get_unread_messages("worker-b", None).unwrap().len(), 1);
}

#[test]
fn unread_type_filter_and_thread_and_all_for_handle() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    db.send_message(&msg("m1", "w", "status one")).unwrap();
    let mut done = msg("m2", "w", "done");
    done.message_type = "worker_done".to_string();
    db.send_message(&done).unwrap();

    let filtered = db.get_unread_messages("w", Some(&["worker_done".to_string()])).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].message_type, "worker_done");

    // Thread replies addressed to a handle, oldest first, after a cursor.
    let mut outbound = msg("q1", "coord", "question");
    outbound.from_handle = "worker".to_string();
    let outbound = db.send_message(&outbound).unwrap();
    let mut reply = msg("r1", "worker", "reply");
    reply.from_handle = "coord".to_string();
    reply.thread_id = Some(outbound.id.clone());
    db.send_message(&reply).unwrap();
    let replies = db.get_thread_messages_for(&outbound.id, "worker", Some(outbound.sequence)).unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].id, "r1");

    // Newest-first, capped.
    assert_eq!(db.get_all_messages_for_handle("w", 100, None).unwrap()[0].id, "m2");
}

#[test]
fn message_type_check_constraint_rejects_invalid_type() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    let mut bad = msg("m1", "worker-a", "x");
    bad.message_type = "not-a-real-type".to_string();
    assert!(db.send_message(&bad).is_err());
}

#[test]
fn delivered_marker_is_distinct_from_read_replay_guard() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    db.send_message(&msg("m1", "worker-a", "hello")).unwrap();
    assert_eq!(db.get_undelivered_unread_messages("worker-a", None).unwrap().len(), 1);
    db.mark_as_delivered(&["m1"]).unwrap();
    assert!(db.get_undelivered_unread_messages("worker-a", None).unwrap().is_empty());
    assert_eq!(db.get_unread_messages("worker-a", None).unwrap().len(), 1); // still unread
}

#[test]
fn create_task_deps_drive_initial_status_and_display() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    db.create_task("t1", "build the parser", None, &[], Some("term-1"), Some("Parser"), Some("Build parser"))
        .unwrap();
    new_task(&db, "t2", "write tests", &["t1"]);

    let all = db.list_tasks(None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].status, "ready"); // no deps
    assert_eq!(all[0].task_title.as_deref(), Some("Parser"));
    assert_eq!(all[0].display_name.as_deref(), Some("Build parser"));
    assert_eq!(all[0].created_by_terminal_handle.as_deref(), Some("term-1"));
    assert_eq!(all[1].status, "pending"); // has a dep
    assert_eq!(all[1].deps, "[\"t1\"]");
}

#[test]
fn completing_a_task_promotes_ready_dependents_and_stamps_result() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "t1", "a", &[]);
    new_task(&db, "t2", "b", &["t1"]);
    new_task(&db, "t3", "c", &["t1", "t2"]);

    db.update_task_status("t1", "completed", Some("done"), Some("2026-01-01T00:00:00.000Z")).unwrap();
    assert_eq!(status_of(&db, "t2"), "ready");
    assert_eq!(status_of(&db, "t3"), "pending");
    let t1 = db.get_task("t1").unwrap().unwrap();
    assert_eq!(t1.result.as_deref(), Some("done"));
    assert!(t1.completed_at.is_some());

    // A later update without a result preserves it (COALESCE); keep t1 completed.
    db.update_task_status("t1", "completed", None, Some("2026-01-02T00:00:00.000Z")).unwrap();
    assert_eq!(db.get_task("t1").unwrap().unwrap().result.as_deref(), Some("done"));

    db.update_task_status("t2", "completed", None, Some("2026-01-01T00:00:00.000Z")).unwrap();
    assert_eq!(status_of(&db, "t3"), "ready");
}

#[test]
fn list_tasks_with_dispatch_surfaces_only_active_assignee() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "ready", "ready task", &[]);
    new_task(&db, "active", "active task", &[]);
    db.create_dispatch_context("active", "term-worker", "ctx1").unwrap();

    let rows = db.list_tasks_with_dispatch(None).unwrap();
    let ready_row = rows.iter().find(|r| r.task.id == "ready").unwrap();
    let active_row = rows.iter().find(|r| r.task.id == "active").unwrap();
    assert_eq!(ready_row.assignee_handle, None);
    assert_eq!(ready_row.dispatch_id, None);
    assert_eq!(active_row.assignee_handle.as_deref(), Some("term-worker"));
    assert_eq!(active_row.dispatch_id.as_deref(), Some("ctx1"));

    // Completing the task drops it from the "active" join.
    db.update_task_status("active", "completed", None, Some("2026-01-01T00:00:00.000Z")).unwrap();
    let rows = db.list_tasks_with_dispatch(None).unwrap();
    let active_row = rows.iter().find(|r| r.task.id == "active").unwrap();
    assert_eq!(active_row.assignee_handle, None);
    assert_eq!(active_row.dispatch_id, None);
}

#[test]
fn decision_gate_blocks_task_and_resolution_unblocks_it() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "t1", "spec", &[]);
    db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();

    let gate = db.create_gate("g1", "t1", "Proceed?", &["yes", "no"]).unwrap();
    assert_eq!(gate.status, "pending");
    assert_eq!(gate.options, "[\"yes\",\"no\"]");
    assert_eq!(status_of(&db, "t1"), "blocked");
    assert_eq!(db.dispatch_context_by_id("ctx1").unwrap().unwrap().status, "completed");

    let resolved = db.resolve_gate("g1", "yes").unwrap().unwrap();
    assert_eq!(resolved.status, "resolved");
    assert_eq!(resolved.resolution.as_deref(), Some("yes"));
    assert_eq!(status_of(&db, "t1"), "ready");
    assert!(db.list_gates(Some("t1"), Some("pending")).unwrap().is_empty());
    assert_eq!(db.list_gates(None, None).unwrap().len(), 1);

    // Missing gate resolves to None.
    assert!(db.resolve_gate("nope", "x").unwrap().is_none());

    db.create_gate("g2", "t1", "Again?", &["ok"]).unwrap();
    assert_eq!(status_of(&db, "t1"), "blocked");
    let timed = db.timeout_gate("g2").unwrap().unwrap();
    assert_eq!(timed.status, "timeout");
    assert_eq!(status_of(&db, "t1"), "blocked");
}

#[test]
fn dispatch_requires_ready_task_and_one_active_per_assignee() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "dep", "dep", &[]);
    new_task(&db, "t1", "spec1", &["dep"]); // pending
    new_task(&db, "t2", "spec2", &[]); // ready

    assert!(db.create_dispatch_context("t1", "worker-1", "ctx0").is_err());
    let err = db.create_dispatch_context("nope", "worker-1", "ctxX").unwrap_err();
    assert!(err.to_string().contains("Task not found: nope"), "{err}");

    db.update_task_status("dep", "completed", None, Some("2026-01-01T00:00:00.000Z")).unwrap();
    assert_eq!(status_of(&db, "t1"), "ready");

    let ctx = db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();
    assert_eq!(ctx.status, "dispatched");
    assert_eq!(status_of(&db, "t1"), "dispatched");
    assert_eq!(db.get_active_dispatch_for_terminal("worker-1").unwrap().unwrap().id, "ctx1");

    // The exact "for task" error text is load-bearing for CLI UX.
    let err = db.create_dispatch_context("t2", "worker-1", "ctx2").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Terminal worker-1 already has an active dispatch (ctx1 for task t1)"
    );

    assert_eq!(db.complete_dispatch("ctx1").unwrap(), 1);
    assert!(db.get_active_dispatch_for_terminal("worker-1").unwrap().is_none());
    let ctx3 = db.create_dispatch_context("t2", "worker-1", "ctx3").unwrap();
    assert_eq!(ctx3.task_id, "t2");
    assert_eq!(db.get_latest_dispatch_for_terminal("worker-1").unwrap().unwrap().id, "ctx3");
}

#[test]
fn fail_dispatch_carries_failures_and_trips_circuit_breaker_at_three() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "t1", "spec", &[]);

    for (ctx_id, expected_count) in [("ctx1", 1_i64), ("ctx2", 2)] {
        let ctx = db.create_dispatch_context("t1", "worker-1", ctx_id).unwrap();
        assert_eq!(ctx.failure_count, expected_count - 1); // carried forward
        let failed = db.fail_dispatch(ctx_id, "boom").unwrap().unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.failure_count, expected_count);
        assert_eq!(failed.last_failure.as_deref(), Some("boom"));
        assert_eq!(status_of(&db, "t1"), "ready");
    }

    db.create_dispatch_context("t1", "worker-1", "ctx3").unwrap();
    let broken = db.fail_dispatch("ctx3", "boom").unwrap().unwrap();
    assert_eq!(broken.status, "circuit_broken");
    assert_eq!(broken.failure_count, 3);
    assert_eq!(status_of(&db, "t1"), "failed");

    assert!(db.fail_dispatch("nope", "boom").unwrap().is_none());
}

#[test]
fn heartbeat_only_touches_dispatched_and_stale_detector_respects_threshold() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "t1", "spec", &[]);
    db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();

    // Fresh dispatch, no heartbeat, is stale against a future threshold.
    let future = "2999-01-01 00:00:00";
    assert_eq!(db.get_stale_dispatches(future).unwrap().len(), 1);

    // A heartbeat newer than the threshold clears staleness; injected value is
    // stored verbatim (lexicographic ISO/space compare orders in time).
    assert_eq!(db.record_heartbeat("ctx1", "2999-06-01 00:00:00").unwrap(), 1);
    assert_eq!(
        db.dispatch_context_by_id("ctx1").unwrap().unwrap().last_heartbeat_at.as_deref(),
        Some("2999-06-01 00:00:00")
    );
    assert!(db.get_stale_dispatches(future).unwrap().is_empty());

    // A heartbeat older than the threshold → stale again.
    db.record_heartbeat("ctx1", "2000-01-01 00:00:00").unwrap();
    assert_eq!(db.get_stale_dispatches(future).unwrap()[0].id, "ctx1");

    // Nothing is stale against a past threshold (dispatched_at grace).
    assert!(db.get_stale_dispatches("1999-01-01 00:00:00").unwrap().is_empty());

    // Zombie-heartbeat guard: once completed, a heartbeat updates 0 rows.
    db.complete_dispatch("ctx1").unwrap();
    assert_eq!(db.record_heartbeat("ctx1", "2999-06-02 00:00:00").unwrap(), 0);
}

#[test]
fn set_dispatch_timestamps_backdates_for_the_grace_window() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    new_task(&db, "t1", "spec", &[]);
    db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();

    // With dispatched_at ≈ now (2026) and no heartbeat, a mid-2026 threshold does
    // not make it stale (dispatched_at not < threshold — the grace shields it).
    assert!(db.get_stale_dispatches("2026-01-01 00:00:00").unwrap().is_empty());

    // Backdate dispatched_at before the threshold → now eligible (no heartbeat).
    db.set_dispatch_timestamps("ctx1", Some("2020-01-01 00:00:00"), None).unwrap();
    let stale = db.get_stale_dispatches("2026-01-01 00:00:00").unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].id, "ctx1");
}

#[test]
fn coordinator_run_lifecycle_and_idle_terminals() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    let run = db.create_coordinator_run("run1", "ship it", "coordinator-a", None).unwrap();
    assert_eq!(run.status, "running");
    assert_eq!(run.poll_interval_ms, 2000);
    assert_eq!(db.active_coordinator_run().unwrap().unwrap().id, "run1");

    let done = db.update_coordinator_run("run1", "completed", Some("2026-01-01T00:00:00.000Z")).unwrap().unwrap();
    assert_eq!(done.status, "completed");
    assert!(done.completed_at.is_some());
    assert!(db.active_coordinator_run().unwrap().is_none());

    let custom = db.create_coordinator_run("run2", "spec", "coordinator-b", Some(500)).unwrap();
    assert_eq!(custom.poll_interval_ms, 500);

    // Idle terminals: handles seen in messages, minus those with active dispatches.
    db.send_message(&msg("m1", "worker-a", "hi")).unwrap();
    db.send_message(&msg("m2", "worker-b", "hi")).unwrap();
    new_task(&db, "t1", "spec", &[]);
    db.create_dispatch_context("t1", "worker-a", "ctx1").unwrap();
    let idle = db.get_idle_terminals(&["coordinator"]).unwrap();
    assert!(idle.contains(&"worker-b".to_string()));
    assert!(!idle.contains(&"worker-a".to_string())); // busy
    assert!(!idle.contains(&"coordinator".to_string())); // excluded
}

#[test]
fn reset_helpers_clear_the_right_tables() {
    let db = OrchestrationDb::open_in_memory().unwrap();
    db.send_message(&msg("m1", "a", "hi")).unwrap();
    new_task(&db, "t1", "spec", &[]);
    db.create_dispatch_context("t1", "worker-1", "ctx1").unwrap();

    db.reset_tasks().unwrap();
    assert_eq!(db.get_inbox(10).unwrap().len(), 1);
    assert!(db.list_tasks(None).unwrap().is_empty());
    assert!(db.dispatch_context_by_id("ctx1").unwrap().is_none());

    db.reset_messages().unwrap();
    assert!(db.get_inbox(10).unwrap().is_empty());
}
