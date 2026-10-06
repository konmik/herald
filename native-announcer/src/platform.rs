use crate::state::{log, MeetingStatus};
use crate::characters::{ResolvedCharacter, ResolvedVoice};
use crate::settings::Settings;
use crate::window::Window;
use std::path::{Path, PathBuf};
use std::process::Child;
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn data_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("CIVILIZED_AGENT_DATA") {
        return path.into();
    }
    let home = std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_else(|| ".".into());
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or(home.join("AppData/Local"))
            .join("CivilizedAgent")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/CivilizedAgent")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".local/share"))
            .join("CivilizedAgent")
    }
}

pub fn show(window: &Window) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let hwnd = window.hwnd() as *mut _;
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            style | WS_EX_NOACTIVATE as isize | WS_EX_TOOLWINDOW as isize,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    #[cfg(not(target_os = "windows"))]
    window.set_visible(true);
}

pub fn foreground() -> usize {
    #[cfg(target_os = "windows")]
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as usize
    }
    #[cfg(not(target_os = "windows"))]
    {
        0
    }
}

pub fn is_foreground(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        foreground() == window.hwnd() as usize
    }
    #[cfg(not(target_os = "windows"))]
    {
        window.is_focused()
    }
}

pub fn opacity(window: &Window, amount: f32) -> bool {
    let amount = amount.clamp(0.0, 1.0);
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        SetLayeredWindowAttributes(
            window.hwnd() as *mut _,
            0x00ff00ff,
            (amount * 255.0).round() as u8,
            LWA_ALPHA | LWA_COLORKEY,
        ) != 0
    }
    #[cfg(target_os = "macos")]
    unsafe {
        use tao::platform::macos::WindowExtMacOS;
        let native = &*(window.ns_window() as *const objc2_app_kit::NSWindow);
        native.setAlphaValue(amount as f64);
        true
    }
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::WidgetExt;
        use tao::platform::unix::WindowExtUnix;
        window.gtk_window().set_opacity(amount as f64);
        true
    }
}

pub fn hide(window: &Window) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        let _ = ShowWindow(window.hwnd() as *mut _, SW_HIDE);
    }
    #[cfg(not(target_os = "windows"))]
    window.set_visible(false);
}

pub fn passive_window(window: &Window, visible: bool) -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let hwnd = window.hwnd() as *mut _;
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        (IsWindowVisible(hwnd) != 0) == visible
            && style & WS_EX_NOACTIVATE as isize != 0
            && style & WS_EX_TOPMOST as isize != 0
    }
    #[cfg(not(target_os = "windows"))]
    {
        window.is_visible() == visible
    }
}

#[cfg(not(target_os = "windows"))]
pub fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    command
}

struct SpeechCommand {
    text: String,
    id: String,
    voice: ResolvedVoice,
    fallback_character: String,
    fallback_speaker: Option<String>,
    settings: Settings,
    volume: Arc<AtomicU16>,
    cancelled: Arc<AtomicBool>,
}

pub struct Speech {
    pub events: mpsc::Receiver<(String, Result<(), String>)>,
    sender: mpsc::Sender<SpeechCommand>,
    cancelled: Arc<AtomicBool>,
    volume: Arc<AtomicU16>,
}

impl Speech {
    pub fn new(preload: bool) -> Self {
        let (sender, commands) = mpsc::channel::<SpeechCommand>();
        let (events, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            if preload {
                #[cfg(target_os = "windows")]
                if let Err(error) = crate::tts::prepare() { log(&data_directory(), error); }
            }
            for SpeechCommand {
                text,
                id,
                voice,
                fallback_character,
                fallback_speaker,
                settings,
                volume,
                cancelled,
            } in commands
            {
                if cancelled.load(Ordering::Relaxed) {
                    continue;
                }
                let result = speak(&text, &voice, &fallback_character, fallback_speaker.as_deref(), &volume, &cancelled, &settings);
                let _ = events.send((id, result));
            }
        });
        Self {
            events: receiver,
            sender,
            cancelled: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicU16::new(100)),
        }
    }

    pub fn start(&mut self, text: &str, id: &str, character: &ResolvedCharacter, settings: &Settings) {
        self.cancel();
        self.set_volume(settings.volume);
        self.cancelled = Arc::new(AtomicBool::new(false));
        let _ = self.sender.send(SpeechCommand {
            text: text.into(),
            id: id.into(),
            voice: character.voice.clone(),
            fallback_character: character.fallback_character.clone(),
            fallback_speaker: character.fallback_speaker.clone(),
            settings: settings.clone(),
            volume: self.volume.clone(),
            cancelled: self.cancelled.clone(),
        });
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: u16) { self.volume.store(volume.min(100), Ordering::Relaxed); }
}

impl Drop for Speech {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(target_os = "windows")]
use crate::tts::speak;

#[cfg(not(target_os = "windows"))]
fn speak(
    text: &str,
    voice: &ResolvedVoice,
    fallback_character: &str,
    fallback_speaker: Option<&str>,
    volume: &AtomicU16,
    cancelled: &AtomicBool,
    _settings: &Settings,
) -> Result<(), String> {
    let preferred = match voice {
        ResolvedVoice::Local { speaker } => speaker.as_deref().or(fallback_speaker),
        ResolvedVoice::ElevenLabs { .. } => fallback_speaker,
    };
    let mut command = if cfg!(target_os = "macos") {
        let mut command = hidden_command("say");
        command.args(["-r", if fallback_character == "claude" { "180" } else { "160" }]);
        let available = Command::new("say")
            .args(["-v", "?"])
            .output()
            .map_err(|e| e.to_string())?;
        let available = String::from_utf8_lossy(&available.stdout);
        let names = preferred.map(|p| vec![p]).unwrap_or_else(|| {
            if fallback_character == "claude" {
                vec!["Alex", "Daniel"]
            } else {
                vec!["Daniel", "Alex"]
            }
        });
        if let Some(name) = names.into_iter().find(|name| {
            available
                .lines()
                .any(|line| line.starts_with(&format!("{name} ")))
        }) {
            command.args(["-v", name]);
        }
        command
    } else {
        let program = if Command::new("espeak-ng").arg("--version").output().is_ok() {
            "espeak-ng"
        } else {
            "espeak"
        };
        let mut command = hidden_command(program);
        command.args(["-a", &(crate::settings::volume_gain(volume.load(Ordering::Relaxed)) * 100.0).round().to_string()]);
        command.args([
            "-v",
            preferred.unwrap_or(if fallback_character == "claude" {
                "en-us+m2"
            } else {
                "en-us+m3"
            }),
            "-s",
            if fallback_character == "claude" { "180" } else { "160" },
        ]);
        command
    };
    let spoken = if cfg!(target_os = "macos") { format!("[[volm {}]]{text}", crate::settings::volume_gain(volume.load(Ordering::Relaxed))) } else { text.into() };
    let mut child = command.arg(spoken).spawn().map_err(|e| e.to_string())?;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("Speech exited with {status}"))
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub struct Signal {
    path: PathBuf,
    child: Option<Child>,
    #[cfg(target_os = "windows")]
    playback: Option<std::thread::JoinHandle<()>>,
    cancelled: Arc<AtomicBool>,
}

impl Signal {
    pub fn new(data: &Path) -> Self {
        let path = data.join("interference-v4.wav");
        if !path.exists() {
            let samples = (crate::state::TRANSITION_DURATION.as_secs_f32() * 16000.0) as u32;
            let sample_bytes = samples * 2;
            let mut wav = Vec::new();
            wav.extend(b"RIFF");
            wav.extend((36 + sample_bytes).to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16u32.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(16000u32.to_le_bytes());
            wav.extend(32000u32.to_le_bytes());
            wav.extend(2u16.to_le_bytes());
            wav.extend(16u16.to_le_bytes());
            wav.extend(b"data");
            wav.extend(sample_bytes.to_le_bytes());
            let mut seed = 734971u32;
            let mut filtered = 0.0_f32;
            for index in 0..samples {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let white = (seed % 681) as f32 - 340.0;
                filtered = filtered * 0.35 + white * 0.65;
                let crackle = if seed.is_multiple_of(257) {
                    if seed & 1 == 0 {
                        180.0
                    } else {
                        -180.0
                    }
                } else {
                    0.0
                };
                let envelope = crate::state::interference_amount(
                    Duration::from_secs_f32(crate::state::TRANSITION_DURATION.as_secs_f32() * index as f32 / (samples - 1) as f32),
                    734971,
                );
                let value = ((white * 0.7 + filtered * 0.3 + crackle) * envelope * 24.0) as i16;
                wav.extend(value.to_le_bytes());
            }
            let _ = std::fs::write(&path, wav);
        }
        Self { path, child: None, #[cfg(target_os = "windows")] playback: None, cancelled: Arc::new(AtomicBool::new(false)) }
    }

    pub fn play(&mut self, settings: &Settings) {
        self.stop();
        if settings.volume == 0 { return; }
        self.cancelled = Arc::new(AtomicBool::new(false));
        #[cfg(target_os = "windows")]
        {
            let path = self.path.clone();
            let settings = settings.clone();
            let cancelled = self.cancelled.clone();
            self.playback = Some(std::thread::spawn(move || {
                if let Err(error) = crate::audio::play_noise(&path, settings.volume, settings.output_device.as_deref(), &cancelled) {
                    log(path.parent().unwrap(), error);
                }
            }));
        }
        #[cfg(not(target_os = "windows"))]
        {
            let programs = if cfg!(target_os = "macos") {
                vec!["afplay"]
            } else {
                vec!["paplay", "aplay"]
            };
            for program in programs {
                let mut command = hidden_command(program);
                if program == "afplay" { command.args(["-v", &crate::settings::volume_gain(settings.volume).to_string()]); }
                if program == "paplay" { command.arg(format!("--volume={}", (crate::settings::volume_gain(settings.volume) * 65536.0).round() as u32)); }
                if program == "aplay" && settings.volume != 100 { continue; }
                if let Ok(child) = command.arg(&self.path).spawn() {
                    self.child = Some(child);
                    break;
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        if let Some(playback) = self.playback.take() {
            let _ = playback.join();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "windows")]
pub struct Preview {
    cancelled: Arc<AtomicBool>,
    playback: Option<std::thread::JoinHandle<Result<(), String>>>,
}

#[cfg(target_os = "windows")]
impl Preview {
    pub fn start(data: PathBuf, settings: Settings) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let playback = std::thread::spawn(move || {
            let signal = Signal::new(&data);
            crate::audio::play_noise(&signal.path, settings.volume, settings.output_device.as_deref(), &stop)?;
            if stop.load(Ordering::Relaxed) { return Ok(()); }
            let mut speech = Speech::new(false);
            let character = crate::characters::resolve(&settings, std::path::Path::new(""), "opencode");
            speech.start("This is an announcement", "settings-preview", &character, &settings);
            let started = std::time::Instant::now();
            loop {
                if stop.load(Ordering::Relaxed) { speech.cancel(); return Ok(()); }
                match speech.events.recv_timeout(Duration::from_millis(50)) {
                    Ok((_, result)) => return result,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Err("Preview speech stopped unexpectedly.".into()),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if started.elapsed() >= Duration::from_secs(30) { return Err("Preview speech timed out.".into()); }
            }
        });
        Self { cancelled, playback: Some(playback) }
    }

    pub fn finished(&mut self) -> Option<Result<(), String>> {
        if !self.playback.as_ref()?.is_finished() { return None; }
        Some(self.playback.take()?.join().unwrap_or_else(|_| Err("Audio preview failed.".into())))
    }
}

#[cfg(target_os = "windows")]
impl Drop for Preview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(playback) = self.playback.take() { let _ = playback.join(); }
    }
}

#[cfg(target_os = "windows")]
pub struct PcmPreview {
    cancelled: Arc<AtomicBool>,
    playback: Option<std::thread::JoinHandle<Result<(), String>>>,
}

#[cfg(target_os = "windows")]
impl PcmPreview {
    pub fn start(samples: Vec<i16>, settings: Settings) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let playback = std::thread::spawn(move || {
            if stop.load(Ordering::Relaxed) { return Ok(()); }
            crate::audio::play_pcm(&samples, 16000, &AtomicU16::new(settings.volume), settings.output_device.as_deref(), &stop)
        });
        Self { cancelled, playback: Some(playback) }
    }

    pub fn finished(&mut self) -> Option<Result<(), String>> {
        if !self.playback.as_ref()?.is_finished() { return None; }
        Some(self.playback.take()?.join().unwrap_or_else(|_| Err("Voice preview failed.".into())))
    }
}

#[cfg(target_os = "windows")]
impl Drop for PcmPreview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(playback) = self.playback.take() { let _ = playback.join(); }
    }
}

pub fn detect_meetings(
    data: PathBuf,
    status: Arc<MeetingStatus>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match meeting_active() {
                Ok(value) => status.update(Some(value), std::time::Instant::now()),
                Err(error) => {
                    status.update(None, std::time::Instant::now());
                    log(&data, format!("Meeting detection: {error}"));
                }
            }
            for _ in 0..50 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

pub fn meeting_override(data: &Path) -> bool {
    std::fs::read(data.join("meeting.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .is_some_and(|v| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            v["active"].as_bool() == Some(true)
                && v["updated"]
                    .as_f64()
                    .is_some_and(|at| at <= now && now - at < 30.0)
        })
}

#[cfg(target_os = "windows")]
fn microphone_active() -> windows::core::Result<bool> {
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        for index in 0..devices.GetCount()? {
            let manager: IAudioSessionManager2 = devices.Item(index)?.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for index in 0..sessions.GetCount()? {
                if sessions.GetSession(index)?.GetState()? == AudioSessionStateActive {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(any(target_os = "windows", test))]
fn meeting_evidence(capture: Result<bool, String>, controls: Result<bool, String>) -> Result<bool, String> {
    if matches!(capture, Ok(true)) || matches!(controls, Ok(true)) { return Ok(true); }
    capture.and(controls)
}

#[cfg(target_os = "windows")]
fn meeting_active() -> Result<bool, String> {
    use windows::core::BSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Accessibility::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn inspect(
        hwnd: windows_sys::Win32::Foundation::HWND,
        value: isize,
    ) -> i32 {
        let mut title = [0u16; 512];
        let length = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        let title = String::from_utf16_lossy(&title[..length as usize]).to_lowercase();
        if [
            "teams", "zoom", "slack", "webex", "discord", "chrome", "edge", "firefox",
        ]
        .iter()
        .any(|app| title.contains(app))
        {
            unsafe { &mut *(value as *mut Vec<(HWND, String)>) }.push((HWND(hwnd), title));
        }
        1
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let capture = microphone_active().map_err(|error| error.to_string());
        let result = (|| -> windows::core::Result<bool> {
            if matches!(capture, Ok(true)) { return Ok(true); }
            let mut windows = Vec::<(HWND, String)>::new();
            if EnumWindows(Some(inspect), &mut windows as *mut _ as isize) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            if windows
                .iter()
                .any(|(_, title)| title == "zoom meeting" || title == "zoom workplace meeting")
            {
                return Ok(true);
            }
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let mut names = automation
                .CreatePropertyCondition(UIA_NamePropertyId, &BSTR::from("Leave").into())?;
            for name in ["Leave call", "Leave meeting", "End call"] {
                let condition = automation
                    .CreatePropertyCondition(UIA_NamePropertyId, &BSTR::from(name).into())?;
                names = automation.CreateOrCondition(&names, &condition)?;
            }
            let button_type = automation.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &UIA_ButtonControlTypeId.0.into(),
            )?;
            let condition = automation.CreateAndCondition(&names, &button_type)?;
            for (hwnd, _) in windows {
                let Ok(element) = automation.ElementFromHandle(hwnd) else {
                    continue;
                };
                if element.FindFirst(TreeScope_Descendants, &condition).is_ok() {
                    return Ok(true);
                }
            }
            Ok(false)
        })();
        CoUninitialize();
        meeting_evidence(capture, result.map_err(|e| e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "Plays speech on the system audio output"]
    fn speech_uses_the_same_pcm_volume_path_as_static() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_secs(10)); stop.store(true, Ordering::Relaxed); });
        let settings = Settings { output_device: Some("unavailable-test-device".into()), ..Default::default() };
        speak("This is an announcement", &ResolvedVoice::Local { speaker: None }, "opencode", None, &AtomicU16::new(75), &cancelled, &settings).unwrap();
        assert!(!cancelled.load(Ordering::Relaxed), "Speech must finish without timing out");
    }

    #[test]
    fn capture_activity_keeps_hidden_meetings_muted_and_probe_failures_fail_closed() {
        assert_eq!(meeting_evidence(Ok(true), Ok(false)), Ok(true));
        assert_eq!(meeting_evidence(Ok(false), Ok(true)), Ok(true));
        assert_eq!(meeting_evidence(Ok(false), Ok(false)), Ok(false));
        assert!(meeting_evidence(Err("Capture unavailable".into()), Ok(false)).is_err());
        assert!(meeting_evidence(Ok(false), Err("Controls unavailable".into())).is_err());
    }

    #[test]
    fn interference_audio_has_silent_edges_and_a_faded_envelope() {
        let data = std::env::temp_dir()
            .join("opencode")
            .join(format!("civilized-signal-{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        let signal = Signal::new(&data);
        let wav = std::fs::read(&signal.path).unwrap();
        let samples: Vec<_> = wav[44..]
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect();
        assert_eq!(samples.len(), 10400);
        assert_eq!(samples[0], 0);
        assert_eq!(*samples.last().unwrap(), 0);
        let edge: i32 = samples[..100].iter().map(|v| i32::from(*v).abs()).sum();
        let middle: i32 = samples[4500..5500]
            .iter()
            .map(|v| i32::from(*v).abs())
            .sum();
        assert!(middle > edge * 20);
        let peak = samples.iter().map(|value| i32::from(*value).abs()).max().unwrap();
        let rms = (samples.iter().map(|value| f64::from(*value).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
        assert!(peak > 8000 && peak < 16000, "Static should be audible without clipping: {peak}");
        assert!(rms > 2000.0, "Static should have audible average volume: {rms}");
        drop(signal);
        std::fs::remove_dir_all(data).unwrap();
    }
}

#[cfg(target_os = "macos")]
fn meeting_active() -> Result<bool, String> {
    let script = r#"tell application "System Events"
set matched to false
repeat with p in (application processes whose background only is false)
if name of p is in {"zoom.us", "Microsoft Teams", "Slack", "Webex", "Discord", "Google Chrome", "Safari", "Firefox"} then
repeat with w in windows of p
try
repeat with el in entire contents of w
if role of el is "AXButton" and name of el is in {"Leave", "Leave call", "Leave meeting", "End call"} then set matched to true
end repeat
end try
end repeat
end if
end repeat
return matched
end tell"#;
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

#[cfg(target_os = "linux")]
fn meeting_active() -> Result<bool, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(4), async {
            use atspi::proxy::accessible::AccessibleProxy;
            let connection = atspi::AccessibilityConnection::new()
                .await
                .map_err(|e| e.to_string())?;
            let proxy = connection
                .root_accessible_on_registry()
                .await
                .map_err(|e| e.to_string())?;
            let mut pending = proxy.get_children().await.map_err(|e| e.to_string())?;
            let mut count = 0;
            while let Some(reference) = pending.pop() {
                count += 1;
                if count > 1500 {
                    break;
                }
                let Some(name) = reference.name() else {
                    continue;
                };
                let proxy = AccessibleProxy::builder(connection.connection())
                    .destination(name.as_str())
                    .map_err(|e| e.to_string())?
                    .path(reference.path().as_str())
                    .map_err(|e| e.to_string())?
                    .build()
                    .await
                    .map_err(|e| e.to_string())?;
                let name = proxy.name().await.unwrap_or_default();
                if ["Leave", "Leave call", "Leave meeting", "End call"].contains(&name.as_str())
                    && proxy
                        .get_role()
                        .await
                        .is_ok_and(|role| role == atspi::Role::Button)
                {
                    return Ok(true);
                }
                if let Ok(children) = proxy.get_children().await {
                    let limit = 100.min(4096usize.saturating_sub(pending.len()));
                    pending.extend(children.into_iter().take(limit));
                }
            }
            Ok(false)
        })
        .await
        .map_err(|_| "Accessibility inspection timed out".to_owned())?
    })
}
