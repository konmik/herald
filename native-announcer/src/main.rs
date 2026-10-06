#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod capture;
#[cfg(any(target_os = "windows", test))]
mod audio;
mod characters;
mod elevenlabs;
mod history;
mod platform;
mod private;
mod render;
mod state;
mod settings;
mod settings_app;
#[cfg(target_os = "windows")]
mod tts;
mod video;
mod window;

use chrono::Timelike;
use platform::{Signal, Speech};
use render::Renderer;
use state::{Inbox, MeetingStatus, Notification};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use window::{
    ControlFlow, ElementState, Event, EventLoop, LogicalSize, MouseButton, PhysicalPosition,
    WindowBuilder, WindowEvent,
};

struct Active {
    notification: Notification,
    character: characters::ResolvedCharacter,
    started: Instant,
    expires: Instant,
    end: Option<Instant>,
    speaking: bool,
    speech_finished: bool,
    silent: bool,
    video: Option<video::Video>,
    video_path: PathBuf,
    history_recorded: bool,
}

impl Active {
    fn ready_to_end(&self, now: Instant) -> bool {
        now >= self.expires && (self.silent || self.speech_finished)
    }
}

fn main() {
    if let Err(error) = run() {
        let data = platform::data_directory();
        let _ = std::fs::create_dir_all(&data);
        state::log(&data, error);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut assets = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .unwrap()
        .join("../resources");
    let mut demo = None;
    let mut test_seconds = None;
    let mut report = None;
    let mut snapshot = None;
    let mut isolated = false;
    let mut open_settings = false;
    let mut capture_directory = None;
    let mut capture_speech_seconds = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--settings" => open_settings = true,
            "--isolated" => isolated = true,
            "--capture-frames" => capture_directory = Some(PathBuf::from(arguments.next().ok_or("Missing capture directory")?)),
            "--capture-speech-seconds" => {
                let seconds = arguments.next().ok_or("Missing speech duration")?.parse::<f64>().map_err(|error| error.to_string())?;
                if !seconds.is_finite() || !(0.0..=3600.0).contains(&seconds) { return Err("Invalid capture speech duration".into()); }
                capture_speech_seconds = Some(Duration::from_secs_f64(seconds));
            }
            "--assets" => assets = arguments.next().ok_or("Missing assets path")?.into(),
            "--demo" => demo = Some(arguments.next().ok_or("Missing character")?),
            "--test-seconds" => {
                test_seconds = Some(
                    arguments
                        .next()
                        .ok_or("Missing duration")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?,
                )
            }
            "--report" => {
                report = Some(PathBuf::from(
                    arguments.next().ok_or("Missing report path")?,
                ))
            }
            "--snapshot" => {
                snapshot = Some(PathBuf::from(
                    arguments.next().ok_or("Missing snapshot path")?,
                ))
            }
            _ => return Err(format!("Unknown option: {argument}")),
        }
    }
    let data = platform::data_directory();
    if open_settings { return settings_app::run(&data); }
    if isolated && std::env::var_os("CIVILIZED_AGENT_DATA").is_none() {
        return Err("Isolated playback requires CIVILIZED_AGENT_DATA".into());
    }
    if capture_directory.is_some() && !isolated {
        return Err("Frame capture requires isolated playback".into());
    }
    if capture_speech_seconds.is_some() && capture_directory.is_none() {
        return Err("Capture speech duration requires frame capture".into());
    }
    let mut frames = capture_directory.as_deref().map(capture::Frames::new).transpose()?;
    private::directory(&data).map_err(|e| e.to_string())?;
    for name in ["queue.json", "queue.tmp", "history.jsonl", "settings.json", "errors.log", "errors.previous.log"] {
        private::harden(&data.join(name)).map_err(|error| error.to_string())?;
    }
    let _lock = match std::net::TcpListener::bind(("127.0.0.1", if isolated { 0 } else { 47863 })) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let mut settings_store = settings::Store::new(&data)?;
    let mut inbox = Inbox::new(data.clone());
    let demo_mode = demo.is_some();
    if let Some(character) = demo {
        inbox.queue.push_front(Notification { id: format!("demo-{}", state::timestamp()), session_id: "demo".into(), presence_session_id: String::new(), completed: state::timestamp(), text: "The native voice adviser is ready. Announcements stay visible without taking focus.".into(), title: "Civilized Agent verification".into(), character, emotion: "neutral".into() });
    }
    let before = platform::foreground();
    let event_loop = EventLoop::new();
    #[cfg(target_os = "macos")]
    let event_loop = {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        let mut event_loop = event_loop;
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop.set_activate_ignoring_other_apps(false);
        event_loop
    };
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("Civilized Agent")
            .with_visible(false)
            .with_focused(false)
            .with_focusable(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(true)
            .with_inner_size(LogicalSize::new(320.0, 240.0))
            .build(&event_loop)
            .map_err(|e| e.to_string())?,
    );
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        window.set_skip_taskbar(true).map_err(|e| e.to_string())?;
    }
    let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
    let mut surface =
        softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
    let mut renderer = Renderer::new()?;
    let local = chrono::Local::now();
    let preload_speech = settings_store.current.volume > 0
        && !settings_store.current.quiet_at(local.hour() * 60 + local.minute())
        && !platform::meeting_override(&data);
    let mut speech = Speech::new(preload_speech);
    let mut signal = Signal::new(&data);
    let meeting = Arc::new(MeetingStatus::new());
    let stop = Arc::new(AtomicBool::new(false));
    platform::detect_meetings(
        data.clone(),
        meeting.clone(),
        stop.clone(),
    );
    let mut current: Option<Active> = None;
    let launched = Instant::now();
    let mut last_inbox = Instant::now() - Duration::from_secs(1);
    let mut next_frame = Instant::now();
    let mut focus_unchanged = true;
    let mut external_focus_changed = false;
    let mut shown = 0;
    let mut finished = 0;
    let mut max_visible = 0.0_f64;
    let mut durations = Vec::new();
    let mut titles = Vec::new();
    let mut speech_started = 0;
    let mut muted_announcements = 0;
    let mut static_frames = 0;
    let mut animation_frames = 0;
    let mut decoded_video_frames = 0;
    let mut video_loops = 0;
    let mut selected_videos = Vec::new();
    let mut video_frame_rates = Vec::new();
    let mut passive_window_ok = true;
    let mut window_opacity_updates = 0;
    let mut abrupt_window_ok = true;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(next_frame);
        match event {
            Event::WindowEvent { event: WindowEvent::Focused(true), .. } => focus_unchanged = false,
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. }
            | Event::WindowEvent { event: WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. }, .. } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent { event: WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }, .. } => {
                speech.cancel(); signal.stop(); current = None; platform::hide(&window); inbox.save(None);
            }
            Event::MainEventsCleared => {
                let now = Instant::now();
                if now < next_frame { return; }
                if test_seconds.is_some_and(|seconds| launched.elapsed() >= Duration::from_secs(seconds)) {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                if platform::is_foreground(&window) { focus_unchanged = false; }
                else if before != 0 && platform::foreground() != before { external_focus_changed = true; }
                if now.duration_since(last_inbox) >= Duration::from_millis(250) {
                    match settings_store.reload() {
                        Ok(true) => {
                            speech.set_volume(settings_store.current.volume);
                            signal.stop();
                        }
                        Err(error) => state::log(&data, format!("Settings: {error}")),
                        _ => {}
                    }
                    if inbox.read(current.as_ref().filter(|a| !demo_mode || a.notification.session_id != "demo").map(|a| &a.notification)) {
                        speech.cancel(); signal.stop(); current = None; platform::hide(&window); inbox.save(None);
                    }
                    last_inbox = now;
                }
                let settings = &settings_store.current;
                let local = chrono::Local::now();
                let muted = meeting.muted(now) || platform::meeting_override(&data) || settings.quiet_at(local.hour() * 60 + local.minute()) || settings.volume == 0;
                for (id, result) in speech.events.try_iter() {
                    if let Some(active) = current.as_mut().filter(|a| a.notification.id == id) {
                        active.speech_finished = true;
                        if let Err(error) = result { state::log(&data, format!("Speech: {error}")); }
                    }
                }
                if let Some(active) = current.as_mut() {
                    if muted && !active.silent { active.silent = true; speech.cancel(); signal.stop(); }
                }
                if current.is_none() {
                    let notification = if demo_mode && inbox.queue.front().is_some_and(|n| n.session_id == "demo") { inbox.queue.pop_front() } else { inbox.next(state::timestamp()) };
                    if let Some(notification) = notification {
                        renderer.text = notification.text.clone();
                        renderer.title = if notification.title.trim().is_empty() { "Untitled session".into() } else { notification.title.clone() };
                        let monitor = window.current_monitor().or_else(|| window.primary_monitor());
                        let max_height = monitor.as_ref().map(|m| (m.size().height as f64 / m.scale_factor() * 0.8) as u32).unwrap_or(700);
                        let height = (renderer.message_height() + 238).min(max_height).max(240);
                        window.set_inner_size(LogicalSize::new(320.0, height as f64));
                        if let Some(monitor) = monitor {
                            let scale = monitor.scale_factor();
                            let position = monitor.position();
                            window.set_outer_position(PhysicalPosition::new(position.x + monitor.size().width as i32 - (336.0 * scale) as i32, position.y + monitor.size().height as i32 - ((height + 64) as f64 * scale) as i32));
                        }
                        let mut character = characters::resolve(settings, &assets, notification.character());
                        if let Some(warning) = &character.video_warning { state::log(&data, warning); }
                        let mut path = character.video_path.clone();
                        let video = match video::Video::open(&path) {
                            Ok(video) => Some(video),
                            Err(error) => {
                                state::log(&data, format!("Video: {error}"));
                                let fallback = video::select_path(&assets, notification.character());
                                if character.id.is_some() && fallback != path {
                                    match video::Video::open(&fallback) {
                                        Ok(video) => {
                                            path = fallback;
                                            character.video_path = path.clone();
                                            Some(video)
                                        }
                                        Err(error) => { state::log(&data, format!("Fallback video: {error}")); None }
                                    }
                                } else { None }
                            }
                        };
                        if let Some(video) = &video {
                            selected_videos.push(path.to_string_lossy().into_owned());
                            video_frame_rates.push(video.fps());
                        }
                        let started = Instant::now();
                        let duration = state::display_duration(&notification.text).max(capture_speech_seconds.map(|speech| speech + state::TRANSITION_DURATION).unwrap_or_default());
                        let expires = started + duration;
                        current = Some(Active { notification, character, started, expires, end: None, speaking: false, speech_finished: muted, silent: muted, video, video_path: path, history_recorded: false });
                        abrupt_window_ok &= platform::opacity(&window, 1.0);
                        window_opacity_updates += 1;
                        let previous_focus = platform::foreground();
                        platform::show(&window);
                        if previous_focus != 0 && platform::foreground() != previous_focus { focus_unchanged = false; }
                        passive_window_ok &= platform::passive_window(&window, true);
                        window.request_redraw();
                        shown += 1;
                        titles.push(renderer.title.clone());
                        if muted { muted_announcements += 1; }
                        if !muted { signal.play(settings); }
                        inbox.save(current.as_ref().map(|a| &a.notification));
                    }
                }
                let mut dismiss = false;
                if let Some(active) = current.as_mut() {
                    if now.duration_since(active.started) >= state::TRANSITION_DURATION && !active.speaking {
                        active.speaking = true;
                        if !active.silent { speech.start(&active.notification.text, &active.notification.id, &active.character, settings); speech_started += 1; }
                    }
                    if active.end.is_none() && active.ready_to_end(now) {
                        active.end = Some(now); speech.cancel();
                        if !active.silent && !muted { signal.play(settings); }
                    }
                    if active.end.is_some_and(|end| now.duration_since(end) >= state::TRANSITION_DURATION) {
                        max_visible = max_visible.max(active.started.elapsed().as_secs_f64());
                        durations.push(active.started.elapsed().as_secs_f64());
                        dismiss = true;
                    } else { window.request_redraw(); }
                }
                if dismiss {
                    current = None; signal.stop(); platform::hide(&window); finished += 1; inbox.save(None);
                    passive_window_ok &= platform::passive_window(&window, false);
                }
                let interval = current.as_ref().map(|active| if active.started.elapsed() < state::TRANSITION_DURATION || active.end.is_some() { 42 } else { (1000.0 / active.video.as_ref().map(|video| video.fps()).unwrap_or(state::VIDEO_FPS as f64)) as u64 }).unwrap_or(250);
                next_frame = now + Duration::from_millis(interval);
                *control_flow = ControlFlow::WaitUntil(next_frame);
            }
            Event::RedrawRequested(_) => {
                if let Some(active) = &mut current {
                    let size = window.inner_size();
                    if let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                        let result = (|| -> Result<(), String> {
                            surface.resize(width, height).map_err(|e| e.to_string())?;
                            let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
                            let transition_time = active.end.map(|end| end.elapsed()).unwrap_or_else(|| active.started.elapsed());
                            let interference = state::visual_interference_amount(transition_time, active.notification.completed as u32);
                            if interference > 0.0 { static_frames += 1; } else { animation_frames += 1; }
                            if let Some(video) = &mut active.video {
                                let previous_frames = video.decoded_frames;
                                let previous_loops = video.loops;
                                if let Err(error) = video.advance(active.started.elapsed()) {
                                    state::log(&data, format!("Video playback: {error}"));
                                }
                                decoded_video_frames += video.decoded_frames - previous_frames;
                                video_loops += video.loops - previous_loops;
                            }
                            renderer.text_interference = if active.started.elapsed() < state::TRANSITION_DURATION || active.end.is_some() { interference } else { 0.0 };
                            renderer.draw(&mut buffer, size.width as usize, size.height as usize, window.scale_factor() as f32, active.video.as_ref().map(video::Video::frame), interference);
                            if let Some(frames) = &mut frames {
                                frames.save(&buffer, size.width, size.height, active.started.elapsed(), active.end.map(|end| end.duration_since(active.started)))?;
                            }
                            if active.started.elapsed() > state::TRANSITION_DURATION && active.end.is_none() {
                                if let Some(path) = snapshot.take() {
                                    let preview = image::RgbaImage::from_fn(size.width, size.height, |x, y| {
                                        let color = buffer[(y * size.width + x) as usize];
                                        image::Rgba([(color >> 16) as u8, (color >> 8) as u8, color as u8, if color == 0xff00ff { 0 } else { 255 }])
                                    });
                                    preview.save(path).map_err(|e| e.to_string())?;
                                }
                            }
                            buffer.present().map_err(|e| e.to_string())?;
                            if !active.history_recorded {
                                let result = if active.character.id.is_some() {
                                    history::record_with_identity(&data, &active.notification, active.character.history_identity(), &active.video_path, chrono::Utc::now())
                                } else {
                                    history::record(&data, &active.notification, &active.video_path, chrono::Utc::now())
                                };
                                match result {
                                    Ok(()) => active.history_recorded = true,
                                    Err(error) => state::log(&data, format!("History: {error}")),
                                }
                            }
                            Ok(())
                        })();
                        if let Err(error) = result { state::log(&data, error); *control_flow = ControlFlow::Exit; }
                    }
                }
            }
            Event::LoopDestroyed => {
                stop.store(true, Ordering::Relaxed); speech.cancel(); signal.stop();
                if let Some(active) = &current { max_visible = max_visible.max(active.started.elapsed().as_secs_f64()); }
                inbox.save(current.as_ref().map(|a| &a.notification));
                if let Some(path) = &report {
                    let report = serde_json::json!({"focusUnchanged": focus_unchanged, "focusChecked": before != 0, "externalFocusChanged": external_focus_changed, "passiveWindow": passive_window_ok, "windowOpacityUpdates": window_opacity_updates, "abruptWindowSucceeded": abrupt_window_ok, "shown": shown, "finished": finished, "visibleSeconds": max_visible, "durations": durations, "sessionTitles": titles, "speechStarted": speech_started, "mutedAnnouncements": muted_announcements, "staticFrames": static_frames, "animationFrames": animation_frames, "videoFPS": video_frame_rates.first().copied().unwrap_or(state::VIDEO_FPS as f64), "selectedVideos": selected_videos, "videoFrameRates": video_frame_rates, "decodedVideoFrames": decoded_video_frames, "videoLoops": video_loops, "decodedFrameLimit": 1});
                    let _ = std::fs::write(path, report.to_string());
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_speech_and_minimum_display_time() {
        let started = Instant::now();
        let mut active = Active {
            notification: Notification {
                id: "test".into(),
                session_id: "test".into(),
                presence_session_id: String::new(),
                completed: 1,
                text: "Done.".into(),
                title: "Test".into(),
                character: "opencode".into(),
                emotion: "neutral".into(),
            },
            character: characters::ResolvedCharacter {
                id: None,
                name: "opencode".into(),
                voice: characters::ResolvedVoice::Local { speaker: None },
                fallback_character: "opencode".into(),
                fallback_speaker: None,
                video_path: PathBuf::new(),
                video_warning: None,
            },
            started,
            expires: started + Duration::from_secs(10),
            end: None,
            speaking: true,
            speech_finished: false,
            silent: false,
            video: None,
            video_path: PathBuf::new(),
            history_recorded: false,
        };
        assert!(!active.ready_to_end(started + Duration::from_secs(20)));
        active.speech_finished = true;
        assert!(!active.ready_to_end(started + Duration::from_secs(9)));
        assert!(active.ready_to_end(started + Duration::from_secs(10)));
        active.silent = true;
        active.speech_finished = false;
        assert!(active.ready_to_end(started + Duration::from_secs(10)));
    }
}
