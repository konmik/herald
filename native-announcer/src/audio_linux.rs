use super::OutputDevice;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::Duration;

pub fn output_devices() -> Vec<OutputDevice> {
    let Ok(output) = Command::new("pactl").args(["--format=json", "list", "sinks"]).output() else { return Vec::new(); };
    if !output.status.success() { return Vec::new(); }
    let Ok(sinks) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) else { return Vec::new(); };
    sinks.into_iter().filter_map(|sink| {
        let id = sink.get("name")?.as_str()?.to_owned();
        let label = sink.get("description").and_then(|value| value.as_str()).unwrap_or(&id).to_owned();
        let index = sink.get("index")?.as_u64()?.try_into().ok()?;
        Some(OutputDevice { id, name: label, index })
    }).collect()
}

pub fn play_noise(path: &std::path::Path, volume: u16, selected: Option<&str>, cancelled: &AtomicBool) -> Result<(), String> {
    if volume == 0 || cancelled.load(Ordering::Relaxed) { return Ok(()); }
    let wav = std::fs::read(path).map_err(|error| error.to_string())?;
    let samples = super::decode_wav(&wav)?;
    play_pcm(&samples, 16000, &AtomicU16::new(volume), selected, cancelled)
}

pub fn play_pcm(samples: &[i16], sample_rate: u32, volume: &AtomicU16, selected: Option<&str>, cancelled: &AtomicBool) -> Result<(), String> {
    if samples.is_empty() || volume.load(Ordering::Relaxed) == 0 || cancelled.load(Ordering::Relaxed) { return Ok(()); }
    let mut command = Command::new("paplay");
    command.args(["--raw", "--format=s16le", "--channels=1", &format!("--rate={sample_rate}"), "--client-name=Herald"]);
    if let Some(device) = selected { command.arg(format!("--device={device}")); }
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped())
        .spawn().map_err(|error| format!("Could not start PulseAudio playback (paplay): {error}"))?;
    let result = (|| {
        let mut input = child.stdin.take().ok_or("PulseAudio playback input is unavailable")?;
        let fd = input.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        for chunk in samples.chunks(1024) {
            let mut scaled = chunk.to_vec();
            super::scale_samples(&mut scaled, volume.load(Ordering::Relaxed));
            let bytes: Vec<u8> = scaled.into_iter().flat_map(i16::to_le_bytes).collect();
            let mut remaining = bytes.as_slice();
            while !remaining.is_empty() {
                if cancelled.load(Ordering::Relaxed) { return Ok(()); }
                match input.write(remaining) {
                    Ok(0) => return Err("PulseAudio playback input closed".into()),
                    Ok(count) => remaining = &remaining[count..],
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                    Err(error) => return Err(format!("PulseAudio playback failed: {error}")),
                }
            }
        }
        drop(input);
        loop {
            if cancelled.load(Ordering::Relaxed) { return Ok(()); }
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                return if status.success() { Ok(()) } else { Err(format!("PulseAudio playback exited with {status}. Check the selected output device and audio service.")) };
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if result.is_err() || cancelled.load(Ordering::Relaxed) { let _ = child.kill(); }
    let _ = child.wait();
    result
}
