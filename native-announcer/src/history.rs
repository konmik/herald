use crate::state::Notification;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShownMessage<'a> {
    shown_at: String,
    id: &'a str,
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    completed: u64,
    text: &'a str,
    title: &'a str,
    character: &'a str,
    source: &'a str,
    video: &'a Path,
}

pub fn record(
    data: &Path,
    notification: &Notification,
    video: &Path,
    shown_at: DateTime<Utc>,
) -> Result<(), String> {
    let character = video
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| *name != "neutral")
        .unwrap_or_else(|| notification.character());
    let entry = ShownMessage {
        shown_at: shown_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        id: &notification.id,
        session_id: &notification.session_id,
        completed: notification.completed,
        text: &notification.text,
        title: &notification.title,
        character,
        source: notification.character(),
        video,
    };
    let mut bytes = serde_json::to_vec(&entry).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut file = crate::private::file(&data.join("history.jsonl"), true).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_data().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_messages_with_display_time_and_actual_selected_character() {
        let data = std::env::temp_dir().join("opencode").join(format!(
            "civilized-history-{}-{}",
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
}
