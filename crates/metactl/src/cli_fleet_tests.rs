use super::*;

#[test]
fn failure_summary_keeps_actionable_nested_errors_without_json_noise() {
    let message = json!({
        "message": "Apply refused",
        "details": ["AGENTS.md: unmanaged", null, 42],
        "source_audit": {"findings": [{"id": "missing", "message": "source unavailable", "path": "library"}]}
    });
    assert_eq!(
        fleet_sync_failure_message_summary(&message.to_string()),
        "Apply refused; AGENTS.md: unmanaged; missing: source unavailable (library)"
    );
    assert_eq!(
        fleet_sync_failure_message_summary("  permission\ndenied\t "),
        "permission denied"
    );
    assert_eq!(
        fleet_sync_failure_message_summary("\n\t"),
        "no failure detail returned"
    );
}

#[test]
fn failure_summary_bounds_unicode_without_panicking_or_hiding_omitted_projects() {
    let long = "界".repeat(700);
    let summary = fleet_sync_failure_message_summary(&long);
    assert_eq!(summary.chars().count(), 600);
    assert!(summary.ends_with("..."));
    let failures: Vec<_> = (0..25)
        .map(|i| json!({"id": format!("p{i}"), "status": "failed", "message": "denied"}))
        .collect();
    let details = fleet_sync_failure_details(&failures);
    assert_eq!(details.len(), 21);
    assert!(details[0].starts_with("p0 "));
    assert!(details[19].starts_with("p19 "));
    assert_eq!(
        details[20],
        "5 more failed project(s); rerun with --json for the full fleet payload"
    );
}

#[test]
fn fleet_log_redacts_paths_and_child_failure_payloads() {
    let entry = json!({"id":"app", "status":"failed", "result":"sync_failed", "profile":"team",
        "path":"sensitive/path", "message":"sensitive failure", "sync":{"private":"payload"}});
    assert_eq!(
        redact_fleet_log_project(&entry),
        json!({"id":"app", "status":"failed", "result":"sync_failed", "profile":"team"})
    );
}
