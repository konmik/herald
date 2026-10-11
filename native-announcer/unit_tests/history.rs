use super::*;

#[test]
fn appends_messages_with_display_time_and_actual_selected_character() {
    let data = std::env::temp_dir().join("opencode").join(format!(
        "herald-history-{}-{}",
        std::process::id(),
        crate::state::timestamp()
    ));
    std::fs::create_dir_all(&data).unwrap();
    let notification: Notification = serde_json::from_value(serde_json::json!({
        "id":"result", "sessionID":"session", "completed":10,
        "text":"Tests passed.\nNothing deployed.", "title":"Code review", "character":"opencode"
    })).unwrap();
    let shown_at = DateTime::parse_from_rfc3339("2026-10-06T12:34:56.789Z")
        .unwrap()
        .with_timezone(&Utc);
    let video = Path::new("resources/videos/royal-herald-04.mp4");
    record(&data, &notification, video, shown_at).unwrap();
    record(&data, &notification, Path::new("resources/opencode/neutral.mp4"), shown_at).unwrap();
    let contents = std::fs::read_to_string(data.join("history.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = contents
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(contents.ends_with('\n'));
    assert_eq!(entries[0]["shownAt"], "2026-10-06T12:34:56.789Z");
    assert_eq!(entries[0]["completed"], 10);
    assert_eq!(entries[0]["sessionID"], "session");
    assert_eq!(entries[0]["text"], notification.text);
    assert_eq!(entries[0]["title"], "Code review");
    assert_eq!(entries[0]["character"], "royal-herald-04");
    assert_eq!(entries[0]["source"], "opencode");
    assert_eq!(entries[0]["video"], video.to_str().unwrap());
    assert_eq!(entries[1]["character"], "opencode");
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn custom_history_keeps_profile_identity_separate_from_integration_source() {
    let data = std::env::temp_dir().join("opencode").join(format!(
        "herald-history-custom-{}-{}",
        std::process::id(),
        crate::state::timestamp()
    ));
    std::fs::create_dir_all(&data).unwrap();
    let notification: Notification = serde_json::from_value(serde_json::json!({
        "id":"custom", "sessionID":"session", "completed":10, "text":"Done.", "character":"claude"
    })).unwrap();
    let video = Path::new("C:/Videos/royal-herald.mp4");
    record_with_identity(&data, &notification, "royal-herald", video, Utc::now()).unwrap();
    let entry: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data.join("history.jsonl")).unwrap()).unwrap();
    assert_eq!(entry["character"], "royal-herald");
    assert_eq!(entry["characterId"], "royal-herald");
    assert_eq!(entry["source"], "claude");
    assert_eq!(entry["video"], video.to_str().unwrap());
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn reports_write_failures() {
    let notification: Notification = serde_json::from_value(serde_json::json!({
        "id":"result", "sessionID":"session", "completed":10, "text":"Done."
    })).unwrap();
    assert!(record(
        Path::new("missing-announcer-history-test-directory"),
        &notification,
        Path::new("resources/videos/herald.mp4"),
        Utc::now(),
    ).is_err());
}
