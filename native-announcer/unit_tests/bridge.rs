use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn profile_preserves_character_selection_and_prompt_fallbacks() {
    let value = serde_json::json!({
        "summaryPrompt": "Global prompt.",
        "characters": {
            "first": { "selected": false, "summaryPrompt": "First prompt." },
            "second": { "selected": true, "summaryPrompt": "Second prompt." },
            "invalid": { "selected": false, "summaryPrompt": "" }
        }
    });
    assert_eq!(profile_from_value(&value, |_| 0), AnnouncementProfile { prompt: "Second prompt.".into(), character_id: Some("second".into()) });
    let value = serde_json::json!({ "summaryPrompt": "Global prompt.", "characters": { "invalid": { "selected": true, "summaryPrompt": "\u{0000}" } } });
    assert_eq!(profile_from_value(&value, |_| 0), AnnouncementProfile { prompt: "Global prompt.".into(), character_id: Some("invalid".into()) });
}

#[test]
fn profile_handles_javascript_style_character_values() {
    let value = serde_json::json!({ "summaryPrompt": "Global.", "characters": [null, { "selected": true, "summaryPrompt": "Array prompt." }] });
    assert_eq!(profile_from_value(&value, |_| 0), AnnouncementProfile { prompt: "Array prompt.".into(), character_id: Some("1".into()) });
    let value = serde_json::json!({ "summaryPrompt": "Global.", "characters": "😀" });
    let profile = profile_from_value(&value, |_| 1);
    assert_eq!(profile.character_id.as_deref(), Some("1"));
    assert_eq!(profile.prompt, "Global.");
}

#[test]
fn profile_read_does_not_rewrite_settings() {
    let data = test_directory();
    let path = data.join("settings.json");
    let bytes = br#"{"summaryPrompt":"Saved prompt.","characters":null}"#;
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(read_profile(&data).prompt, "Saved prompt.");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn profile_wire_name_matches_the_hook_contract() {
    let profile = AnnouncementProfile { prompt: "Prompt.".into(), character_id: Some("herald".into()) };
    assert_eq!(serde_json::to_string(&profile).unwrap(), r#"{"prompt":"Prompt.","characterID":"herald"}"#);
}

#[test]
fn title_reads_only_the_latest_transcript_tail() {
    let data = test_directory();
    let path = data.join("session.jsonl");
    let old = br#"{"type":"custom-title","customTitle":"Old title"}
"#;
    let mut bytes = old.to_vec();
    bytes.extend(std::iter::repeat_n(b'x', TRANSCRIPT_TAIL as usize));
    bytes.extend_from_slice(br#"{"type":"summary","summary":"Tail summary"}
{"type":"custom-title","customTitle":"Newest title"}
"#);
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(session_title(Some(&path), "Hook title"), "Newest title");
    let mut outside = old.to_vec();
    outside.extend(std::iter::repeat_n(b'x', TRANSCRIPT_TAIL as usize));
    std::fs::write(&path, outside).unwrap();
    assert_eq!(session_title(Some(&path), "Hook title"), "Hook title");
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn deliver_publishes_a_private_complete_json_file() {
    let data = test_directory();
    let command: state::Command = serde_json::from_value(serde_json::json!({
        "type":"presence", "clientID":"client", "sessionIDs":["session"], "sequence":4, "at":10
    })).unwrap();
    deliver(&data, &command).unwrap();
    let files: Vec<_> = std::fs::read_dir(data.join("inbox")).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension().and_then(|extension| extension.to_str()), Some("json"));
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&files[0]).unwrap()).unwrap()["type"], "presence");
    assert!(!files.iter().any(|path| path.extension().and_then(|extension| extension.to_str()) == Some("tmp")));
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn notify_removes_the_transcript_path_before_delivery() {
    let data = test_directory();
    let transcript = data.join("transcript.jsonl");
    std::fs::write(&transcript, br#"{"type":"custom-title","customTitle":"Transcript title"}
"#).unwrap();
    let mut value = serde_json::json!({ "type":"notify", "id":"1", "sessionID":"claude:1", "completed":10, "text":"Done.", "title":"Hook title", "transcriptPath":transcript });
    let command = prepare_command(&mut value).unwrap();
    deliver(&data, &command).unwrap();
    let path = std::fs::read_dir(data.join("inbox")).unwrap().next().unwrap().unwrap().path();
    let delivered = serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(delivered["title"], "Transcript title");
    assert!(delivered.get("transcriptPath").is_none());
    std::fs::remove_dir_all(data).unwrap();
}

fn test_directory() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join("opencode").join(format!("herald-bridge-{}-{}-{unique}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&path).unwrap();
    path
}
