use image::RgbaImage;
use std::path::Path;
use std::time::Duration;

pub struct Video {
    decoder: Decoder,
    frame: RgbaImage,
    tick: Option<u64>,
    pub decoded_frames: u64,
    pub loops: u64,
}

impl Video {
    pub fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            decoder: Decoder::open(path)?,
            frame: RgbaImage::new(128, 128),
            tick: None,
            decoded_frames: 0,
            loops: 0,
        })
    }

    pub fn advance(&mut self, elapsed: Duration) -> Result<(), String> {
        let tick = (elapsed.as_secs_f64() * crate::state::VIDEO_FPS as f64) as u64;
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
                if dimensions >> 32 != 128 || dimensions as u32 != 128 {
                    return Err("Announcer videos must be 128 by 128 pixels".into());
                }
                let stride = actual.GetUINT32(&MF_MT_DEFAULT_STRIDE).unwrap_or(512) as i32;
                if stride.unsigned_abs() != 512 {
                    return Err("Unexpected video stride".into());
                }
                Ok(Self {
                    reader,
                    stride,
                    _runtime: runtime,
                })
            }
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
                    let result = if length >= 128 * 128 * 4 {
                        let input = std::slice::from_raw_parts(bytes, length as usize);
                        for y in 0..128 {
                            let source_y = if self.stride < 0 { 127 - y } else { y };
                            for x in 0..128 {
                                let index = (source_y * 128 + x) * 4;
                                frame.put_pixel(
                                    x as u32,
                                    y as u32,
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
