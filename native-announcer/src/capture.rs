use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct Frames {
    sender: Option<SyncSender<CapturedFrame>>,
    writer: Option<JoinHandle<Result<(), String>>>,
    index: u64,
}

struct CapturedFrame {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    entry: serde_json::Value,
}

impl Frames {
    pub fn new(directory: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut timeline = std::fs::File::create(directory.join("timeline.jsonl"))
            .map_err(|error| error.to_string())?;
        let directory = directory.to_path_buf();
        let (sender, receiver) = sync_channel::<CapturedFrame>(32);
        let writer = std::thread::spawn(move || {
            for frame in receiver {
                let image = image::RgbImage::from_fn(frame.width, frame.height, |x, y| {
                    let color = frame.pixels[(y * frame.width + x) as usize];
                    let color = if color == 0xff00ff { 0x202020 } else { color };
                    image::Rgb([(color >> 16) as u8, (color >> 8) as u8, color as u8])
                });
                let filename = frame.entry["file"].as_str().ok_or("Missing capture filename")?;
                image.save(directory.join(filename)).map_err(|error| error.to_string())?;
                writeln!(timeline, "{}", frame.entry).map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        Ok(Self { sender: Some(sender), writer: Some(writer), index: 0 })
    }

    #[cfg(test)]
    pub fn save(&mut self, pixels: &[u32], width: u32, height: u32, elapsed: Duration, closing_start: Option<Duration>) -> Result<(), String> {
        self.write(pixels, width, height, elapsed, closing_start, None)
    }

    pub fn save_scene(&mut self, pixels: &[u32], width: u32, height: u32, elapsed: Duration, closing_start: Option<Duration>, card: crate::render::CardPlacement, scene: Option<&crate::render::EntranceScene>, viewport: crate::render::PhysicalRect) -> Result<(), String> {
        let geometry = serde_json::json!({"monitor": scene.map(|scene| scene.monitor), "viewport": viewport,
            "card": card.rect, "scale": card.scale, "source": scene.map(|scene| scene.source), "impact": scene.map(|scene| scene.impact),
            "phase": crate::render::announcement_phase(elapsed, closing_start.is_some())});
        self.write(pixels, width, height, elapsed, closing_start, Some(geometry))
    }

    fn write(&mut self, pixels: &[u32], width: u32, height: u32, elapsed: Duration, closing_start: Option<Duration>, geometry: Option<serde_json::Value>) -> Result<(), String> {
        let filename = format!("frame-{:06}.png", self.index);
        let mut entry = serde_json::json!({"file":filename,"elapsed":elapsed.as_secs_f64(),"closingStart":closing_start.map(|value| value.as_secs_f64())});
        if let Some(geometry) = geometry { entry["geometry"] = geometry; }
        self.sender.as_ref().ok_or("Frame writer closed")?.send(CapturedFrame { pixels: pixels.to_vec(), width, height, entry }).map_err(|error| error.to_string())?;
        self.index += 1;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        self.sender.take();
        if let Some(writer) = self.writer.take() {
            writer.join().map_err(|_| "Frame writer failed".to_string())??;
        }
        Ok(())
    }
}

impl Drop for Frames {
    fn drop(&mut self) { let _ = self.finish(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_only_rendered_pixels_with_a_plain_background_and_timing() {
        let directory = std::env::temp_dir().join("opencode").join(format!("herald-frames-{}-{}", std::process::id(), crate::state::timestamp()));
        let mut frames = Frames::new(&directory).unwrap();
        frames.save(&[0xff00ff, 0x123456], 2, 1, Duration::from_millis(15), None).unwrap();
        frames.save(&[0xffffff, 0], 2, 1, Duration::from_millis(80), Some(Duration::from_millis(70))).unwrap();
        drop(frames);
        let image = image::open(directory.join("frame-000000.png")).unwrap().to_rgb8();
        assert_eq!(image.get_pixel(0, 0).0, [32, 32, 32]);
        assert_eq!(image.get_pixel(1, 0).0, [18, 52, 86]);
        let timeline = std::fs::read_to_string(directory.join("timeline.jsonl")).unwrap();
        let entries: Vec<serde_json::Value> = timeline.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["elapsed"], 0.015);
        assert_eq!(entries[1]["elapsed"], 0.08);
        assert!(entries[0]["closingStart"].is_null());
        assert_eq!(entries[1]["closingStart"], 0.07);
        assert!(entries[0].get("geometry").is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn expanded_and_restored_captures_keep_physical_geometry_and_the_sampled_phase() {
        let directory = std::env::temp_dir().join("opencode").join(format!("herald-scene-{}", std::process::id()));
        let card = crate::render::CardPlacement { rect: crate::render::PhysicalRect { x: -504, y: 250, width: 480, height: 390 }, scale: 1.5 };
        let scene = crate::render::EntranceScene::new(crate::render::PhysicalRect { x: -1920, y: -200, width: 1920, height: 1080 }, card, 1234, crate::lightning::LightningSettings::default());
        let mut frames = Frames::new(&directory).unwrap();
        let pixels = vec![0x123456; scene.canvas.width as usize * scene.canvas.height as usize];
        frames.save_scene(&pixels, scene.canvas.width, scene.canvas.height, Duration::from_millis(140), None, card, Some(&scene), scene.canvas).unwrap();
        frames.save_scene(&vec![0; 480 * 390], 480, 390, Duration::from_millis(700), None, card, Some(&scene), card.rect).unwrap();
        frames.finish().unwrap();
        let timeline = std::fs::read_to_string(directory.join("timeline.jsonl")).unwrap();
        let entries: Vec<serde_json::Value> = timeline.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(entries[0]["geometry"]["phase"], "impact");
        assert_eq!(entries[0]["geometry"]["monitor"]["y"], -200);
        assert_eq!(entries[0]["geometry"]["source"][1], 879.0);
        assert_eq!(entries[0]["geometry"]["card"]["x"], -504);
        assert_eq!(entries[0]["geometry"]["scale"], 1.5);
        assert_eq!(entries[1]["geometry"]["phase"], "holding");
        assert_eq!(entries[1]["geometry"]["viewport"], serde_json::json!({"x": -504, "y": 250, "width": 480, "height": 390}));
        assert_eq!(image::open(directory.join("frame-000001.png")).unwrap().width(), 480);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
