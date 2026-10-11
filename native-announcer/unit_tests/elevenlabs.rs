use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

fn isolated_speech_test(body: impl FnOnce()) {
    const CHILD: &str = "HERALD_SPEECH_TEST_CHILD";
    let name = std::thread::current().name().expect("speech test thread is unnamed").to_owned();
    let done = format!("HERALD_SPEECH_TEST_DONE {name}");
    if let Some(marker) = std::env::var_os(CHILD) {
        assert_eq!(marker, std::ffi::OsStr::new(name.as_str()));
        body();
        println!("{done}");
        return;
    }
    let output = Command::new(std::env::current_exe().expect("speech test executable is unavailable"))
        .args(["--exact", name.as_str(), "--nocapture", "--test-threads=1"])
        .env(CHILD, &name)
        .output()
        .expect("speech test child failed to start");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = format!("{name}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(output.status.code(), Some(0), "{detail}");
    assert!(stdout.contains(&done), "{detail}");
}

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
fn voice_usage_get_is_authenticated_and_calculates_remaining_limits() {
    let body = serde_json::json!({
        "voice_slots_used": 2,
        "voice_limit": 10,
        "voice_add_edit_counter": 3,
        "max_voice_add_edits": 65
    }).to_string();
    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let (base, handle) = mock(response, "GET /v1/user/subscription");
    let usage = Client::new(base, "fixture-key").unwrap().voice_usage().unwrap();
    assert_eq!(usage.voice_slots_used, 2);
    assert_eq!(usage.voice_limit, 10);
    assert_eq!(usage.voice_add_edit_counter, 3);
    assert_eq!(usage.max_voice_add_edits, Some(65));
    assert_eq!(usage.remaining_voice_slots(), 8);
    assert_eq!(usage.voice_add_edit_allowance(), Some((65, 62)));
    assert!(handle.join().unwrap().to_ascii_lowercase().contains("xi-api-key: fixture-key"));
}

#[test]
fn voice_usage_allows_missing_or_null_edit_limit() {
    for body in [
        r#"{"voice_slots_used":2,"voice_limit":10,"voice_add_edit_counter":3}"#,
        r#"{"voice_slots_used":2,"voice_limit":10,"voice_add_edit_counter":3,"max_voice_add_edits":null}"#,
    ] {
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let (base, handle) = mock(response, "GET /v1/user/subscription");
        let usage = Client::new(base, "fixture-key").unwrap().voice_usage().unwrap();
        assert_eq!(usage.max_voice_add_edits, None);
        assert_eq!(usage.voice_add_edit_allowance(), None);
        handle.join().unwrap();
    }
}

#[test]
fn voice_usage_remaining_limits_saturate_at_zero() {
    let body = r#"{"voice_slots_used":12,"voice_limit":10,"voice_add_edit_counter":70,"max_voice_add_edits":65}"#;
    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let (base, handle) = mock(response, "GET /v1/user/subscription");
    let usage = Client::new(base, "fixture-key").unwrap().voice_usage().unwrap();
    assert_eq!(usage.remaining_voice_slots(), 0);
    assert_eq!(usage.voice_add_edit_allowance(), Some((65, 0)));
    handle.join().unwrap();
}

#[test]
fn voice_usage_rejects_malformed_and_private_http_errors() {
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\nnot valid json!!".to_string();
    let (base, handle) = mock(response, "GET /v1/user/subscription");
    let client = Client::new(base, "secret-api-key").unwrap();
    assert_eq!(client.voice_usage().unwrap_err(), "ElevenLabs returned an invalid voice usage response.");
    handle.join().unwrap();

    for status in [401, 403] {
        let response = format!("HTTP/1.1 {status} Unauthorized\r\nContent-Length: 23\r\n\r\nsecret response details");
        let (base, handle) = mock(response, "GET /v1/user/subscription");
        let error = Client::new(base, "secret-api-key").unwrap().voice_usage().unwrap_err();
        assert!(error.contains(&status.to_string()));
        assert!(error.contains("invalid") && error.contains("permission"));
        assert!(!error.contains("secret-api-key") && !error.contains("secret response"));
        handle.join().unwrap();
    }
}

#[test]
fn voice_usage_does_not_follow_redirects() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let response = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/stolen\r\nContent-Length: 0\r\n\r\n", destination.local_addr().unwrap());
    let (base, handle) = mock(response, "GET /v1/user/subscription");
    let error = Client::new(base, "fixture-secret").unwrap().voice_usage().unwrap_err();
    assert_eq!(error, "ElevenLabs request failed with HTTP 302.");
    handle.join().unwrap();
    assert_eq!(destination.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn multiline_speech_text_is_sent_without_losing_line_breaks() {
    isolated_speech_test(|| {
        let pcm = pcm_bytes(&[257, 514]);
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: {}\r\n\r\n{}", pcm.len(), String::from_utf8_lossy(&pcm));
        let (base, handle) = mock(response, "text-to-speech/saved-voice");
        let sample = "Hear the herald.\nListen as I deliver this announcement with warmth and clarity. Your work is ready, and every check has passed.";
        assert_eq!(Client::new(base, "fixture-key").unwrap().synthesize_speech("saved-voice", sample, SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap(), [257, 514]);
        let request = handle.join().unwrap();
        assert!(request.to_ascii_lowercase().contains("xi-api-key: fixture-key"));
        let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["text"], sample);
    });
}

#[test]
fn speech_uses_literal_api_outputs() {
    isolated_speech_test(|| {
        let pcm = pcm_bytes(&[257, 514]);
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: {}\r\n\r\n", pcm.len());
        let (base, handle) = mock(format!("{response}{}", String::from_utf8_lossy(&pcm)), "text-to-speech/saved-voice");
        let client = Client::new(base, "key").unwrap();
        assert_eq!(client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap(), [257, 514]);
        assert!(handle.join().unwrap().contains("eleven_flash_v2_5"));
    });
}

#[test]
fn malformed_and_http_error_outputs_are_rejected_without_body_details() {
    isolated_speech_test(|| {
        let (base, handle) = mock("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: 1\r\n\r\n{".into(), "text-to-speech");
        let client = Client::new(base, "secret-key").unwrap();
        let error = client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap_err();
        assert_eq!(error.to_string(), "ElevenLabs returned invalid PCM audio.");
        handle.join().unwrap();

        let (base, handle) = mock("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 20\r\n\r\nsecret response body".into(), "text-to-speech");
        let client = Client::new(base, "secret-key").unwrap();
        let error = client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap_err();
        assert_eq!(error.to_string(), "ElevenLabs request failed with HTTP 500.");
        assert!(!error.to_string().contains("secret"));
        handle.join().unwrap();
    });
}

#[test]
fn redirects_do_not_forward_the_key_and_encoded_audio_is_rejected() {
    isolated_speech_test(|| {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let response = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/stolen\r\nContent-Length: 0\r\n\r\n", destination.local_addr().unwrap());
        let (base, handle) = mock(response, "text-to-speech");
        let client = Client::new(base, "fixture-secret").unwrap();
        assert_eq!(client.synthesize_speech("saved-voice", "Hello", SpeechModel::Flash, &Arc::new(AtomicBool::new(false))).unwrap_err().to_string(), "ElevenLabs request failed with HTTP 302.");
        handle.join().unwrap();
        assert_eq!(destination.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(validate_pcm_type("audio/mpeg"), Err("ElevenLabs returned an unsupported audio format; expected PCM at 16 kHz.".into()));
        assert_eq!(decode_pcm(b"RIFFencoded audio!"), Err("ElevenLabs returned encoded audio instead of PCM.".into()));
    });
}

#[test]
fn speech_cancellation_does_not_wait_for_a_slow_server() {
    isolated_speech_test(|| {
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
    });
}

#[test]
fn saved_key_is_used_without_environment_resolution() {
    let settings = crate::settings::Settings { elevenlabs_api_key: Some("saved-key".into()), ..Default::default() };
    assert_eq!(Client::from_settings(&settings).unwrap().unwrap().api_key, "saved-key");
}

#[test]
fn v4_uses_dialogue_and_validates_pcm() {
    isolated_speech_test(|| {
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
    });
}

#[test]
fn turbo_uses_authenticated_websocket_and_collects_audio() {
    isolated_speech_test(|| {
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
    });
}
