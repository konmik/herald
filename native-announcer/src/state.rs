use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Notification {
    pub id: String,
    #[serde(rename = "sessionID", alias = "SessionID")]
    pub session_id: String,
    #[serde(default, rename = "presenceSessionID")]
    pub presence_session_id: String,
    pub completed: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub character: String,
    #[serde(default, rename = "characterID", skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    #[serde(default = "neutral")]
    pub emotion: String,
}

fn neutral() -> String {
    "neutral".into()
}

impl Notification {
    pub fn character(&self) -> &str {
        if self.character == "monty" {
            "monty"
        } else if self.character == "claude"
            || (self.character.is_empty() && self.session_id.starts_with("claude:"))
        {
            "claude"
        } else {
            "opencode"
        }
    }
}

pub fn is_night(hour: u32, start: u32, end: u32) -> bool {
    if start > end {
        hour >= start || hour < end
    } else {
        hour >= start && hour < end
    }
}

pub struct MeetingStatus {
    checked: Mutex<Option<(Option<bool>, Instant)>>,
}

impl MeetingStatus {
    pub fn new() -> Self {
        Self {
            checked: Mutex::new(None),
        }
    }

    pub fn update(&self, active: Option<bool>, now: Instant) {
        if let Ok(mut checked) = self.checked.lock() {
            *checked = Some((active, now));
        }
    }

    pub fn muted(&self, now: Instant) -> bool {
        self.checked.lock().map_or(true, |checked| {
            checked.is_none_or(|(active, at)| active != Some(false)
                || now.saturating_duration_since(at) >= Duration::from_secs(15))
        })
    }

    pub fn ready(&self) -> bool {
        self.checked.lock().is_ok_and(|checked| checked.is_some())
    }
}

pub fn display_duration(text: &str) -> Duration {
    Duration::from_secs_f64((3.0 + text.split_whitespace().count() as f64 * 0.4).max(5.0))
}

pub const TRANSITION_DURATION: Duration = Duration::from_millis(650);
pub const SIGNAL_SEED: u32 = 734971;
pub const VIDEO_FPS: u32 = 8;

pub fn interference_amount(elapsed: Duration, seed: u32) -> f32 {
    let progress = elapsed.as_secs_f32() / TRANSITION_DURATION.as_secs_f32();
    if !(0.0..1.0).contains(&progress) {
        return 0.0;
    }
    let seconds = elapsed.as_secs_f32();
    let attack = (seconds / 0.045).min(1.0);
    let release = ((TRANSITION_DURATION.as_secs_f32() - seconds) / 0.09).min(1.0);
    let random = noise_hash((elapsed.as_millis() / 23) as u32 ^ seed);
    let burst = match random & 7 {
        0 => 0.10,
        1 | 2 => 0.90,
        _ => 0.24 + ((random >> 8) & 255) as f32 / 255.0 * 0.43,
    };
    attack * release * burst
}

pub fn noise_hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846ca68b);
    value ^ (value >> 16)
}

pub fn visual_interference_amount(elapsed: Duration, seed: u32) -> f32 {
    let tick = (elapsed.as_millis() / 42) as u32;
    let random = noise_hash(tick ^ seed);
    if elapsed < TRANSITION_DURATION {
        match random & 7 {
            0..=3 => 0.0,
            4 | 5 => 0.32,
            _ => 0.62,
        }
    } else if random.is_multiple_of(71) {
        0.28
    } else {
        0.0
    }
}

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn log(data: &Path, message: impl std::fmt::Display) {
    use std::io::Write;
    let path = data.join("errors.log");
    if path.metadata().is_ok_and(|m| m.len() > 65536) {
        let _ = std::fs::rename(&path, data.join("errors.previous.log"));
    }
    if let Ok(mut file) = crate::private::file(&path, true) {
        let _ = writeln!(file, "{message}");
    }
}

pub fn timing(message: impl std::fmt::Display) {
    use std::io::Write;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(_lock) = LOCK.lock() else { return; };
    let data = crate::platform::data_directory();
    let path = data.join("timing.log");
    if path.metadata().is_ok_and(|m| m.len() > 65536) {
        let _ = std::fs::rename(&path, data.join("timing.previous.log"));
    }
    if let Ok(mut file) = crate::private::file(&path, true) {
        let _ = writeln!(file, "{} {message}", timestamp());
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Command {
    Notify(Notification),
    #[serde(rename = "get-ready")]
    GetReady(Notification),
    Discard {
        #[serde(rename = "sessionID")]
        session_id: String,
        at: u64,
    },
    Presence {
        #[serde(rename = "clientID")]
        client_id: String,
        #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, rename = "sessionIDs")]
        session_ids: Vec<String>,
        #[serde(default)]
        sequence: u64,
        at: u64,
    },
}

pub struct Inbox {
    pub data: PathBuf,
    pub queue: VecDeque<Notification>,
    pub preparations: VecDeque<Notification>,
    received: VecDeque<String>,
    discarded: VecDeque<(String, u64)>,
    presence: HashMap<String, (Vec<String>, u64, u64)>,
}

impl Inbox {
    pub fn new(data: PathBuf) -> Self {
        if let Err(error) = crate::private::directory(&data.join("inbox")) { log(&data, error); }
        let queue: VecDeque<Notification> = std::fs::read(data.join("queue.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            data,
            queue: queue
                .into_iter()
                .rev()
                .take(128)
                .collect::<VecDeque<_>>()
                .into_iter()
                .rev()
                .collect(),
            received: VecDeque::new(),
            preparations: VecDeque::new(),
            discarded: VecDeque::new(),
            presence: HashMap::new(),
        }
    }

    fn discard(&mut self, session_id: String, at: u64) {
        let previous = self
            .discarded
            .iter()
            .find(|(s, _)| s == &session_id)
            .map(|(_, at)| *at)
            .unwrap_or(0);
        self.discarded.retain(|(s, _)| s != &session_id);
        self.discarded
            .push_back((session_id.clone(), at.max(previous)));
        if self.discarded.len() > 2048 {
            self.discarded.pop_front();
        }
        self.queue
            .retain(|n| n.session_id != session_id || n.completed > at.max(previous));
    }

    pub fn is_open(&self, notification: &Notification, now: u64) -> bool {
        let owner = if notification.presence_session_id.is_empty() {
            &notification.session_id
        } else {
            &notification.presence_session_id
        };
        self.presence.values().any(|(sessions, at, _)| {
            now.saturating_sub(*at) < 6000
                && (sessions.contains(owner) || sessions.contains(&notification.session_id))
        })
    }

    pub fn preparation_valid(&self, n: &Notification, now: u64) -> bool {
        now.saturating_sub(n.completed) < 120_000 && self.is_open(n, now)
            && !self.is_discarded(n)
    }

    pub fn is_discarded(&self, n: &Notification) -> bool {
        self.discarded.iter().any(|(session, at)| session == &n.session_id && *at >= n.completed)
    }

    pub fn next(&mut self, now: u64) -> Option<Notification> {
        let index = self
            .queue
            .iter()
            .position(|notification| self.is_open(notification, now))?;
        self.queue.remove(index)
    }

    pub fn read(&mut self, current: Option<&Notification>) -> bool {
        self.presence
            .retain(|_, (_, at, _)| timestamp().saturating_sub(*at) < 6000);
        let mut paths: Vec<_> = std::fs::read_dir(self.data.join("inbox"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|s| s == "json"))
            .collect();
        paths.sort();
        let mut changed = false;
        for path in paths.into_iter().take(128) {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    log(&self.data, error);
                    continue;
                }
            };
            let bytes = bytes.strip_prefix(&[239, 187, 191]).unwrap_or(&bytes);
            let command: Command = match serde_json::from_slice(bytes) {
                Ok(command) => command,
                Err(error) => {
                    log(&self.data, error);
                    let _ = std::fs::rename(&path, path.with_extension("invalid"));
                    continue;
                }
            };
            match command {
                Command::GetReady(n) => {
                    if timestamp().saturating_sub(n.completed) < 120_000 && self.preparations.len() < 128 {
                        timing(format!("get_ready id={}", n.id));
                        self.preparations.push_back(n);
                    }
                }
                Command::Notify(mut n) => {
                    timing(format!("notification_received id={}", n.id));
                    if !self.received.contains(&n.id)
                        && !self
                            .discarded
                            .iter()
                            .any(|(s, at)| s == &n.session_id && *at >= n.completed)
                    {
                        if let Some((title, summary)) = n.text.split_once('|') {
                            n.title = title.trim().to_owned();
                            n.text = summary.trim().to_owned();
                        }
                        n.text = n.text.chars().take(4096).collect();
                        n.title = n.title.chars().take(256).collect();
                        self.queue.retain(|v| v.session_id != n.session_id);
                        if self.queue.len() >= 128 {
                            self.queue.pop_front();
                        }
                        self.queue.push_back(n.clone());
                    }
                    if !self.received.contains(&n.id) {
                        self.received.push_back(n.id);
                    }
                    if self.received.len() > 2048 {
                        self.received.pop_front();
                    }
                }
                Command::Discard { session_id, at } => self.discard(session_id, at),
                Command::Presence {
                    client_id,
                    session_id,
                    mut session_ids,
                    sequence,
                    at,
                } => {
                    if let Some(session_id) = session_id.filter(|id| !id.is_empty()) {
                        session_ids.push(session_id);
                    }
                    session_ids.truncate(2048);
                    if !self.presence.get(&client_id).is_some_and(
                        |(_, previous_at, previous_sequence)| {
                            (*previous_at, *previous_sequence) > (at, sequence)
                        },
                    )
                        && timestamp().saturating_sub(at) < 6000
                        && (self.presence.len() < 2048 || self.presence.contains_key(&client_id))
                    {
                        self.presence.insert(client_id, (session_ids, at, sequence));
                    }
                }
            }
            if let Err(error) = std::fs::remove_file(path) {
                log(&self.data, error);
            }
            changed = true;
        }
        if changed {
            self.save(current);
        }
        current.is_some_and(|n| !self.is_open(n, timestamp()))
    }

    pub fn save(&self, current: Option<&Notification>) {
        let values: Vec<_> = current.into_iter().chain(self.queue.iter()).collect();
        if let Ok(bytes) = serde_json::to_vec(&values) {
            let temp = self.data.join("queue.tmp");
            if let Err(error) = crate::private::write(&temp, &bytes)
                .and_then(|_| std::fs::rename(temp, self.data.join("queue.json")))
            {
                log(&self.data, error);
            }
        }
    }
}

#[cfg(test)]
#[path = "../unit_tests/state.rs"]
mod tests;
