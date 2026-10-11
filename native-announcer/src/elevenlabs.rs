use base64::Engine;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum SpeechModel {
    #[default]
    #[serde(rename = "eleven_flash_v2_5")]
    Flash,
    #[serde(rename = "eleven_v4")]
    V4,
    #[serde(rename = "eleven_v4_turbo")]
    V4Turbo,
}

impl SpeechModel {
    pub const ALL: [Self; 3] = [Self::Flash, Self::V4, Self::V4Turbo];

    pub fn id(self) -> &'static str {
        match self {
            Self::Flash => "eleven_flash_v2_5",
            Self::V4 => "eleven_v4",
            Self::V4Turbo => "eleven_v4_turbo",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Flash => "ElevenLabs Flash v2.5",
            Self::V4 => "ElevenLabs V4",
            Self::V4Turbo => "ElevenLabs V4 Turbo",
        }
    }
}

pub const DEFAULT_VOICE_ID: &str = "JBFqnCBsd6RMkjVDRZzb";

pub fn validate_api_key(key: &str) -> Result<(), String> {
    if key.trim().is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
        return Err("ElevenLabs API key is invalid.".into());
    }
    Ok(())
}

const DEFAULT_BASE_URL: &str = "https://api.elevenlabs.io";
const API_BASE: &str = "ELEVENLABS_API_BASE_URL";
const REQUEST_BODY_LIMIT: usize = 8 * 1024 * 1024;
const AUDIO_LIMIT: usize = 8 * 1024 * 1024;
const VOICE_USAGE_LIMIT: usize = 64 * 1024;
static SPEECH_REQUESTS: AtomicUsize = AtomicUsize::new(0);

struct SpeechSlot;

impl Drop for SpeechSlot {
    fn drop(&mut self) { SPEECH_REQUESTS.fetch_sub(1, Ordering::Relaxed); }
}

#[derive(Clone)]
pub struct Client {
    base_url: String,
    api_key: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct VoiceUsage {
    pub voice_slots_used: u64,
    pub voice_limit: u64,
    pub voice_add_edit_counter: u64,
    pub max_voice_add_edits: Option<u64>,
}

impl VoiceUsage {
    pub fn remaining_voice_slots(&self) -> u64 {
        self.voice_limit.saturating_sub(self.voice_slots_used)
    }

    pub fn voice_add_edit_allowance(&self) -> Option<(u64, u64)> {
        self.max_voice_add_edits.map(|limit| (limit, limit.saturating_sub(self.voice_add_edit_counter)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpeechError {
    Cancelled,
    Message(String),
}

impl std::fmt::Display for SpeechError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Speech was cancelled."),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl Client {
    pub fn from_settings(settings: &crate::settings::Settings) -> Result<Option<Self>, String> {
        let key = match settings.elevenlabs_api_key.clone() {
            Some(key) => key,
            None => return Ok(None),
        };
        let base_url = std::env::var(API_BASE).unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        Self::new(base_url, key).map(Some)
    }

    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self, String> {
        let base_url = normalize_base_url(&base_url.into())?;
        let api_key = api_key.into();
        validate_api_key(&api_key)?;
        Ok(Self { base_url, api_key })
    }

    pub fn voice_usage(&self) -> Result<VoiceUsage, String> {
        let response = self.get("/v1/user/subscription")?;
        serde_json::from_slice(&response).map_err(|_| "ElevenLabs returned an invalid voice usage response.".to_string())
    }

    pub fn synthesize_speech(&self, voice_id: &str, text: &str, model: SpeechModel, cancelled: &Arc<AtomicBool>) -> Result<Vec<i16>, SpeechError> {
        if cancelled.load(Ordering::Relaxed) { return Err(SpeechError::Cancelled); }
        validate_remote_id(voice_id, "saved voice").map_err(SpeechError::Message)?;
        if text.trim().is_empty() { return Ok(Vec::new()); }
        if text.len() > 4096 || text.contains('\0') { return Err(SpeechError::Message("Announcement text is invalid.".into())); }
        if SPEECH_REQUESTS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| (count < 4).then_some(count + 1)).is_err() {
            return Err(SpeechError::Message("Previous ElevenLabs speech requests are still finishing.".into()));
        }
        let slot = SpeechSlot;
        let stop = cancelled.clone();
        let client = self.clone();
        let voice_id = voice_id.to_owned();
        let text = text.to_owned();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _slot = slot;
            if stop.load(Ordering::Relaxed) { return; }
            let result = if model == SpeechModel::V4Turbo {
                client.synthesize_turbo_speech(&voice_id, &text, &stop)
            } else {
                let (path, body) = build_http_speech_request(model, &voice_id, &text);
                client.request(&path, &body.to_string()).and_then(|bytes| decode_pcm(&bytes))
            };
            let _ = sender.send(result);
        });
        loop {
            if cancelled.load(Ordering::Relaxed) { return Err(SpeechError::Cancelled); }
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(result) => return result.map_err(SpeechError::Message),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(SpeechError::Message("ElevenLabs speech request stopped unexpectedly.".into())),
            }
        }
    }

    fn request(&self, path: &str, body: &str) -> Result<Vec<u8>, String> {
        if body.len() > REQUEST_BODY_LIMIT { return Err("ElevenLabs request is too large.".into()); }
        let url = format!("{}{}", self.base_url, path);
        let agent = ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(30)).build();
        let result = agent
            .post(&url)
            .set("xi-api-key", &self.api_key)
            .set("Content-Type", "application/json")
            .send_string(body);
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(status, _)) => return Err(format!("ElevenLabs request failed with HTTP {status}.")),
            Err(ureq::Error::Transport(_)) => return Err("Could not reach ElevenLabs.".into()),
        };
        if !(200..300).contains(&response.status()) { return Err(format!("ElevenLabs request failed with HTTP {}.", response.status())); }
        validate_pcm_type(response.header("Content-Type").unwrap_or_default())?;
        read_limited(response.into_reader(), AUDIO_LIMIT)
    }

    fn get(&self, path: &str) -> Result<Vec<u8>, String> {
        let url = format!("{}{}", self.base_url, path);
        let agent = ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(10)).build();
        let result = agent.get(&url).set("xi-api-key", &self.api_key).call();
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(status, _)) if matches!(status, 401 | 403) => return Err(format!("The API key is invalid or lacks User read permission. ElevenLabs returned HTTP {status}.")),
            Err(ureq::Error::Status(status, _)) => return Err(format!("ElevenLabs request failed with HTTP {status}.")),
            Err(ureq::Error::Transport(_)) => return Err("Could not reach ElevenLabs.".into()),
        };
        if !(200..300).contains(&response.status()) { return Err(format!("ElevenLabs request failed with HTTP {}.", response.status())); }
        read_limited(response.into_reader(), VOICE_USAGE_LIMIT)
    }

    fn synthesize_turbo_speech(&self, voice_id: &str, text: &str, cancelled: &AtomicBool) -> Result<Vec<i16>, String> {
        use tungstenite::{client::IntoClientRequest, Message};
        let base = self.base_url.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
        let mut request = format!("{base}/v1/text-to-dialogue/stream-input?model_id=eleven_v4_turbo&output_format=pcm_16000").into_client_request().map_err(|_| "Invalid ElevenLabs connection.".to_string())?;
        request.headers_mut().insert("xi-api-key", self.api_key.parse().map_err(|_| "ElevenLabs API key is invalid.".to_string())?);
        let started = Instant::now();
        let config = tungstenite::protocol::WebSocketConfig { max_message_size: Some(AUDIO_LIMIT * 2), max_frame_size: Some(AUDIO_LIMIT * 2), ..Default::default() };
        use std::net::ToSocketAddrs;
        let host = request.uri().host().ok_or("Invalid ElevenLabs host.")?;
        let port = request.uri().port_u16().unwrap_or(if request.uri().scheme_str() == Some("wss") { 443 } else { 80 });
        let addresses = (host, port).to_socket_addrs().map_err(|_| "Could not resolve ElevenLabs host.".to_string())?;
        let stream = addresses.filter_map(|address| std::net::TcpStream::connect_timeout(&address, Duration::from_secs(5)).ok()).next().ok_or("Could not connect to ElevenLabs Turbo.")?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|_| "Could not limit ElevenLabs connection.".to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|_| "Could not limit ElevenLabs connection.".to_string())?;
        let (mut socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None).map_err(|_| "Could not connect to ElevenLabs Turbo.".to_string())?;
        let stream = match socket.get_mut() {
            tungstenite::stream::MaybeTlsStream::Plain(stream) => stream,
            tungstenite::stream::MaybeTlsStream::Rustls(stream) => &mut stream.sock,
            _ => return Err("Unsupported ElevenLabs connection.".into()),
        };
        stream.set_read_timeout(Some(Duration::from_secs(1))).map_err(|_| "Could not limit ElevenLabs connection.".to_string())?;
        for body in [
            serde_json::json!({ "voices": [voice_id] }),
            serde_json::json!({ "inputs": [{ "text": text, "voice_id": voice_id, "new_turn": false }] }),
            serde_json::json!({ "close_socket": true }),
        ] {
            socket.send(Message::Text(body.to_string())).map_err(|_| "Could not send ElevenLabs Turbo request.".to_string())?;
        }
        let mut audio = Vec::new();
        loop {
            if cancelled.load(Ordering::Relaxed) { return Err("Speech was cancelled.".into()); }
            if started.elapsed() > Duration::from_secs(30) { return Err("ElevenLabs Turbo speech timed out.".into()); }
            match socket.read() {
                Ok(Message::Text(value)) => {
                    let value: serde_json::Value = serde_json::from_str(&value).map_err(|_| "Invalid ElevenLabs Turbo response.".to_string())?;
                    if value.get("error").is_some_and(|value| !value.is_null()) { return Err("ElevenLabs Turbo rejected the speech request.".into()); }
                    if let Some(encoded) = value.get("audio").and_then(|value| value.as_str()) {
                        let chunk = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| "Invalid ElevenLabs Turbo audio.".to_string())?;
                        if audio.len() + chunk.len() > AUDIO_LIMIT { return Err("ElevenLabs Turbo audio is too large.".into()); }
                        audio.extend(chunk);
                    }
                    if value.get("is_final").and_then(|value| value.as_bool()) == Some(true) { return decode_pcm(&audio); }
                }
                Ok(Message::Close(_)) => return Err("ElevenLabs Turbo returned incomplete audio.".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => return Err("ElevenLabs Turbo connection failed.".into()),
            }
        }
    }
}

fn build_http_speech_request(model: SpeechModel, voice_id: &str, text: &str) -> (String, serde_json::Value) {
    if model == SpeechModel::V4 {
        ("/v1/text-to-dialogue?output_format=pcm_16000".into(), serde_json::json!({ "inputs": [{ "text": text, "voice_id": voice_id }], "model_id": model.id() }))
    } else {
        (format!("/v1/text-to-speech/{voice_id}?output_format=pcm_16000"), serde_json::json!({ "text": text, "model_id": model.id() }))
    }
}

fn decode_pcm(bytes: &[u8]) -> Result<Vec<i16>, String> {
    if bytes.is_empty() || bytes.len() % 2 != 0 { return Err("ElevenLabs returned invalid PCM audio.".into()); }
    if [b"RIFF".as_slice(), b"ID3".as_slice(), b"OggS".as_slice(), b"fLaC".as_slice()].iter().any(|header| bytes.starts_with(header)) {
        return Err("ElevenLabs returned encoded audio instead of PCM.".into());
    }
    Ok(bytes.chunks_exact(2).map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]])).collect())
}

fn validate_pcm_type(value: &str) -> Result<(), String> {
    let mut parts = value.split(';');
    if !matches!(parts.next().unwrap_or_default().trim().to_ascii_lowercase().as_str(), "audio/pcm" | "audio/x-pcm" | "application/octet-stream") {
        return Err("ElevenLabs returned an unsupported audio format; expected PCM at 16 kHz.".into());
    }
    for part in parts {
        if let Some((name, value)) = part.trim().split_once('=') {
            if name.trim().eq_ignore_ascii_case("rate") && value.trim() != "16000" {
                return Err("ElevenLabs returned an unsupported audio sample rate.".into());
            }
        }
    }
    Ok(())
}

fn validate_remote_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err(format!("ElevenLabs {label} ID is invalid."));
    }
    Ok(())
}

fn normalize_base_url(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    if value == DEFAULT_BASE_URL { return Ok(value.into()); }
    let Some((scheme, remainder)) = value.split_once("://") else { return Err("ElevenLabs API base URL is invalid.".into()); };
    if scheme == "https" && remainder == "api.elevenlabs.io" { return Ok(value.into()); }
    if scheme != "http" { return Err("ElevenLabs API base URL must use HTTPS.".into()); }
    let host = remainder.split('/').next().unwrap_or_default();
    if remainder.contains('/') || remainder.contains('?') || remainder.contains('#') || host.contains('@') || !loopback_host(host) {
        return Err("HTTP ElevenLabs API URLs are allowed only for a loopback test server.".into());
    }
    Ok(value.into())
}

fn loopback_host(host: &str) -> bool {
    let host = if let Some(host) = host.strip_prefix('[') {
        host.split_once(']').map_or("", |(host, _)| host)
    } else if host.matches(':').count() == 1 {
        host.split_once(':').map_or(host, |(host, _)| host)
    } else {
        host
    };
    host == "127.0.0.1" || host == "localhost" || host == "::1"
}

fn read_limited(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader.by_ref().take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|_| "Could not read ElevenLabs response.".to_string())?;
    if bytes.len() > limit { return Err("ElevenLabs response is too large.".into()); }
    Ok(bytes)
}

#[cfg(test)]
#[path = "../unit_tests/elevenlabs.rs"]
mod tests;
