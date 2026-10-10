use crate::lightning::LightningSettings;
use crate::render::{announcement_phase, AnnouncementPhase, CardPlacement, EntranceScene, LightningActivity, PhysicalRect, Renderer};
use crate::settings::Settings;
use crate::state::TRANSITION_DURATION;
use crate::video::Video;
use gpui_kit::{RenderImage, Task, Window};
use image::{Frame as ImageFrame, RgbaImage};
use rand::Rng;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MESSAGE: &str = "This is an announcement.";

#[derive(Clone, Copy)]
struct Intent {
    lightning: LightningSettings,
    replay: u64,
    revision: u64,
    active: bool,
}

struct Shared {
    intent: Intent,
    frame: Option<(u64, Result<Frame, String>)>,
    stop: bool,
}

struct Frame {
    image: RgbaImage,
    character_id: String,
    character_name: String,
    phase: AnnouncementPhase,
}

pub(super) struct Preview {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    pub(super) driver: Option<Task<()>>,
    pub(super) image: Option<Arc<RenderImage>>,
    retired: Option<Arc<RenderImage>>,
    pub(super) character_id: String,
    pub(super) character_name: String,
    pub(super) phase: AnnouncementPhase,
    pub(super) error: Option<String>,
    pub(super) size: (f32, f32),
    scale: f32,
}

impl Preview {
    pub(super) fn start(assets: PathBuf, settings: Settings, active: bool, scale: f32) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(Shared {
            intent: Intent { lightning: settings.lightning, replay: 0, revision: 0, active },
            frame: None,
            stop: false,
        }), Condvar::new()));
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new().name("herald-lightning-preview".into())
            .spawn(move || run(worker_shared, assets, settings, scale)).map_err(|error| error.to_string())?;
        Ok(Self { shared, worker: Some(worker), driver: None, image: None, retired: None,
            character_id: String::new(), character_name: String::new(), phase: AnnouncementPhase::Leader, error: None,
            size: (352.0, 336.0), scale })
    }

    pub(super) fn update(&self, lightning: LightningSettings) {
        let (lock, wake) = &*self.shared;
        let mut shared = lock.lock().unwrap();
        if shared.intent.lightning == lightning { return; }
        shared.intent.lightning = lightning;
        shared.intent.revision += 1;
        shared.frame = None;
        wake.notify_one();
    }

    pub(super) fn replay(&self) {
        let (lock, wake) = &*self.shared;
        let mut shared = lock.lock().unwrap();
        shared.intent.replay += 1;
        shared.intent.revision += 1;
        shared.frame = None;
        wake.notify_one();
    }

    pub(super) fn set_active(&self, active: bool) {
        let (lock, wake) = &*self.shared;
        let mut shared = lock.lock().unwrap();
        if shared.intent.active == active { return; }
        shared.intent.active = active;
        wake.notify_one();
    }

    pub(super) fn receive(&mut self) -> bool {
        if self.retired.is_some() { return false; }
        let result = {
            let mut shared = self.shared.0.lock().unwrap();
            shared.frame.take().filter(|(revision, _)| *revision == shared.intent.revision)
        };
        let Some((_, result)) = result else { return false; };
        match result {
            Ok(frame) => {
                self.size = (frame.image.width() as f32 / self.scale, frame.image.height() as f32 / self.scale);
                let image = Arc::new(RenderImage::new(vec![ImageFrame::new(frame.image)]));
                self.retired = self.image.replace(image);
                self.character_id = frame.character_id;
                self.character_name = frame.character_name;
                self.phase = frame.phase;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        true
    }

    pub(super) fn retire_painted(&mut self, window: &mut Window) {
        if let Some(image) = self.retired.take() { let _ = window.drop_image(image); }
    }

    pub(super) fn clear_images(&mut self, window: &mut Window) {
        self.retire_painted(window);
        if let Some(image) = self.image.take() { let _ = window.drop_image(image); }
    }

    pub(super) fn leave(mut self, window: &mut Window) {
        self.driver = None;
        let images = [self.image.take(), self.retired.take()];
        window.on_next_frame(move |window, _| {
            for image in images.into_iter().flatten() { let _ = window.drop_image(image); }
        });
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.driver = None;
        let (lock, wake) = &*self.shared;
        {
            let mut shared = lock.lock().unwrap();
            shared.stop = true;
            shared.frame = None;
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

struct Playback {
    renderer: Renderer,
    scene: EntranceScene,
    stage: PhysicalRect,
    video: Video,
    character_id: String,
    character_name: String,
    seed: u32,
    effect_elapsed: Duration,
    video_elapsed: Duration,
    duration: Duration,
}

fn bundled_video(assets: &Path, previous: &str) -> Result<(String, String, PathBuf), String> {
    let pool = crate::characters::bundled_characters().iter().filter_map(|(id, character)| {
        let path = assets.join(character.animation_path.as_ref()?);
        path.is_file().then(|| (id.clone(), character.name.clone(), path))
    }).collect::<Vec<_>>();
    let choices = pool.iter().filter(|(id, _, _)| pool.len() == 1 || id != previous).collect::<Vec<_>>();
    if choices.is_empty() { return Err("No bundled character video is available.".into()); }
    Ok(choices[rand::thread_rng().gen_range(0..choices.len())].clone())
}

impl Playback {
    fn new(assets: &Path, settings: &Settings, previous: &str, scale: f32) -> Result<Self, String> {
        let (character_id, character_name, path) = bundled_video(assets, previous)?;
        let mut video = Video::open(&path)?;
        video.advance(Duration::ZERO)?;
        let mut renderer = Renderer::with_settings(settings)?;
        renderer.text = MESSAGE.into();
        renderer.title = "Lightning preview".into();
        let height = renderer.announcement_height(scale, 700);
        let stage = PhysicalRect { x: 0, y: 0, width: (352.0 * scale) as u32, height: ((height + 80) as f32 * scale) as u32 };
        let card = CardPlacement { rect: PhysicalRect { x: (16.0 * scale) as i32, y: (16.0 * scale) as i32,
            width: (320.0 * scale) as u32, height: (height as f32 * scale) as u32 }, scale };
        let seed = rand::thread_rng().gen();
        let scene = EntranceScene::new(stage, card, seed, settings.lightning);
        Ok(Self { renderer, scene, stage, video, character_id, character_name, seed,
            effect_elapsed: Duration::ZERO, video_elapsed: Duration::ZERO, duration: crate::state::display_duration(MESSAGE) })
    }

    fn update(&mut self, lightning: LightningSettings) {
        self.renderer.set_lightning(lightning);
        self.scene = EntranceScene::new(self.stage, self.scene.card, self.seed, lightning);
        self.effect_elapsed = Duration::ZERO;
    }

    fn frame(&mut self, delta: Duration) -> Frame {
        self.video_elapsed += delta;
        self.effect_elapsed += delta;
        let cycle = self.duration + TRANSITION_DURATION;
        while self.effect_elapsed >= cycle { self.effect_elapsed -= cycle; }
        let closing = self.effect_elapsed >= self.duration;
        let transition = if closing { self.effect_elapsed - self.duration } else { self.effect_elapsed };
        let entrance = (!closing && self.effect_elapsed < TRANSITION_DURATION).then_some(self.effect_elapsed);
        let _ = self.video.advance(self.video_elapsed);
        let interference = crate::state::visual_interference_amount(transition, self.seed);
        self.renderer.text_interference = if closing || entrance.is_some() { interference } else { 0.0 };
        self.renderer.lightning_activity = if closing { Some(LightningActivity::Closing(transition, self.seed)) }
            else if entrance.is_none() { Some(LightningActivity::Holding(self.effect_elapsed, self.duration, self.seed)) }
            else { None };
        let pixels = self.renderer.draw_scene(&self.scene, Some(self.video.frame()), interference, entrance);
        let origin = if entrance.is_some() { self.scene.canvas } else { self.scene.card.rect };
        Frame { image: compose(&pixels, origin, self.stage), character_id: self.character_id.clone(),
            character_name: self.character_name.clone(), phase: announcement_phase(transition, closing) }
    }
}

fn compose(pixels: &[u32], source: PhysicalRect, stage: PhysicalRect) -> RgbaImage {
    RgbaImage::from_fn(stage.width, stage.height, |x, y| {
        let x = x as i32 + stage.x - source.x;
        let y = y as i32 + stage.y - source.y;
        if x < 0 || y < 0 || x >= source.width as i32 || y >= source.height as i32 { return image::Rgba([0, 0, 0, 0]); }
        let color = pixels[y as usize * source.width as usize + x as usize];
        if color == 0xff00ff { image::Rgba([0, 0, 0, 0]) }
        else { image::Rgba([color as u8, (color >> 8) as u8, (color >> 16) as u8, 255]) }
    })
}

fn delta_after_intent_change(delta: Duration, replay_changed: bool) -> Duration {
    if replay_changed { Duration::ZERO } else { delta }
}

fn run(shared: Arc<(Mutex<Shared>, Condvar)>, assets: PathBuf, mut settings: Settings, scale: f32) {
    let mut playback: Option<Playback> = None;
    let mut current: Option<Intent> = None;
    let mut failure = "Preview video is unavailable. Try Replay.".to_owned();
    let mut tick = Instant::now();
    loop {
        let intent = {
            let (lock, wake) = &*shared;
            let mut state = lock.lock().unwrap();
            while !state.stop && !state.intent.active {
                state = wake.wait(state).unwrap();
                tick = Instant::now();
            }
            if state.stop { break; }
            state.intent
        };
        let now = Instant::now();
        let mut delta = now.saturating_duration_since(tick);
        tick = now;
        let changed = current.is_none_or(|current| current.revision != intent.revision);
        let result = if changed {
            settings.lightning = intent.lightning;
            let replay_changed = current.is_none_or(|current| current.replay != intent.replay);
            let result = if replay_changed {
                let previous = playback.as_ref().map(|playback| playback.character_id.as_str()).unwrap_or("");
                match Playback::new(&assets, &settings, previous, scale) {
                    Ok(next) => { playback = Some(next); Ok(()) }
                    Err(error) => { playback = None; failure = error.clone(); Err(error) }
                }
            } else {
                if let Some(playback) = &mut playback { playback.update(intent.lightning); }
                Ok(())
            };
            delta = delta_after_intent_change(delta, replay_changed);
            result
        } else { Ok(()) };
        current = Some(intent);
        let frame = result.and_then(|_| playback.as_mut().map(|playback| playback.frame(delta)).ok_or_else(|| failure.clone()));
        let (lock, wake) = &*shared;
        let mut state = lock.lock().unwrap();
        if state.stop { break; }
        if state.intent.active && state.intent.revision == intent.revision { state.frame = Some((intent.revision, frame)); }
        if state.intent.revision == intent.revision {
            let _ = wake.wait_timeout(state, Duration::from_millis(16)).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_conversion_clears_the_sentinel_and_keeps_scene_and_card_origins() {
        let stage = PhysicalRect { x: -10, y: 3, width: 8, height: 6 };
        let card = PhysicalRect { x: -8, y: 5, width: 2, height: 2 };
        let colors = [0x123456, 0xff00ff, 0xabcdef, 0x010203];
        let image = compose(&colors, card, stage);
        assert_eq!(image.get_pixel(2, 2).0, [0x56, 0x34, 0x12, 255]);
        assert_eq!(image.get_pixel(3, 2).0, [0, 0, 0, 0]);
        assert_eq!(image.get_pixel(2, 3).0, [0xef, 0xcd, 0xab, 255]);
        assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 0]);
        let scene = PhysicalRect { x: -9, y: 4, width: 6, height: 5 };
        let mut pixels = vec![0xff00ff; 30];
        for row in 0..2 { pixels[(row + 1) * 6 + 1..(row + 1) * 6 + 3].copy_from_slice(&colors[row * 2..row * 2 + 2]); }
        assert_eq!(compose(&pixels, scene, stage), image);
    }

    #[test]
    fn preview_matches_announcement_dimensions_padding_and_pixels_at_display_scales() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        for scale in [1.0, 1.5, 2.0] {
            let settings = Settings::default();
            let mut playback = Playback::new(&assets, &settings, "", scale).unwrap();
            let card = playback.scene.card;
            assert_eq!(card.scale, scale);
            assert_eq!(card.rect.width, (320.0 * scale) as u32);
            assert_eq!(card.rect.height, (playback.renderer.announcement_height(scale, 700) as f32 * scale) as u32);
            assert_eq!(playback.stage.right() - card.rect.right(), (16.0 * scale) as i32);
            assert_eq!(playback.stage.bottom() - card.rect.bottom(), (64.0 * scale) as i32);
            let mut renderer = Renderer::with_settings(&settings).unwrap();
            renderer.text = MESSAGE.into();
            renderer.title = "Lightning preview".into();
            let mut expected = vec![0; card.rect.width as usize * card.rect.height as usize];
            renderer.draw(&mut expected, card.rect.width as usize, card.rect.height as usize, scale, Some(playback.video.frame()), 0.0, None);
            let pixels = playback.renderer.draw_scene(&playback.scene, Some(playback.video.frame()), 0.0, None);
            assert_eq!(pixels, expected);
            let image = compose(&pixels, card.rect, playback.stage);
            for y in 0..card.rect.height {
                for x in 0..card.rect.width {
                    let color = expected[(y * card.rect.width + x) as usize];
                    let expected = if color == 0xff00ff { [0, 0, 0, 0] }
                        else { [color as u8, (color >> 8) as u8, (color >> 16) as u8, 255] };
                    assert_eq!(image.get_pixel(x + card.rect.x as u32, y + card.rect.y as u32).0, expected);
                }
            }
        }
    }

    #[test]
    fn preview_card_backdrop_outside_the_bubble_and_portrait_is_transparent() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        for scale in [1.0, 2.0] {
            let mut playback = Playback::new(&assets, &Settings::default(), "", scale).unwrap();
            let card = playback.scene.card.rect;
            let pixels = playback.renderer.draw_scene(&playback.scene, Some(playback.video.frame()), 0.0, None);
            let image = compose(&pixels, card, playback.stage);
            for (x, y) in [(card.x + 1, card.y + 1), (card.x + 1, card.bottom() - 2)] {
                assert_eq!(image.get_pixel(x as u32, y as u32).0[3], 0, "Opaque backdrop at ({x}, {y}) at scale {scale}");
            }
        }
    }

    #[test]
    fn bundled_selection_excludes_custom_files_and_replay_avoids_the_previous_character() {
        let root = std::env::temp_dir().join("opencode").join(format!("herald-lightning-pool-{}-{}", std::process::id(), crate::state::timestamp()));
        std::fs::create_dir_all(root.join("videos")).unwrap();
        std::fs::write(root.join("videos/custom.mp4"), []).unwrap();
        assert!(bundled_video(&root, "").is_err());
        let characters = crate::characters::bundled_characters().iter().take(2).collect::<Vec<_>>();
        for (_, character) in &characters { std::fs::write(root.join(character.animation_path.as_ref().unwrap()), []).unwrap(); }
        let previous = characters[0].0;
        for _ in 0..20 { assert_eq!(&bundled_video(&root, previous).unwrap().0, characters[1].0); }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edits_rebuild_geometry_without_resetting_video_identity_elapsed_or_seed() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let mut playback = Playback::new(&assets, &Settings::default(), "", 1.0).unwrap();
        playback.frame(Duration::from_millis(900));
        let identity = playback.character_id.clone();
        let elapsed = playback.video_elapsed;
        let decoded = playback.video.decoded_frames;
        let seed = playback.seed;
        let settings = crate::lightning::PRESETS[1].settings;
        playback.update(settings);
        assert_eq!(playback.character_id, identity);
        assert_eq!(playback.video_elapsed, elapsed);
        assert_eq!(playback.video.decoded_frames, decoded);
        assert_eq!(playback.seed, seed);
        assert_eq!(playback.effect_elapsed, Duration::ZERO);
        let frame = playback.frame(Duration::from_millis(140));
        assert_eq!(frame.phase, AnnouncementPhase::Impact);
        playback.effect_elapsed = playback.duration + TRANSITION_DURATION - Duration::from_millis(1);
        assert_eq!(playback.frame(Duration::from_millis(2)).phase, AnnouncementPhase::Leader);
        assert_eq!(playback.character_id, identity);
    }

    #[test]
    fn worker_resets_effect_delta_only_for_replay() {
        let delta = Duration::from_millis(37);
        assert_eq!(delta_after_intent_change(delta, false), delta);
        assert_eq!(delta_after_intent_change(delta, true), Duration::ZERO);
    }

    #[test]
    fn worker_coalesces_edits_in_one_slot_and_joins_on_drop_even_while_inactive() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let preview = Preview::start(assets, Settings::default(), false, 1.0).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert!(preview.shared.0.lock().unwrap().frame.is_none());
        for index in 0..20 {
            preview.update(LightningSettings { brightness: 1.0 + index as f32 * 0.01, ..LightningSettings::default() });
        }
        preview.set_active(true);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ready = {
                let shared = preview.shared.0.lock().unwrap();
                shared.frame.as_ref().is_some_and(|(revision, frame)| {
                    assert_eq!(*revision, shared.intent.revision);
                    assert_eq!(frame.as_ref().unwrap().image.width(), 352);
                    assert!(frame.as_ref().unwrap().image.height() >= 320);
                    true
                })
            };
            if ready { break; }
            assert!(Instant::now() < deadline, "The worker did not publish the latest intent");
            std::thread::sleep(Duration::from_millis(10));
        }
        preview.set_active(false);
        let shared = preview.shared.clone();
        drop(preview);
        let shared = shared.0.lock().unwrap();
        assert!(shared.stop);
        assert!(shared.frame.is_none());
    }
}
