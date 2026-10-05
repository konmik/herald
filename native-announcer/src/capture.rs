use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Frames {
    directory: PathBuf,
    timeline: std::fs::File,
    index: u64,
}

impl Frames {
    pub fn new(directory: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let timeline = std::fs::File::create(directory.join("timeline.jsonl"))
            .map_err(|error| error.to_string())?;
        Ok(Self { directory: directory.into(), timeline, index: 0 })
    }

    pub fn save(&mut self, pixels: &[u32], width: u32, height: u32, elapsed: Duration, closing_start: Option<Duration>) -> Result<(), String> {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            let color = pixels[(y * width + x) as usize];
            let color = if color == 0xff00ff { 0x202020 } else { color };
            image::Rgb([(color >> 16) as u8, (color >> 8) as u8, color as u8])
        });
        let filename = format!("frame-{:06}.png", self.index);
        image.save(self.directory.join(&filename)).map_err(|error| error.to_string())?;
        let entry = serde_json::json!({"file":filename,"elapsed":elapsed.as_secs_f64(),"closingStart":closing_start.map(|value| value.as_secs_f64())});
        writeln!(self.timeline, "{entry}").map_err(|error| error.to_string())?;
        self.index += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_only_rendered_pixels_with_a_plain_background_and_timing() {
        let directory = std::env::temp_dir().join("opencode").join(format!("civilized-frames-{}-{}", std::process::id(), crate::state::timestamp()));
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
        std::fs::remove_dir_all(directory).unwrap();
    }
}
