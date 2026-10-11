use super::*;

#[test]
fn preparation_requires_presence_and_rejects_discarded_or_expired_requests() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let now = timestamp();
    write_command(&data, "0", serde_json::json!({"type":"presence","clientID":"client","sessionIDs":["session"],"at":now}));
    write_command(&data, "1", serde_json::json!({"type":"get-ready","id":"ready","sessionID":"session","completed":now}));
    inbox.read(None);
    let n = inbox.preparations.pop_front().unwrap();
    assert!(inbox.preparation_valid(&n, now));
    assert!(!inbox.preparation_valid(&n, now + 120_000));
    inbox.discard("session".into(), now);
    assert!(!inbox.preparation_valid(&n, now));
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn playback_requires_a_live_owner_and_stops_when_the_last_client_closes() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let now = timestamp();
    let notification: Notification = serde_json::from_value(serde_json::json!({
        "id":"child", "sessionID":"child", "presenceSessionID":"parent", "completed":now, "text":"Done."
    })).unwrap();
    assert!(!inbox.is_open(&notification, now));
    write_command(&data, "0", serde_json::json!({"type":"presence","clientID":"first","sessionIDs":["parent","other"],"at":now}));
    write_command(&data, "1", serde_json::json!({"type":"presence","clientID":"second","sessionIDs":["parent"],"at":now}));
    assert!(!inbox.read(Some(&notification)));
    assert!(inbox.is_open(&notification, now));
    assert!(!inbox.is_open(&notification, now + 6000));
    write_command(&data, "2", serde_json::json!({"type":"presence","clientID":"first","sessionIDs":[],"at":now}));
    assert!(!inbox.read(Some(&notification)));
    write_command(&data, "3", serde_json::json!({"type":"presence","clientID":"second","sessionIDs":[],"at":now}));
    assert!(inbox.read(Some(&notification)));
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn closed_sessions_do_not_block_playback_for_open_sessions() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let now = timestamp();
    for session in ["closed", "open"] {
        inbox.queue.push_back(serde_json::from_value(serde_json::json!({"id":session,"sessionID":session,"completed":now,"text":"Done."})).unwrap());
    }
    write_command(&data, "0", serde_json::json!({"type":"presence","clientID":"client","sessionIDs":["open"],"at":now}));
    inbox.read(None);
    assert_eq!(inbox.next(now).unwrap().session_id, "open");
    assert!(inbox.next(now).is_none());
    assert_eq!(inbox.queue.front().unwrap().session_id, "closed");
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn meeting_audio_requires_a_recent_successful_check() {
    let meeting = MeetingStatus::new();
    let now = std::time::Instant::now();
    assert!(!meeting.ready());
    assert!(meeting.muted(now));
    meeting.update(Some(false), now);
    assert!(meeting.ready());
    assert!(!meeting.muted(now));
    assert!(meeting.muted(now + Duration::from_secs(15)));
    meeting.update(None, now);
    assert!(meeting.ready());
    assert!(meeting.muted(now));
    meeting.update(Some(true), now);
    assert!(meeting.muted(now));
    meeting.update(Some(false), now);
    assert!(!meeting.muted(now));
}

#[test]
fn late_heartbeats_cannot_reopen_a_closed_client() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let now = timestamp();
    let notification: Notification = serde_json::from_value(serde_json::json!({"id":"notification","sessionID":"session","completed":now,"text":"Done."})).unwrap();
    write_command(&data, "0", serde_json::json!({"type":"presence","clientID":"client","sessionIDs":[],"at":now,"sequence":2}));
    write_command(&data, "1", serde_json::json!({"type":"presence","clientID":"client","sessionIDs":["session"],"at":now,"sequence":1}));
    inbox.read(None);
    assert!(!inbox.is_open(&notification, now));
    assert!(!data.join("inbox/1.json").exists());
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn duration_scales_and_never_under_five_seconds() {
    assert_eq!(display_duration("Done."), Duration::from_secs(5));
    assert_eq!(
        display_duration(&"word ".repeat(30)),
        Duration::from_secs(15)
    );
}

#[test]
fn interference_fades_in_and_out_and_video_is_eight_fps() {
    assert_eq!(interference_amount(Duration::ZERO, 1), 0.0);
    assert_eq!(interference_amount(TRANSITION_DURATION, 1), 0.0);
    let amounts: Vec<_> = (3..23)
        .map(|index| interference_amount(Duration::from_millis(index * 23), 1))
        .collect();
    assert!(amounts.iter().any(|value| *value > 0.85));
    assert!(amounts.iter().any(|value| *value < 0.15));
    assert!(
        amounts
            .windows(2)
            .filter(|pair| (pair[0] - pair[1]).abs() > 0.3)
            .count()
            >= 4
    );
    assert_eq!(VIDEO_FPS, 8);
    let bursts: Vec<_> = (0..15)
        .map(|index| visual_interference_amount(Duration::from_millis(index * 42), 1))
        .collect();
    assert!(bursts.contains(&0.0));
    assert!(bursts.contains(&0.62));
    assert!(bursts
        .windows(2)
        .any(|pair| (pair[0] - pair[1]).abs() > 0.5));
}

#[test]
fn night_boundaries() {
    assert!(is_night(22, 22, 8));
    assert!(is_night(7, 22, 8));
    assert!(!is_night(8, 22, 8));
    assert!(!is_night(21, 22, 8));
    assert!(!is_night(12, 22, 22));
}

#[test]
fn bridge_notification_deserializes() {
    let command: Command = serde_json::from_str(r#"{"type":"notify","id":"1","sessionID":"claude:1","completed":10,"text":"Done.","title":"My task","characterID":"herald"}"#).unwrap();
    let Command::Notify(n) = command else {
        panic!()
    };
    assert_eq!(n.title, "My task");
    assert_eq!(n.character(), "claude");
    assert_eq!(n.character_id.as_deref(), Some("herald"));
}

fn test_directory() -> PathBuf {
    let path = std::env::temp_dir().join("opencode").join(format!(
        "herald-state-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write_command(data: &Path, name: &str, value: serde_json::Value) {
    std::fs::write(
        data.join("inbox").join(format!("{name}.json")),
        value.to_string(),
    )
    .unwrap();
}

#[test]
fn notification_separator_sets_title_and_summary_for_both_hosts() {
    for session in ["claude:task", "opencode:task"] {
        for (text, title, summary) in [
            (" Tests passed | All checks are green. ", "Tests passed", "All checks are green."),
            ("结果 😀 | Fixed A | B.", "结果 😀", "Fixed A | B."),
            ("All checks are green.", "Session title", "All checks are green."),
        ] {
            let data = test_directory();
            let mut inbox = Inbox::new(data.clone());
            write_command(&data, "0", serde_json::json!({
                "type": "notify", "id": "result", "sessionID": session,
                "completed": 10, "text": text, "title": "Session title"
            }));
            inbox.read(None);
            let notification = inbox.queue.front().unwrap();
            assert_eq!(notification.title, title);
            assert_eq!(notification.text, summary);
            std::fs::remove_dir_all(data).unwrap();
        }
    }
}

#[test]
fn discard_preserves_current_playback_and_rejects_old_notifications() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let notification: Notification = serde_json::from_value(
        serde_json::json!({"id":"active","sessionID":"session","completed":10,"text":"Done."}),
    )
    .unwrap();
    inbox.queue.push_back(notification.clone());
    write_command(
        &data,
        "0-presence",
        serde_json::json!({"type":"presence","clientID":"client","sessionID":"session","at":timestamp()}),
    );
    write_command(
        &data,
        "1-discard",
        serde_json::json!({"type":"discard","sessionID":"session","at":20}),
    );
    write_command(
        &data,
        "2-old",
        serde_json::json!({"type":"notify","id":"old","sessionID":"session","completed":15,"text":"Old."}),
    );
    assert!(!inbox.read(Some(&notification)));
    assert!(inbox.queue.is_empty());
    write_command(
        &data,
        "3-new",
        serde_json::json!({"type":"notify","id":"new","sessionID":"session","completed":25,"text":"New."}),
    );
    inbox.read(None);
    assert_eq!(inbox.queue.front().unwrap().id, "new");
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn viewing_a_session_does_not_suppress_its_notification() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let now = timestamp();
    write_command(
        &data,
        "0",
        serde_json::json!({"type":"presence","clientID":"client","sessionID":"viewed","at":now}),
    );
    write_command(
        &data,
        "1",
        serde_json::json!({"type":"notify","id":"seen","sessionID":"viewed","completed":now+1,"text":"Done."}),
    );
    write_command(
        &data,
        "2",
        serde_json::json!({"type":"presence","clientID":"expired","sessionID":"stale","at":now-7000}),
    );
    write_command(
        &data,
        "3",
        serde_json::json!({"type":"notify","id":"unseen","sessionID":"stale","completed":now,"text":"Done."}),
    );
    inbox.read(None);
    assert_eq!(inbox.queue.len(), 2);
    assert_eq!(inbox.queue.front().unwrap().id, "seen");
    write_command(
        &data,
        "4",
        serde_json::json!({"type":"presence","clientID":"client","sessionID":"viewed","at":now+2}),
    );
    let active = inbox.queue.front().unwrap().clone();
    assert!(!inbox.read(Some(&active)));
    assert_eq!(inbox.queue.len(), 2);
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn blurred_client_can_clear_presence_with_null_session() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let now = timestamp();
    write_command(
        &data,
        "0",
        serde_json::json!({"type":"presence","clientID":"client","sessionID":"viewed","at":now}),
    );
    write_command(
        &data,
        "1",
        serde_json::json!({"type":"presence","clientID":"client","sessionID":null,"at":now+1}),
    );
    write_command(
        &data,
        "2",
        serde_json::json!({"type":"notify","id":"new","sessionID":"viewed","completed":now+2,"text":"Done."}),
    );
    inbox.read(None);
    assert_eq!(inbox.queue.len(), 1);
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn duplicates_are_ignored_and_queue_survives_restart() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    let value = serde_json::json!({"type":"notify","id":"one","sessionID":"session","completed":10,"text":"Done.","title":"Persistent title"});
    write_command(&data, "0", value.clone());
    write_command(&data, "1", value);
    inbox.read(None);
    assert_eq!(inbox.queue.len(), 1);
    let restored = Inbox::new(data.clone());
    assert_eq!(restored.queue.front().unwrap().title, "Persistent title");
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn malformed_commands_are_quarantined() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    std::fs::write(data.join("inbox/broken.json"), "not json").unwrap();
    inbox.read(None);
    assert!(data.join("inbox/broken.invalid").exists());
    assert!(inbox.queue.is_empty());
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn queue_and_message_sizes_are_bounded() {
    let data = test_directory();
    let mut inbox = Inbox::new(data.clone());
    for index in 0..140 {
        write_command(
            &data,
            &format!("{index:03}"),
            serde_json::json!({"type":"notify","id":format!("{index}"),"sessionID":format!("session-{index}"),"completed":10,"text":"x".repeat(5000),"title":"y".repeat(300)}),
        );
    }
    inbox.read(None);
    inbox.read(None);
    assert_eq!(inbox.queue.len(), 128);
    assert!(inbox
        .queue
        .iter()
        .all(|n| n.text.len() == 4096 && n.title.len() == 256));
    std::fs::remove_dir_all(data).unwrap();
}
