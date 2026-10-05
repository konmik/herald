use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub text: String,
    pub sid: i32,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    Ready,
    Audio { samples: Vec<i16> },
    Done,
    Error { message: String },
}

pub fn respond(response: &Response) -> Result<(), String> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, response).map_err(|error| error.to_string())?;
    output.write_all(b"\n").and_then(|_| output.flush()).map_err(|error| error.to_string())
}

struct Worker {
    child: Child,
    input: ChildStdin,
    events: mpsc::Receiver<Result<Response, String>>,
}

impl Worker {
    fn start() -> Result<Self, String> {
        use std::os::windows::process::CommandExt;
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let directory = std::env::var_os("CIVILIZED_AGENT_GPU_RUNTIME").map(std::path::PathBuf::from)
            .unwrap_or_else(|| executable.parent().unwrap().join("gpu"));
        let binary = directory.join("civilized-announcer.exe");
        if !binary.is_file() { return Err("GPU runtime is not installed".into()); }
        let data = crate::platform::data_directory();
        crate::private::directory(&data).map_err(|error| error.to_string())?;
        let path = data.join("gpu-speech.log");
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 1024 * 1024) {
            let previous = data.join("gpu-speech.previous.log");
            let _ = std::fs::remove_file(&previous);
            std::fs::rename(&path, previous).map_err(|error| error.to_string())?;
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|error| error.to_string())?;
        crate::private::harden(&path).map_err(|error| error.to_string())?;
        let mut child = Command::new(binary).arg("--gpu-speech-worker")
            .env("CIVILIZED_AGENT_TTS", crate::tts::model_directory()?)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log).creation_flags(0x08000000)
            .spawn().map_err(|error| error.to_string())?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, events) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let event = line.map_err(|error| error.to_string()).and_then(|line| serde_json::from_str(&line).map_err(|error| error.to_string()));
                if sender.send(event).is_err() { break; }
            }
        });
        let worker = Self { child, input, events };
        match worker.events.recv_timeout(Duration::from_secs(20)).map_err(|error| error.to_string())?? {
            Response::Ready => Ok(worker),
            _ => Err("GPU worker did not become ready".into()),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

#[derive(Default)]
struct State {
    worker: Option<Worker>,
    retry_after: Option<Instant>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> { STATE.get_or_init(|| Mutex::new(State::default())) }

fn ensure_worker(state: &mut State) -> Result<&mut Worker, String> {
    if state.retry_after.is_some_and(|time| time > Instant::now()) { return Err("GPU initialization is temporarily unavailable".into()); }
    if state.worker.is_none() {
        match Worker::start() {
            Ok(worker) => { state.worker = Some(worker); state.retry_after = None; }
            Err(error) => { state.retry_after = Some(Instant::now() + Duration::from_secs(30)); return Err(error); }
        }
    }
    Ok(state.worker.as_mut().unwrap())
}

pub fn prepare() -> Result<(), String> {
    let mut state = state().lock().map_err(|_| "GPU worker failed")?;
    ensure_worker(&mut state).map(|_| ())
}

pub fn release() {
    if let Ok(mut state) = state().lock() { state.worker = None; state.retry_after = None; }
}

pub fn generate(text: &str, sid: i32, cancelled: &AtomicBool, sender: &mpsc::SyncSender<Vec<i16>>) -> Result<(), (String, bool)> {
    let mut state = state().lock().map_err(|_| ("GPU worker failed".into(), false))?;
    let mut sent = false;
    let result = (|| -> Result<(), String> {
        let worker = ensure_worker(&mut state)?;
        serde_json::to_writer(&mut worker.input, &Request { text: text.into(), sid }).map_err(|error| error.to_string())?;
        worker.input.write_all(b"\n").and_then(|_| worker.input.flush()).map_err(|error| error.to_string())?;
        let started = Instant::now();
        loop {
            if cancelled.load(Ordering::Relaxed) { return Ok(()); }
            if started.elapsed() > Duration::from_secs(90) { return Err("GPU speech timed out".into()); }
            match worker.events.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(Response::Audio { samples })) => {
                    if sender.send(samples).is_err() { return Ok(()); }
                    sent = true;
                }
                Ok(Ok(Response::Done)) => return Ok(()),
                Ok(Ok(Response::Error { message })) | Ok(Err(message)) => return Err(message),
                Ok(Ok(Response::Ready)) => return Err("Unexpected GPU worker response".into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {},
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("GPU worker stopped unexpectedly".into()),
            }
        }
    })();
    if result.is_err() || cancelled.load(Ordering::Relaxed) {
        state.worker = None;
        state.retry_after = if cancelled.load(Ordering::Relaxed) { None } else { Some(Instant::now() + Duration::from_secs(30)) };
    }
    result.map_err(|error| (error, sent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_protocol_preserves_pcm_and_request_text() {
        let response = serde_json::to_vec(&Response::Audio { samples: vec![i16::MIN, 0, i16::MAX] }).unwrap();
        let Response::Audio { samples } = serde_json::from_slice(&response).unwrap() else { panic!() };
        assert_eq!(samples, [i16::MIN, 0, i16::MAX]);
        let request = serde_json::to_vec(&Request { text: "Hello\nworld".into(), sid: 2 }).unwrap();
        assert_eq!(serde_json::from_slice::<Request>(&request).unwrap().text, "Hello\nworld");
    }
}
