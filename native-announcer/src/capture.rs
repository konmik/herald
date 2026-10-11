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
            "phase": crate::render::announcement_phase(if closing_start.is_some() { elapsed } else { elapsed.mul_f32(scene.map_or(1.0, |scene| scene.lightning().speed)) }, closing_start.is_some())});
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
#[path = "../unit_tests/capture.rs"]
mod tests;
