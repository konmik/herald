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
    #[serde(skip_serializing_if = "Option::is_none")]
    character_id: Option<&'a str>,
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
    record_with_values(data, notification, character, None, video, shown_at)
}

pub fn record_with_identity(
    data: &Path,
    notification: &Notification,
    character: &str,
    video: &Path,
    shown_at: DateTime<Utc>,
) -> Result<(), String> {
    record_with_values(data, notification, character, Some(character), video, shown_at)
}

fn record_with_values(
    data: &Path,
    notification: &Notification,
    character: &str,
    character_id: Option<&str>,
    video: &Path,
    shown_at: DateTime<Utc>,
) -> Result<(), String> {
    let entry = ShownMessage {
        shown_at: shown_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        id: &notification.id,
        session_id: &notification.session_id,
        completed: notification.completed,
        text: &notification.text,
        title: &notification.title,
        character,
        character_id,
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
#[path = "../unit_tests/history.rs"]
mod tests;
