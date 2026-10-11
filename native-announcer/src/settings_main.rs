#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod settings_app;

include!("runtime.rs");

fn benchmark_preview(assets: &std::path::Path, settings: &settings::Settings, preset: &str, options: &benchmark::Options, recorder: &mut benchmark::Recorder) -> Result<u64, String> {
    let mut preview = settings_app::BenchmarkPreview::new(assets, settings, options.scale, benchmark::SEED)?;
    let period = Duration::from_secs_f64(1.0 / options.fps);
    let cycle_length = preview.cycle();
    let duration = cycle_length - settings.lightning.transition_duration();
    let mut at = Duration::ZERO;
    for cycle in 0..options.cycles {
        while at < cycle_length * (cycle as u32 + 1) {
            let start = Instant::now();
            let (image, phase) = preview.frame(if at.is_zero() { Duration::ZERO } else { period });
            let local = at - cycle_length * cycle as u32;
            let burst = phase != render::AnnouncementPhase::Holding || render::ambient_active(local, duration, benchmark::SEED, settings.lightning);
            recorder.record("preview", preset, cycle, local, phase, burst, start, image.as_raw());
            at += period;
        }
    }
    Ok(0)
}

fn main() {
    entrypoint(Executable::Settings {
        open_settings: settings_app::run,
        benchmark_preview,
    });
}

#[cfg(test)]
mod role_tests {
    use super::*;

    #[test]
    fn settings_role_defaults_to_settings_and_hosts_playback_when_selected() {
        let role = Executable::Settings { open_settings: settings_app::run, benchmark_preview };
        assert_eq!(role.default_mode(), InteractiveMode::Settings);
        assert!(role.benchmark_preview().is_some());
        assert_eq!(apply_mode(role.default_mode(), "--announcer"), InteractiveMode::Announcer);
    }
}
