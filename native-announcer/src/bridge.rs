use crate::{platform, private, settings, state};
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const TRANSCRIPT_TAIL: u64 = 262_144;
static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnnouncementProfile {
    pub(crate) prompt: String,
    #[serde(rename = "characterID", skip_serializing_if = "Option::is_none")]
    pub(crate) character_id: Option<String>,
}

#[derive(Clone)]
struct CharacterProfile {
    id: String,
    selected: bool,
    prompt: Option<String>,
}

pub(crate) fn run<R: Read, W: Write>(mut input: R, mut output: W, assets: &Path) -> Result<(), String> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let command_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or("Bridge command type is required")?;
    match command_type {
        "boot" => boot(assets),
        "read-announcement-profile" => {
            let profile = read_profile(&platform::data_directory());
            serde_json::to_writer(&mut output, &profile).map_err(|error| error.to_string())?;
            output.flush().map_err(|error| error.to_string())
        }
        _ => {
            let data = platform::data_directory();
            let command = prepare_command(&mut value)?;
            deliver(&data, &command)
        }
    }
}

pub(crate) fn read_profile(data: &Path) -> AnnouncementProfile {
    let bytes = match std::fs::read(data.join("settings.json")) {
        Ok(bytes) => bytes,
        Err(_) => return default_profile(),
    };
    let text = String::from_utf8_lossy(&bytes);
    let value = match serde_json::from_str::<Value>(&text) {
        Ok(value) => value,
        Err(_) => return default_profile(),
    };
    profile_from_value(&value, |length| rand::thread_rng().gen_range(0..length))
}

pub(crate) fn profile_from_value(value: &Value, choose: impl FnOnce(usize) -> usize) -> AnnouncementProfile {
    let characters = character_profiles(value);
    let selected: Vec<_> = characters.iter().filter(|character| character.selected).collect();
    let candidates = if selected.is_empty() { characters.iter().collect() } else { selected };
    let character = if candidates.is_empty() { None } else { candidates.get(choose(candidates.len())).copied() };
    let global_prompt = value
        .get("summaryPrompt")
        .and_then(Value::as_str)
        .filter(|prompt| settings::validate_summary_prompt(prompt).is_ok());
    let prompt = character
        .and_then(|character| character.prompt.as_deref())
        .filter(|prompt| settings::validate_summary_prompt(prompt).is_ok())
        .or(global_prompt)
        .unwrap_or(settings::DEFAULT_SUMMARY_PROMPT)
        .to_owned();
    AnnouncementProfile { prompt, character_id: character.map(|character| character.id.clone()) }
}

fn default_profile() -> AnnouncementProfile {
    AnnouncementProfile { prompt: settings::DEFAULT_SUMMARY_PROMPT.into(), character_id: None }
}

fn character_profiles(value: &Value) -> Vec<CharacterProfile> {
    let Some(characters) = value.get("characters") else { return Vec::new(); };
    match characters {
        Value::Object(entries) => entries
            .iter()
            .map(|(id, character)| character_profile(id, character))
            .collect(),
        Value::Array(entries) => entries
            .iter()
            .enumerate()
            .map(|(index, character)| character_profile(&index.to_string(), character))
            .collect(),
        Value::String(value) => (0..value.encode_utf16().count())
            .map(|index| CharacterProfile { id: index.to_string(), selected: false, prompt: None })
            .collect(),
        _ => Vec::new(),
    }
}

fn character_profile(id: &str, value: &Value) -> CharacterProfile {
    CharacterProfile {
        id: id.into(),
        selected: value.get("selected").and_then(Value::as_bool) == Some(true),
        prompt: value.get("summaryPrompt").and_then(Value::as_str).map(str::to_owned),
    }
}

fn prepare_command(value: &mut Value) -> Result<state::Command, String> {
    if value.get("type").and_then(Value::as_str) == Some("notify") {
        let transcript = value
            .get("transcriptPath")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        let fallback = value.get("title").and_then(Value::as_str).unwrap_or_default();
        let title = session_title(transcript.as_deref(), fallback);
        let object = value.as_object_mut().ok_or("Bridge command must be an object")?;
        object.insert("title".into(), Value::String(title));
        object.remove("transcriptPath");
    }
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

pub(crate) fn session_title(path: Option<&Path>, fallback: &str) -> String {
    let Some(path) = path else { return title_or_default(fallback.to_owned()); };
    let result = (|| -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let length = size.min(TRANSCRIPT_TAIL);
        file.seek(SeekFrom::Start(size - length))?;
        let mut bytes = vec![0; length as usize];
        file.read_exact(&mut bytes)?;
        let mut title = fallback.to_owned();
        for line in String::from_utf8_lossy(&bytes).split('\n') {
            let Ok(entry) = serde_json::from_str::<Value>(line) else { continue; };
            if entry.get("type").and_then(Value::as_str) == Some("custom-title") {
                if let Some(value) = entry.get("customTitle").and_then(Value::as_str) { title = value.into(); }
            }
            if title.is_empty() && entry.get("type").and_then(Value::as_str) == Some("summary") {
                if let Some(value) = entry.get("summary").and_then(Value::as_str) {
                    title = value.into();
                }
            }
        }
        Ok(title_or_default(title))
    })();
    result.unwrap_or_else(|_| title_or_default(fallback.to_owned()))
}

fn deliver(data: &Path, command: &state::Command) -> Result<(), String> {
    private::directory(data).map_err(|error| error.to_string())?;
    let inbox = data.join("inbox");
    private::directory(&inbox).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(command).map_err(|error| error.to_string())?;
    for _ in 0..16 {
        let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!("{}-{}-{}", state::timestamp(), std::process::id(), sequence);
        let temporary = inbox.join(format!("{name}.tmp"));
        let final_path = inbox.join(format!("{name}.json"));
        let mut file = match private::create_new(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        };
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &final_path)
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temporary); }
        return result.map_err(|error| error.to_string());
    }
    Err("Could not allocate a unique bridge inbox file".into())
}

fn boot(assets: &Path) -> Result<(), String> {
    if std::env::var("HERALD_EXTERNAL_COMPANION").as_deref() == Ok("1") {
        match std::env::var_os("HERALD_DATA") {
            Some(data) if !data.is_empty() => return Ok(()),
            _ => return Err("An external companion requires HERALD_DATA".into()),
        }
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(executable);
    command
        .arg("--assets")
        .arg(assets)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200);
    }
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 { return Err(std::io::Error::last_os_error()); }
            Ok(())
        });
    }
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

fn title_or_default(title: String) -> String {
    if title.is_empty() { "Untitled session".into() } else { title }
}

#[cfg(test)]
mod tests {
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
}
