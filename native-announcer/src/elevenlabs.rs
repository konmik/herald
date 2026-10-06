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
const DESIGN_BODY_LIMIT: u64 = 8 * 1024 * 1024;
const AUDIO_LIMIT: usize = 8 * 1024 * 1024;
const PREVIEW_LIMIT: usize = 4 * 1024 * 1024;
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

#[derive(Clone, Debug)]
pub struct VoicePreview {
    pub generated_voice_id: String,
    pub duration_secs: f64,
    pub samples: Vec<i16>,
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

    pub fn design(&self, voice_description: &str, sample_text: &str) -> Result<Vec<VoicePreview>, String> {
        if !(20..=1000).contains(&voice_description.trim().chars().count()) || invalid_prose(voice_description) {
            return Err("Voice description must be between 20 and 1000 characters.".into());
        }
        if !(100..=1000).contains(&sample_text.trim().chars().count()) || invalid_prose(sample_text) {
            return Err("Sample text must be between 100 and 1000 characters.".into());
        }
        let body = serde_json::json!({
            "voice_description": voice_description,
            "text": sample_text,
            "model_id": "eleven_ttv_v3",
            "guidance_scale": 4
        });
        let response = self.request("/v1/text-to-voice/design?output_format=pcm_16000", &body.to_string())?;
        let response: DesignResponse = serde_json::from_slice(&response).map_err(|_| "ElevenLabs returned an invalid voice design response.".to_string())?;
        if response.previews.is_empty() || response.previews.len() > 12 {
            return Err("ElevenLabs returned no usable voice previews.".into());
        }
        response.previews.into_iter().map(VoicePreview::try_from).collect()
    }

    pub fn create_voice(&self, voice_name: &str, voice_description: &str, generated_voice_id: &str) -> Result<String, String> {
        if !(1..=160).contains(&voice_name.trim().len()) || voice_name.chars().any(char::is_control) {
            return Err("Enter a valid character name before saving the voice.".into());
        }
        if !(20..=1000).contains(&voice_description.trim().chars().count()) || invalid_prose(voice_description) {
            return Err("Enter a valid voice description before saving the voice.".into());
        }
        validate_remote_id(generated_voice_id, "generated voice")?;
        let body = serde_json::json!({
            "voice_name": voice_name,
            "voice_description": voice_description,
            "generated_voice_id": generated_voice_id
        });
        let response = self.request("/v1/text-to-voice", &body.to_string())?;
        let response: CreateResponse = serde_json::from_slice(&response).map_err(|_| "ElevenLabs returned an invalid saved voice response.".to_string())?;
        validate_remote_id(&response.voice_id, "saved voice")?;
        Ok(response.voice_id)
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
        if body.len() > DESIGN_BODY_LIMIT as usize { return Err("ElevenLabs request is too large.".into()); }
        let url = format!("{}{}", self.base_url, path);
        let speech = path.contains("text-to-speech") || path.contains("text-to-dialogue");
        let agent = ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(if speech { 30 } else { 180 })).build();
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
        if speech { validate_pcm_type(response.header("Content-Type").unwrap_or_default())?; }
        read_limited(response.into_reader(), if speech { AUDIO_LIMIT } else { DESIGN_BODY_LIMIT as usize })
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

#[derive(Deserialize)]
struct DesignResponse {
    previews: Vec<PreviewResponse>,
}

#[derive(Deserialize)]
struct PreviewResponse {
    audio_base_64: String,
    generated_voice_id: String,
    duration_secs: f64,
    media_type: String,
}

#[derive(Deserialize)]
struct CreateResponse {
    voice_id: String,
}

impl TryFrom<PreviewResponse> for VoicePreview {
    type Error = String;

    fn try_from(value: PreviewResponse) -> Result<Self, Self::Error> {
        validate_remote_id(&value.generated_voice_id, "generated voice")?;
        validate_pcm_type(&value.media_type)?;
        if !value.duration_secs.is_finite() || !(0.0 < value.duration_secs && value.duration_secs <= 300.0) {
            return Err("ElevenLabs returned an invalid preview duration.".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value.audio_base_64.as_bytes())
            .map_err(|_| "ElevenLabs returned invalid preview audio.".to_string())?;
        if bytes.len() > PREVIEW_LIMIT { return Err("ElevenLabs preview audio is too large.".into()); }
        let samples = decode_pcm(&bytes)?;
        if samples.is_empty() { return Err("ElevenLabs returned empty preview audio.".into()); }
        Ok(Self { generated_voice_id: value.generated_voice_id, duration_secs: value.duration_secs, samples })
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

fn invalid_prose(value: &str) -> bool {
    value.chars().any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
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
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    fn mock(response: String, expected: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 { break; }
                request.extend_from_slice(&buffer[..count]);
                if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers.lines().find_map(|line| line.strip_prefix("Content-Length:").and_then(|length| length.trim().parse().ok())).unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length { break; }
                }
            }
            let request_text = String::from_utf8_lossy(&request).into_owned();
            assert!(request_text.contains(expected));
            let _ = stream.write_all(response.as_bytes());
            request_text
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn missing_settings_key_disables_elevenlabs() {
        assert!(Client::from_settings(&crate::settings::Settings::default()).unwrap().is_none());
    }

    #[test]
    fn base_url_policy_rejects_insecure_non_loopback_servers() {
        assert!(normalize_base_url("https://api.elevenlabs.io").is_ok());
        assert!(normalize_base_url("http://127.0.0.1:1234").is_ok());
        assert!(normalize_base_url("http://[::1]:1234").is_ok());
        assert!(normalize_base_url("http://192.168.1.4:1234").is_err());
        assert!(normalize_base_url("https://example.test").is_err());
    }

    #[test]
    fn multiline_design_text_is_sent_without_losing_line_breaks() {
        let audio = base64::engine::general_purpose::STANDARD.encode(pcm_bytes(&[123, -456]));
        let body = serde_json::json!({"previews":[{"audio_base_64":audio,"generated_voice_id":"multiline","duration_secs":0.1,"media_type":"audio/pcm"}]}).to_string();
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let (base, handle) = mock(response, "text-to-voice/design");
        let sample = "Hear the herald.\nListen as I deliver this announcement with warmth and clarity. Your work is ready, and every check has passed.";
        let previews = Client::new(base, "fixture-key").unwrap().design("A theatrical herald.\nWarm and clear.", sample).unwrap();
        assert_eq!(previews[0].samples, [123, -456]);
        let request = handle.join().unwrap();
        let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["text"], sample);
        assert_eq!(body["voice_description"], "A theatrical herald.\nWarm and clear.");
    }

    #[test]
    fn design_save_and_speech_use_literal_api_outputs() {
        let audio = base64::engine::general_purpose::STANDARD.encode(pcm_bytes(&[1, -2, 3]));
        let body = serde_json::json!({"previews":[
            {"audio_base_64":audio,"generated_voice_id":"generated-0","duration_secs":0.1,"media_type":"audio/pcm"},
            {"audio_base_64":base64::engine::general_purpose::STANDARD.encode(pcm_bytes(&[4, 5])),"generated_voice_id":"generated-1","duration_secs":0.1,"media_type":"audio/pcm"},
            {"audio_base_64":base64::engine::general_purpose::STANDARD.encode(pcm_bytes(&[6, 7])),"generated_voice_id":"generated-2","duration_secs":0.1,"media_type":"audio/pcm"}
        ]}).to_string();
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
        let (base, handle) = mock(response, "text-to-voice/design");
        let client = Client::new(base, "key").unwrap();
        let previews = client.design("A clear announcer voice", &"sample text ".repeat(10)).unwrap();
        assert_eq!(previews.len(), 3);
        assert_eq!(previews[1].generated_voice_id, "generated-1");
        let request = handle.join().unwrap();
        assert!(request.contains("eleven_ttv_v3"));

        let body = "{\"voice_id\":\"saved-generated-0\"}";
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let (base, handle) = mock(response, "text-to-voice");
        let client = Client::new(base, "key").unwrap();
        assert_eq!(client.create_voice("Royal herald", "A clear announcer voice", "generated-0").unwrap(), "saved-generated-0");
        assert!(handle.join().unwrap().contains("generated_voice_id"));

        let pcm = pcm_bytes(&[257, 514]);
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: {}\r\n\r\n", pcm.len());
        let (base, handle) = mock(format!("{response}{}", String::from_utf8_lossy(&pcm)), "text-to-speech/saved-voice");
        let client = Client::new(base, "key").unwrap();
        assert_eq!(client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap(), [257, 514]);
        assert!(handle.join().unwrap().contains("eleven_flash_v2_5"));
    }

    #[test]
    fn malformed_and_http_error_outputs_are_rejected_without_body_details() {
        let (base, handle) = mock("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: 1\r\n\r\n{".into(), "text-to-speech");
        let client = Client::new(base, "secret-key").unwrap();
        let error = client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap_err();
        assert_eq!(error.to_string(), "ElevenLabs returned invalid PCM audio.");
        handle.join().unwrap();

        let (base, handle) = mock("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 20\r\n\r\nsecret response body".into(), "text-to-voice/design");
        let client = Client::new(base, "secret-key").unwrap();
        let error = client.design("A clear announcer voice", &"sample text ".repeat(10)).unwrap_err();
        assert!(!error.contains("secret"));
        handle.join().unwrap();
    }

    #[test]
    fn redirects_do_not_forward_the_key_and_encoded_audio_is_rejected() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let response = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/stolen\r\nContent-Length: 0\r\n\r\n", destination.local_addr().unwrap());
        let (base, handle) = mock(response, "text-to-voice/design");
        let client = Client::new(base, "fixture-secret").unwrap();
        assert_eq!(client.design("A clear theatrical announcer", &"sample text ".repeat(10)).unwrap_err(), "ElevenLabs request failed with HTTP 302.");
        handle.join().unwrap();
        assert_eq!(destination.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(validate_pcm_type("audio/mpeg"), Err("ElevenLabs returned an unsupported audio format; expected PCM at 16 kHz.".into()));
        assert_eq!(decode_pcm(b"RIFFencoded audio!"), Err("ElevenLabs returned encoded audio instead of PCM.".into()));
    }

    #[test]
    fn speech_cancellation_does_not_wait_for_a_slow_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let _ = stream.read(&mut bytes);
            thread::sleep(Duration::from_millis(200));
        });
        let client = Client::new(format!("http://{address}"), "key").unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            stop.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        assert_eq!(client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &cancelled), Err(SpeechError::Cancelled));
        assert!(started.elapsed() < Duration::from_millis(180));
        handle.join().unwrap();
    }

    #[test]
    fn saved_key_is_used_without_environment_resolution() {
        let settings = crate::settings::Settings { elevenlabs_api_key: Some("saved-key".into()), ..Default::default() };
        assert_eq!(Client::from_settings(&settings).unwrap().unwrap().api_key, "saved-key");
    }

    #[test]
    fn v4_uses_dialogue_and_validates_pcm() {
        let pcm = pcm_bytes(&[257, 514]);
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: {}\r\n\r\n{}", pcm.len(), String::from_utf8_lossy(&pcm));
        let (base, handle) = mock(response, "text-to-dialogue?");
        let client = Client::new(base, "key").unwrap();
        assert_eq!(client.synthesize_speech("saved-voice", "Hello", SpeechModel::V4, &Arc::new(AtomicBool::new(false))).unwrap(), [257, 514]);
        let request = handle.join().unwrap();
        let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["model_id"], "eleven_v4");
        assert_eq!(body["inputs"][0]["voice_id"], "saved-voice");
        assert_eq!(body["inputs"][0]["text"], "Hello");
    }

    #[test]
    fn turbo_uses_authenticated_websocket_and_collects_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept_hdr(stream, |request: &tungstenite::handshake::server::Request, response| {
                assert_eq!(request.headers()["xi-api-key"], "fixture-key");
                assert!(request.uri().query().unwrap().contains("model_id=eleven_v4_turbo"));
                Ok(response)
            }).unwrap();
            let init: serde_json::Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(init["voices"][0], "saved-voice");
            let input: serde_json::Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(input["inputs"][0]["text"], "Hello");
            let _ = socket.read().unwrap();
            for samples in [[1, -2], [3, -4]] {
                let audio = base64::engine::general_purpose::STANDARD.encode(pcm_bytes(&samples));
                socket.send(tungstenite::Message::Text(serde_json::json!({"audio": audio}).to_string())).unwrap();
            }
            socket.send(tungstenite::Message::Text("{\"is_final\":true}".into())).unwrap();
        });
        let client = Client::new(format!("http://{address}"), "fixture-key").unwrap();
        assert_eq!(client.synthesize_speech("saved-voice", "Hello", SpeechModel::V4Turbo, &Arc::new(AtomicBool::new(false))).unwrap(), [1, -2, 3, -4]);
        handle.join().unwrap();
    }
}
