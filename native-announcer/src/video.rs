use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn select_path(assets: &Path, character: &str) -> PathBuf {
    use std::hash::{BuildHasher, Hasher};
    let paths: Vec<_> = std::fs::read_dir(assets.join("videos"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")))
        .collect();
    if paths.is_empty() {
        assets.join(character).join("neutral.mp4")
    } else {
        let random = std::collections::hash_map::RandomState::new().build_hasher().finish();
        paths[random as usize % paths.len()].clone()
    }
}

pub struct Video {
    decoder: Decoder,
    frame: RgbaImage,
    tick: Option<u64>,
    pub decoded_frames: u64,
    pub loops: u64,
}

impl Video {
    pub fn open(path: &Path) -> Result<Self, String> {
        let decoder = Decoder::open(path)?;
        let (width, height) = decoder.dimensions();
        Ok(Self {
            decoder,
            frame: RgbaImage::new(width, height),
            tick: None,
            decoded_frames: 0,
            loops: 0,
        })
    }

    pub fn advance(&mut self, elapsed: Duration) -> Result<(), String> {
        let tick = (elapsed.as_secs_f64() * self.fps()) as u64;
        if self.tick == Some(tick) {
            return Ok(());
        }
        self.loops += self.decoder.read(&mut self.frame)? as u64;
        self.decoded_frames += 1;
        self.tick = Some(tick);
        Ok(())
    }

    pub fn frame(&self) -> &RgbaImage {
        &self.frame
    }

    pub fn fps(&self) -> f64 {
        self.decoder.fps()
    }
}

#[cfg(target_os = "windows")]
use native::Decoder;

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    struct Runtime {
        com: bool,
        media: bool,
    }

    impl Drop for Runtime {
        fn drop(&mut self) {
            unsafe {
                if self.media {
                    let _ = MFShutdown();
                }
                if self.com {
                    CoUninitialize();
                }
            }
        }
    }

    pub struct Decoder {
        reader: IMFSourceReader,
        stride: i32,
        width: u32,
        height: u32,
        fps: f64,
        _runtime: Runtime,
    }

    impl Decoder {
        pub fn open(path: &Path) -> Result<Self, String> {
            unsafe {
                let mut runtime = Runtime {
                    com: CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok(),
                    media: false,
                };
                MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| e.to_string())?;
                runtime.media = true;
                let mut attributes = None;
                MFCreateAttributes(&mut attributes, 1).map_err(|e| e.to_string())?;
                let attributes = attributes.ok_or("No video attributes")?;
                attributes
                    .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
                    .map_err(|e| e.to_string())?;
                let filename: Vec<u16> = path
                    .as_os_str()
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let reader = MFCreateSourceReaderFromURL(PCWSTR(filename.as_ptr()), &attributes)
                    .map_err(|e| e.to_string())?;
                reader
                    .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                    .map_err(|e| e.to_string())?;
                reader
                    .SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)
                    .map_err(|e| e.to_string())?;
                let media = MFCreateMediaType().map_err(|e| e.to_string())?;
                media
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                    .map_err(|e| e.to_string())?;
                media
                    .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                    .map_err(|e| e.to_string())?;
                reader
                    .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &media)
                    .map_err(|e| e.to_string())?;
                let actual = reader
                    .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
                    .map_err(|e| e.to_string())?;
                let dimensions = actual
                    .GetUINT64(&MF_MT_FRAME_SIZE)
                    .map_err(|e| e.to_string())?;
                let width = (dimensions >> 32) as u32;
                let height = dimensions as u32;
                if width == 0 || height == 0 || width > 4096 || height > 4096 {
                    return Err("Unsupported video dimensions".into());
                }
                let stride = actual.GetUINT32(&MF_MT_DEFAULT_STRIDE).unwrap_or(width * 4) as i32;
                if stride.unsigned_abs() < width * 4 {
                    return Err("Unexpected video stride".into());
                }
                let rate = actual.GetUINT64(&MF_MT_FRAME_RATE).map_err(|e| e.to_string())?;
                let fps = (rate >> 32) as f64 / (rate as u32) as f64;
                if !fps.is_finite() || !(1.0..=240.0).contains(&fps) {
                    return Err("Unsupported video frame rate".into());
                }
                Ok(Self {
                    reader,
                    stride,
                    width,
                    height,
                    fps,
                    _runtime: runtime,
                })
            }
        }

        pub fn dimensions(&self) -> (u32, u32) {
            (self.width, self.height)
        }

        pub fn fps(&self) -> f64 {
            self.fps
        }

        pub fn read(&mut self, frame: &mut RgbaImage) -> Result<bool, String> {
            unsafe {
                let mut looped = false;
                for _ in 0..8 {
                    let mut flags = 0;
                    let mut sample = None;
                    self.reader
                        .ReadSample(
                            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                            0,
                            None,
                            Some(&mut flags),
                            None,
                            Some(&mut sample),
                        )
                        .map_err(|e| e.to_string())?;
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        if looped {
                            return Err("Video has no frames".into());
                        }
                        self.reader
                            .SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(0_i64))
                            .map_err(|e| e.to_string())?;
                        looped = true;
                        continue;
                    }
                    let Some(sample) = sample else {
                        continue;
                    };
                    let buffer = sample
                        .ConvertToContiguousBuffer()
                        .map_err(|e| e.to_string())?;
                    let mut bytes = std::ptr::null_mut();
                    let mut length = 0;
                    buffer
                        .Lock(&mut bytes, None, Some(&mut length))
                        .map_err(|e| e.to_string())?;
                    let result = if length >= self.stride.unsigned_abs() * self.height {
                        let input = std::slice::from_raw_parts(bytes, length as usize);
                        for y in 0..self.height {
                            let source_y = if self.stride < 0 { self.height - 1 - y } else { y };
                            for x in 0..self.width {
                                let index = (source_y * self.stride.unsigned_abs() + x * 4) as usize;
                                frame.put_pixel(
                                    x,
                                    y,
                                    image::Rgba([
                                        input[index + 2],
                                        input[index + 1],
                                        input[index],
                                        255,
                                    ]),
                                );
                            }
                        }
                        Ok(looped)
                    } else {
                        Err("Video frame is truncated".into())
                    };
                    buffer.Unlock().map_err(|e| e.to_string())?;
                    return result;
                }
                Err("Video decoder did not produce a frame".into())
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
struct Decoder {
    child: std::process::Child,
    output: std::process::ChildStdout,
}

#[cfg(not(target_os = "windows"))]
impl Decoder {
    fn dimensions(&self) -> (u32, u32) {
        (128, 128)
    }

    fn fps(&self) -> f64 {
        crate::state::VIDEO_FPS as f64
    }

    fn open(path: &Path) -> Result<Self, String> {
        use std::process::{Command, Stdio};
        let mut child = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-threads",
                "1",
                "-stream_loop",
                "-1",
                "-i",
            ])
            .arg(path)
            .args([
                "-an",
                "-vf",
                "fps=8,scale=128:128",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let output = child.stdout.take().ok_or("No video decoder output")?;
        Ok(Self { child, output })
    }

    fn read(&mut self, frame: &mut RgbaImage) -> Result<bool, String> {
        use std::io::Read;
        self.output
            .read_exact(frame.as_mut())
            .map_err(|e| e.to_string())?;
        Ok(false)
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_the_integration_video_without_a_shared_library() {
        let assets = Path::new("missing-announcer-test-assets");
        assert_eq!(select_path(assets, "claude"), assets.join("claude/neutral.mp4"));
    }

    #[test]
    fn selects_only_videos_from_the_shared_library() {
        let directory = std::env::temp_dir().join(format!("civilized-video-selection-{}-{}", std::process::id(), crate::state::timestamp()));
        let library = directory.join("videos");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("first.mp4"), []).unwrap();
        std::fs::write(library.join("second.MP4"), []).unwrap();
        std::fs::write(library.join("portrait.png"), []).unwrap();
        std::fs::create_dir(library.join("directory.mp4")).unwrap();
        for _ in 0..100 {
            let selected = select_path(&directory, "opencode");
            assert!(selected == library.join("first.mp4") || selected == library.join("second.MP4"));
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
