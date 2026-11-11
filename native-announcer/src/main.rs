#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod capture;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", test))]
mod audio;
mod bridge;
mod characters;
mod elevenlabs;
mod fonts;
mod history;
mod lightning;
#[cfg(target_os = "linux")]
mod linux_surface;
#[cfg(target_os = "macos")]
mod macos_surface;
mod platform;
mod pixels;
mod private;
mod profile;
mod benchmark;
mod render;
mod state;
mod settings;
mod settings_app;
#[cfg(target_os = "windows")]
mod tts;
mod video;
mod animation;
mod chimera;
mod window;
#[cfg(target_os = "windows")]
mod windows_surface;

use chrono::Timelike;
use platform::{Signal, Speech, SpeechEvent};
use render::{CardPlacement, EntranceScene, PhysicalRect, Renderer};
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
    animation: Option<animation::Animation>,
    history_recorded: bool,
    placement: CardPlacement,
    entrance: Option<EntranceScene>,
    transition_duration: Duration,
    presented_bounds: Option<PhysicalRect>,
}

struct Pending {
    notification: Notification,
    character: characters::ResolvedCharacter,
    requested: Instant,
    readiness_timeout: Duration,
    duration: Duration,
    animation: Option<animation::Animation>,
    placement: CardPlacement,
    entrance: Option<EntranceScene>,
    transition_duration: Duration,
}

enum Presentation {
    Idle,
    Preparing(Pending),
    Playing(Active),
}

impl Presentation {
    fn restore(&self, window: &window::Window) -> Result<(), String> {
        platform::hide(window);
        let placement = match self {
            Self::Playing(active) => active.placement,
            Self::Preparing(pending) => pending.placement,
            Self::Idle => return Ok(()),
        };
        platform::physical_bounds(window, placement.rect, placement.scale)
    }

    fn notification(&self) -> Option<&Notification> {
        match self {
            Self::Idle => None,
            Self::Preparing(pending) => Some(&pending.notification),
            Self::Playing(active) => Some(&active.notification),
        }
    }

    fn take_preparing(&mut self, id: Option<&str>) -> Option<Pending> {
        let matches = match (&*self, id) {
            (Self::Preparing(pending), Some(id)) => pending.notification.id == id,
            (Self::Preparing(_), None) => true,
            _ => false,
        };
        if !matches { return None; }
        match std::mem::replace(self, Self::Idle) {
            Self::Preparing(pending) => Some(pending),
            _ => None,
        }
    }
}

impl Pending {
    fn readiness_timed_out(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.requested) >= self.readiness_timeout
    }

    fn activate(self, silent: bool) -> Active {
        let started = Instant::now();
        Active {
            notification: self.notification,
            character: self.character,
            started,
            expires: started + self.duration,
            end: None,
            speaking: false,
            speech_finished: silent,
            silent,
            animation: self.animation,
            history_recorded: false,
            placement: self.placement,
            entrance: self.entrance,
            transition_duration: self.transition_duration,
            presented_bounds: None,
        }
    }
}

const SPEECH_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Wake interval while Lightning moves: entrance, exit and holding bursts aim for 120 frames per second.
const ANIMATION_FRAME: Duration = Duration::from_micros(8_333);

fn speech_readiness_timeout(settings: &settings::Settings) -> Duration {
    SPEECH_READY_TIMEOUT + Duration::from_secs(u64::from(settings.silent_sound_seconds))
}

impl Active {
    fn entrance_strike(&self, now: Instant) -> Option<Duration> {
        let elapsed = now.saturating_duration_since(self.started);
        (self.end.is_none() && elapsed < self.transition_duration).then_some(elapsed)
    }

    fn seed(&self) -> u32 {
        self.notification.id.bytes().fold(self.notification.completed as u32, |seed, byte| state::noise_hash(seed ^ u32::from(byte)))
    }

    fn ready_to_end(&self, now: Instant) -> bool {
        now >= self.expires && (self.silent || self.speech_finished)
    }

    fn viewport(&self, now: Instant) -> PhysicalRect {
        if self.entrance_strike(now).is_some() {
            self.entrance.as_ref().map(|scene| scene.canvas).unwrap_or(self.placement.rect)
        } else { self.placement.rect }
    }
}

fn main() {
    let bridge_mode = std::env::args().skip(1).any(|argument| argument == "--bridge");
    let benchmark_mode = std::env::args().skip(1).any(|argument| argument == "--benchmark-lightning");
    let result = if bridge_mode { run_bridge() } else { run() };
    if let Err(error) = result {
        if bridge_mode || benchmark_mode {
            eprintln!("{error}");
            std::process::exit(1);
        }
        let data = platform::data_directory();
        let _ = std::fs::create_dir_all(&data);
        state::log(&data, error);
        std::process::exit(1);
    }
}

fn default_assets() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = executable.parent().ok_or("Could not locate the native announcer directory")?;
    Ok(bundled_assets(directory).unwrap_or_else(|| directory.join("../resources")))
}

/// Inside a macOS application bundle the executable lives in `Contents/MacOS` and its assets in `Contents/Resources`.
/// The announcer helper (`Herald.app/Contents/Helpers/Herald Announcer.app`) uses the main application's Resources.
fn bundled_assets(directory: &std::path::Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") || directory.file_name()? != "MacOS" { return None; }
    let contents = directory.parent()?;
    let resources = contents.join("Resources");
    if resources.join("characters.json").is_file() { return Some(resources); }
    let helpers = contents.parent()?.parent()?;
    if helpers.file_name()? != "Helpers" { return None; }
    let resources = helpers.parent()?.join("Resources");
    resources.join("characters.json").is_file().then_some(resources)
}

/// The installed `Herald.app` opens Settings, since Finder cannot pass `--settings`; its announcer helper runs as `Herald Announcer`.
/// `Herald Settings` is the executable name of the earlier separate Settings application.
fn macos_settings_bundle(executable: &std::path::Path) -> bool {
    cfg!(target_os = "macos")
        && executable.file_name().is_some_and(|name| name == "Herald" || name == "Herald Settings")
        && executable.parent().and_then(|path| path.file_name()).is_some_and(|name| name == "MacOS")
}

fn run_bridge() -> Result<(), String> {
    let mut assets = default_assets()?;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bridge" => {}
            "--assets" => assets = arguments.next().ok_or("Missing assets path")?.into(),
            _ => return Err(format!("Unknown bridge option: {argument}")),
        }
    }
    bridge::run(std::io::stdin().lock(), std::io::stdout().lock(), &assets)
}

fn run() -> Result<(), String> {
    let mut assets = default_assets()?;
    let mut demo = None;
    let mut test_seconds = None;
    let mut report = None;
    let mut snapshot = None;
    let mut isolated = false;
    let mut open_settings = macos_settings_bundle(&std::env::current_exe().map_err(|e| e.to_string())?);
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
            "--benchmark-lightning" => {
                let output = PathBuf::from(arguments.next().ok_or("Missing benchmark output path")?);
                return benchmark::run(&assets, &output, arguments.collect());
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
    if open_settings { return settings_app::run(&data, &assets); }
    if isolated && std::env::var_os("HERALD_DATA").is_none() {
        return Err("Isolated playback requires HERALD_DATA".into());
    }
    if capture_directory.is_some() && !isolated {
        return Err("Frame capture requires isolated playback".into());
    }
    if capture_speech_seconds.is_some() && capture_directory.is_none() {
        return Err("Capture speech duration requires frame capture".into());
    }
    let mut frames = capture_directory.as_deref().map(capture::Frames::new).transpose()?;
    let mut frame_log = profile::FrameLog::from_env("HERALD_FRAME_LOG");
    let mut wake_late = 0.0_f64;
    let capture_lightning = capture_directory.as_ref().and_then(|_| std::env::var("HERALD_LIGHTNING_STYLE").ok())
        .map(|value| lightning::PRESETS.iter().find(|preset| preset.id == value).map(|preset| preset.settings).unwrap_or_default());
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
        inbox.queue.push_front(Notification { id: format!("demo-{}", state::timestamp()), session_id: "demo".into(), presence_session_id: String::new(), completed: state::timestamp(), text: "The native voice adviser is ready. Announcements stay visible without taking focus.".into(), title: "Herald verification".into(), character, character_id: None, emotion: "neutral".into() });
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
    let builder = WindowBuilder::new()
            .with_title("Herald")
            .with_visible(false)
            .with_focused(false)
            .with_focusable(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(true)
            .with_inner_size(LogicalSize::new(320.0, 240.0));
    #[cfg(target_os = "linux")]
    let builder = {
        use tao::platform::unix::WindowBuilderExtUnix;
        builder.with_transparent(true).with_transparent_draw(false).with_default_vbox(false)
    };
    #[cfg(target_os = "macos")]
    let builder = builder.with_transparent(true);
    let window = Rc::new(
        builder
            .build(&event_loop)
            .map_err(|e| e.to_string())?,
    );
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::WidgetExt;
        use tao::platform::unix::WindowExtUnix;
        window.set_skip_taskbar(true).map_err(|e| e.to_string())?;
        window.gtk_window().realize();
    }
    #[cfg(target_os = "windows")]
    let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    let mut surface =
        softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    let mut alpha_surface = windows_surface::Surface::new(window.hwnd() as *mut _);
    #[cfg(target_os = "macos")]
    let mut surface = macos_surface::Surface::new(&window)?;
    #[cfg(target_os = "linux")]
    let mut surface = linux_surface::Surface::new(window.clone());
    #[cfg(target_os = "linux")]
    let mut wake_timer = {
        let proxy = event_loop.create_proxy();
        Some(gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
            if proxy.send_event(()).is_ok() { gtk::glib::ControlFlow::Continue }
            else { gtk::glib::ControlFlow::Break }
        }))
    };
    let mut renderer = Renderer::with_settings(&settings_store.current)?;
    let local = chrono::Local::now();
    let mut speech = Speech::new(&settings_store.current, local.hour() * 60 + local.minute(), platform::meeting_override(&data));
    let mut signal = Signal::new(&data);
    let meeting = Arc::new(MeetingStatus::new());
    let stop = Arc::new(AtomicBool::new(false));
    platform::detect_meetings(
        data.clone(),
        meeting.clone(),
        stop.clone(),
    );
    let mut current = Presentation::Idle;
    let mut warmups = std::collections::HashMap::<String, (state::Notification, Arc<platform::Warmup>)>::new();
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
    let mut procedural_frames = 0;
    let mut max_played_samples = 0;
    let mut pre_speech_played_samples = 0;
    let mut selected_visuals = Vec::new();
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
                speech.cancel(); signal.stop();
                for (_, warmup) in warmups.values() { warmup.cancel(); }
                warmups.clear();
                if let Err(error) = current.restore(&window) { state::log(&data, error); *control_flow = ControlFlow::Exit; }
                current = Presentation::Idle; inbox.save(None);
            }
            Event::MainEventsCleared => {
                platform::refresh_card_input(&window);
                let now = Instant::now();
                if now < next_frame { return; }
                wake_late = now.saturating_duration_since(next_frame).as_secs_f64() * 1000.0;
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
                            for (_, warmup) in warmups.values() { warmup.cancel(); }
                            warmups.clear();
                            signal.stop();
                            if matches!(&current, Presentation::Idle) {
                                renderer.set_preferences(&settings_store.current.announcement_body_font, &settings_store.current.announcement_title_font);
                            }
                        }
                        Err(error) => state::log(&data, format!("Settings: {error}")),
                        _ => {}
                    }
                    let invalidated = {
                        let notification = current.notification().filter(|n| !demo_mode || n.session_id != "demo");
                        inbox.read(notification)
                    };
                    if invalidated || matches!(&current, Presentation::Preparing(pending) if inbox.is_discarded(&pending.notification)) {
                        speech.cancel(); signal.stop();
                        if let Err(error) = current.restore(&window) { state::log(&data, error); *control_flow = ControlFlow::Exit; return; }
                        current = Presentation::Idle; inbox.save(None);
                    }
                    last_inbox = now;
                }
                let settings = &settings_store.current;
                let local = chrono::Local::now();
                let muted = meeting.muted(now) || platform::meeting_override(&data) || settings.quiet_at(local.hour() * 60 + local.minute()) || settings.volume == 0;
                warmups.retain(|_, (notification, warmup)| {
                    let keep = !muted && inbox.preparation_valid(notification, state::timestamp());
                    if !keep { warmup.cancel(); }
                    keep
                });
                for notification in inbox.preparations.drain(..).collect::<Vec<_>>() {
                    if !muted && inbox.preparation_valid(&notification, state::timestamp()) && !warmups.contains_key(&notification.id) && warmups.len() < 128 {
                        if let Some(warmup) = platform::Warmup::start(settings) {
                            warmups.insert(notification.id.clone(), (notification, warmup));
                        }
                    }
                }
                let mut activation = None;
                if muted {
                    if let Some(pending) = current.take_preparing(None) {
                        speech.cancel();
                        activation = Some((pending, true));
                    }
                }
                for event in speech.events.try_iter() {
                    match event {
                        SpeechEvent::Ready { id, animation } => {
                            state::timing(format!("speech_ready_received id={id}"));
                            if let Some(mut pending) = current.take_preparing(Some(&id)) {
                                if let (Some(visual), Some(playback)) = (&mut pending.animation, animation) { visual.attach(playback); }
                                if muted { speech.cancel(); }
                                activation = Some((pending, muted));
                            }
                        }
                        SpeechEvent::Finished { id, result } => {
                            if let Some((_, warmup)) = warmups.remove(&id) { warmup.cancel(); }
                            if let Some(pending) = current.take_preparing(Some(&id)) {
                                if let Err(error) = result { state::log(&data, format!("Speech: {error}")); }
                                speech.cancel();
                                activation = Some((pending, true));
                            } else if let Presentation::Playing(active) = &mut current {
                                if active.notification.id == id {
                                    active.speech_finished = true;
                                    if let Err(error) = result { state::log(&data, format!("Speech: {error}")); }
                                }
                            }
                        }
                    }
                }
                let timed_out = matches!(&current, Presentation::Preparing(pending) if pending.readiness_timed_out(now));
                if timed_out {
                    if let Some(pending) = current.take_preparing(None) {
                        state::log(&data, format!("Speech readiness timed out for {}", pending.notification.id));
                        speech.cancel();
                        activation = Some((pending, true));
                    }
                }
                if activation.is_none() && matches!(current, Presentation::Idle) && meeting.ready() {
                    renderer.set_preferences(&settings.announcement_body_font, &settings.announcement_title_font);
                    let notification = if demo_mode && inbox.queue.front().is_some_and(|n| n.session_id == "demo") { inbox.queue.pop_front() } else { inbox.next(state::timestamp()) };
                    if let Some(notification) = notification {
                        state::timing(format!("notification_selected id={} completed={}", notification.id, notification.completed));
                        let mut character = characters::resolve(settings, &assets, notification.character(), notification.character_id.as_deref());
                        let style = match character.visual { characters::ResolvedVisual::Chimera => render::AnnouncementStyle::Chimera, characters::ResolvedVisual::Video(_) => render::AnnouncementStyle::Video };
                        renderer.set_style(style);
                        let lightning = capture_lightning.unwrap_or(settings.lightning);
                        renderer.set_lightning(lightning);
                        renderer.text = render::display_text(&notification.text);
                        let title = render::display_text(&notification.title);
                        renderer.title = if title.is_empty() { "Untitled session".into() } else { title };
                        let monitor = window.current_monitor().or_else(|| window.primary_monitor());
                        let scale = monitor.as_ref().map_or_else(|| window.scale_factor(), |monitor| style.fit_scale(monitor.size().width, monitor.size().height, monitor.scale_factor()));
                        let max_height = monitor.as_ref().map(|m| {
                            let logical_height = m.size().height as f64 / scale;
                            match style { render::AnnouncementStyle::Chimera => (logical_height - 80.0) as u32, render::AnnouncementStyle::Video => (logical_height * 0.8) as u32 }
                        }).unwrap_or(700);
                        let layout = renderer.announcement_layout(scale as f32, max_height);
                        let height = layout.height;
                        let position = monitor.as_ref().map(|monitor| {
                            let position = monitor.position();
                            PhysicalPosition::new(position.x + monitor.size().width as i32 - ((layout.width + 16) as f64 * scale) as i32, position.y + monitor.size().height as i32 - ((height + 64) as f64 * scale) as i32)
                        }).unwrap_or_else(|| PhysicalPosition::new(0, 0));
                        let placement = CardPlacement { style, rect: PhysicalRect { x: position.x, y: position.y,
                            width: (f64::from(layout.width) * scale) as u32, height: (height as f64 * scale) as u32 }, scale: scale as f32 };
                        if let Err(error) = platform::physical_bounds(&window, placement.rect, placement.scale) {
                            state::log(&data, error); *control_flow = ControlFlow::Exit; return;
                        }
                        #[cfg(target_os = "windows")]
                        let entrance = monitor.as_ref().filter(|monitor| {
                            let bounds = monitor.bounds();
                            bounds.contains(placement.rect.x, placement.rect.y)
                                && bounds.contains(placement.rect.right() - 1, placement.rect.bottom() - 1)
                        }).map(|monitor| {
                            let seed = notification.id.bytes().fold(notification.completed as u32, |seed, byte| state::noise_hash(seed ^ u32::from(byte)));
                            EntranceScene::new(monitor.bounds(), placement, seed, lightning)
                        });
                        #[cfg(not(target_os = "windows"))]
                        let entrance = None;
                        if let Some(warning) = &character.video_warning { state::log(&data, warning); }
                        state::timing(format!("video_load_start id={}", notification.id));
                        let mut animation = match animation::Animation::open(&character.visual) {
                            Ok(animation) => Some(animation),
                            Err(error) => {
                                state::log(&data, format!("Video: {error}"));
                                let default_video = video::select_path(&assets, notification.character());
                                if character.id.is_some() && character.visual.video_path().is_some_and(|path| default_video != path) {
                                    match video::Video::open(&default_video) {
                                        Ok(video) => {
                                            character.visual = characters::ResolvedVisual::Video(default_video);
                                            Some(animation::Animation::Video(video))
                                        }
                                        Err(error) => { state::log(&data, format!("Default video: {error}")); None }
                                    }
                                } else { None }
                            }
                        };
                        state::timing(format!("video_load_end id={}", notification.id));
                        let transition_duration = lightning.transition_duration();
                        let mut duration = state::display_duration(&notification.text).max(capture_speech_seconds.map(|speech| speech + transition_duration).unwrap_or_default());
                        if muted && capture_speech_seconds.is_some() {
                            if let Some(animation::Animation::Chimera(head)) = &mut animation {
                                let utterance = match chimera::prepare(&notification.text, &stop) {
                                    Ok(utterance) => utterance,
                                    Err(error) => { state::log(&data, error); *control_flow = ControlFlow::Exit; return; }
                                };
                                duration = duration.max(Duration::from_secs_f64(utterance.samples.len() as f64 / f64::from(utterance.sample_rate())) + transition_duration);
                                head.attach_capture(utterance, transition_duration);
                            }
                        }
                        let readiness_timeout = speech_readiness_timeout(settings);
                        let pending = Pending { notification, character, requested: Instant::now(), readiness_timeout, duration, animation, placement, entrance, transition_duration };
                        if muted {
                            activation = Some((pending, true));
                        } else {
                            let warmup = warmups.get(&pending.notification.id).map(|(_, warmup)| warmup.clone());
                            speech.start_prepared(&pending.notification.text, &pending.notification.id, &pending.character, settings, warmup);
                            speech_started += 1;
                            current = Presentation::Preparing(pending);
                        }
                    }
                }
                if let Some((pending, silent)) = activation {
                    let active = pending.activate(silent);
                    abrupt_window_ok &= platform::opacity(&window, 1.0);
                    window_opacity_updates += 1;
                    #[cfg(not(target_os = "windows"))]
                    platform::show(&window);
                    window.request_redraw();
                    if let Some(log) = &mut frame_log { log.reset(); }
                    shown += 1;
                    titles.push(renderer.title.clone());
                    if active.silent { muted_announcements += 1; }
                    selected_visuals.push(match &active.character.visual { characters::ResolvedVisual::Chimera => "chimera", characters::ResolvedVisual::Video(_) => "video" });
                    if let (Some(animation), Some(path)) = (&active.animation, active.character.visual.video_path()) {
                        selected_videos.push(path.to_string_lossy().into_owned());
                        video_frame_rates.push(animation.fps());
                    }
                    if !active.silent { signal.play(settings); }
                    inbox.save(Some(&active.notification));
                    current = Presentation::Playing(active);
                }
                let mut dismiss = false;
                if let Presentation::Playing(active) = &mut current {
                    if muted && !active.silent { active.silent = true; speech.cancel(); signal.stop(); }
                    if now.saturating_duration_since(active.started) >= active.transition_duration && !active.speaking {
                        active.speaking = true;
                        signal.stop();
                        if !active.silent {
                            state::timing(format!("speech_release id={}", active.notification.id));
                            speech.release(&active.notification.id);
                        }
                    }
                    if active.end.is_none() && active.ready_to_end(now) {
                        active.end = Some(now); speech.cancel();
                        if !active.silent && !muted { signal.play(settings); }
                    }
                    if active.end.is_some_and(|end| now.duration_since(end) >= active.transition_duration) {
                        max_visible = max_visible.max(active.started.elapsed().as_secs_f64());
                        durations.push(active.started.elapsed().as_secs_f64());
                        dismiss = true;
                    } else { window.request_redraw(); }
                }
                if dismiss {
                    if let Err(error) = current.restore(&window) { state::log(&data, error); *control_flow = ControlFlow::Exit; return; }
                    current = Presentation::Idle; signal.stop(); finished += 1; inbox.save(None);
                    passive_window_ok &= platform::passive_window(&window, false);
                }
                let interval = match &current {
                    Presentation::Preparing(_) => Duration::from_millis(42),
                    Presentation::Playing(active) if active.started.elapsed() < active.transition_duration => ANIMATION_FRAME,
                    Presentation::Playing(active) if active.end.is_some() => ANIMATION_FRAME,
                    Presentation::Playing(active) if renderer.ambient_active(active.started.elapsed(), active.expires.duration_since(active.started), active.seed()) => ANIMATION_FRAME,
                    Presentation::Playing(active) => Duration::from_millis(((1000.0 / active.animation.as_ref().map(|animation| animation.fps()).unwrap_or(state::VIDEO_FPS as f64)) as u64).min(33)),
                    Presentation::Idle => Duration::from_millis(250),
                };
                next_frame = now + interval;
                *control_flow = ControlFlow::WaitUntil(next_frame);
            }
            Event::RedrawRequested(_) => {
                if let Presentation::Playing(active) = &mut current {
                    let frame_now = Instant::now();
                    let elapsed = frame_now.saturating_duration_since(active.started);
                    let entrance_time = active.entrance_strike(frame_now);
                    let viewport = active.viewport(frame_now);
                    if let (Some(width), Some(height)) = (NonZeroU32::new(viewport.width), NonZeroU32::new(viewport.height)) {
                        let result = (|| -> Result<(), String> {
                            let transition_time = active.end.map(|end| frame_now.saturating_duration_since(end)).unwrap_or(elapsed);
                            let interference = state::visual_interference_amount(transition_time, active.notification.completed as u32);
                            if interference > 0.0 { static_frames += 1; } else { animation_frames += 1; }
                            if let Some(animation) = &mut active.animation {
                                let previous_frames = animation.decoded_frames();
                                let previous_loops = animation.loops();
                                let previous_procedural = animation.procedural_frames();
                                if let Err(error) = animation.advance(elapsed) {
                                    state::log(&data, format!("Animation playback: {error}"));
                                }
                                decoded_video_frames += animation.decoded_frames() - previous_frames;
                                video_loops += animation.loops() - previous_loops;
                                procedural_frames += animation.procedural_frames() - previous_procedural;
                                max_played_samples = max_played_samples.max(animation.sample_position());
                                if !active.speaking { pre_speech_played_samples = pre_speech_played_samples.max(animation.sample_position()); }
                            }
                            renderer.set_anatomy_paths(active.animation.as_ref().map_or(&[], animation::Animation::anatomy_paths));
                            renderer.set_eye_rays(active.animation.as_ref().map_or(&[], animation::Animation::eye_rays));
                            renderer.set_mouth_discharge(active.animation.as_ref().and_then(animation::Animation::mouth_discharge));
                            renderer.text_interference = if elapsed < state::TRANSITION_DURATION || active.end.is_some() { interference } else { 0.0 };
                            let seed = active.seed();
                            renderer.lightning_activity = if active.end.is_some() {
                                Some(render::LightningActivity::Closing(transition_time, seed))
                            } else if entrance_time.is_none() {
                                Some(render::LightningActivity::Holding(elapsed, active.expires.duration_since(active.started), seed))
                            } else { None };
                            let image = active.animation.as_ref().map(animation::Animation::frame);
                            let pixels = if let Some(scene) = &active.entrance {
                                renderer.draw_scene(scene, image, interference, entrance_time)
                            } else {
                                let mut pixels = vec![0; viewport.width as usize * viewport.height as usize];
                                renderer.draw(&mut pixels, viewport.width as usize, viewport.height as usize, active.placement.scale, image, interference, entrance_time);
                                pixels
                            };
                            let changed_bounds = active.presented_bounds != Some(viewport);
                            if changed_bounds {
                                platform::hide(&window);
                                platform::physical_bounds(&window, viewport, active.placement.scale)?;
                                platform::card_region(&window, active.placement.rect);
                            }
                            profile::time(profile::Stage::Present, || -> Result<(), String> {
                                #[cfg(target_os = "windows")]
                                if active.placement.style == render::AnnouncementStyle::Chimera {
                                    return alpha_surface.present(width.get(), height.get(), &pixels);
                                }
                                surface.resize(width, height).map_err(|e| e.to_string())?;
                                let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
                                buffer.copy_from_slice(&pixels);
                                #[cfg(target_os = "windows")]
                                let result = buffer.present();
                                #[cfg(any(target_os = "macos", target_os = "linux"))]
                                let result = buffer.present(active.placement.style.pixel_format());
                                result.map_err(|e| e.to_string())
                            })?;
                            if let Some(log) = &mut frame_log {
                                let phase = render::announcement_phase(transition_time, active.end.is_some());
                                log.record("announcement", frame_now, &format!("{phase:?}").to_lowercase(), wake_late);
                            }
                            if changed_bounds {
                                let previous_focus = platform::foreground();
                                platform::show(&window);
                                if previous_focus != 0 && platform::foreground() != previous_focus { focus_unchanged = false; }
                                passive_window_ok &= platform::passive_window(&window, true);
                                active.presented_bounds = Some(viewport);
                            }
                            if let Some(frames) = &mut frames {
                                let speech_sample = active.animation.as_ref().and_then(|animation| match animation {
                                    animation::Animation::Chimera(_) => Some(animation.sample_position()),
                                    animation::Animation::Video(_) => None,
                                });
                                frames.save_scene(&pixels, viewport.width, viewport.height, elapsed, active.end.map(|end| end.duration_since(active.started)), active.placement, active.entrance.as_ref(), viewport, speech_sample)?;
                            }
                            if elapsed >= active.transition_duration && active.end.is_none() {
                                if let Some(path) = snapshot.take() {
                                    let preview = image::RgbaImage::from_fn(viewport.width, viewport.height, |x, y| {
                                        let color = pixels[(y * viewport.width + x) as usize];
                                        active.placement.style.pixel_format().rgba(color)
                                    });
                                    preview.save(path).map_err(|e| e.to_string())?;
                                }
                            }
                            if !active.history_recorded {
                                state::timing(format!("visual_presented id={}", active.notification.id));
                                let result = if active.character.id.is_some() {
                                    history::record_with_identity(&data, &active.notification, active.character.history_identity(), active.character.visual.video_path(), chrono::Utc::now())
                                } else {
                                    history::record(&data, &active.notification, active.character.visual.video_path().expect("Default character has a video"), chrono::Utc::now())
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
                for (_, warmup) in warmups.values() { warmup.cancel(); }
                if let Err(error) = current.restore(&window) { state::log(&data, error); }
                stop.store(true, Ordering::Relaxed); speech.cancel(); signal.stop();
                if let Presentation::Playing(active) = &current { max_visible = max_visible.max(active.started.elapsed().as_secs_f64()); }
                if let Some(frames) = &mut frames {
                    if let Err(error) = frames.finish() { state::log(&data, error); }
                }
                #[cfg(target_os = "linux")]
                if let Some(timer) = wake_timer.take() { timer.remove(); }
                inbox.save(current.notification());
                if let Some(path) = &report {
                    let report = serde_json::json!({"focusUnchanged": focus_unchanged, "focusChecked": before != 0, "externalFocusChanged": external_focus_changed, "passiveWindow": passive_window_ok, "windowOpacityUpdates": window_opacity_updates, "abruptWindowSucceeded": abrupt_window_ok, "shown": shown, "finished": finished, "visibleSeconds": max_visible, "durations": durations, "sessionTitles": titles, "speechStarted": speech_started, "mutedAnnouncements": muted_announcements, "staticFrames": static_frames, "animationFrames": animation_frames, "videoFPS": video_frame_rates.first().copied().unwrap_or(state::VIDEO_FPS as f64), "selectedVideos": selected_videos, "videoFrameRates": video_frame_rates, "decodedVideoFrames": decoded_video_frames, "proceduralFrames": procedural_frames, "maxPlayedSamples": max_played_samples, "preSpeechPlayedSamples": pre_speech_played_samples, "selectedVisuals": selected_visuals, "videoLoops": video_loops, "decodedFrameLimit": 1});
                    let _ = std::fs::write(path, report.to_string());
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
#[path = "../unit_tests/main.rs"]
mod tests;
