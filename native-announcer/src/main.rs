#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

include!("runtime.rs");

fn settings_binary(executable: &std::path::Path) -> Result<PathBuf, String> {
    let name = executable.file_name().and_then(|name| name.to_str()).ok_or("Invalid announcer executable name")?;
    let name = name.strip_prefix("herald").map(|suffix| format!("herald-settings{suffix}"));
    #[cfg(target_os = "macos")]
    if executable.file_name().is_some_and(|name| name == "Herald Announcer") {
        return Ok(executable.parent().and_then(|path| path.parent()).and_then(|path| path.parent())
            .and_then(|path| path.parent()).and_then(|path| path.parent())
            .ok_or("Invalid announcer helper path")?.join("MacOS/Herald"));
    }
    Ok(executable.with_file_name(name.ok_or("Could not locate the Settings executable")?))
}

fn run_settings(data: &std::path::Path, assets: &std::path::Path) -> Result<(), String> {
    let binary = settings_binary(&std::env::current_exe().map_err(|error| error.to_string())?)?;
    std::process::Command::new(binary).arg("--settings").arg("--assets").arg(assets)
        .env("HERALD_DATA", data).spawn().map_err(|error| error.to_string())?;
    Ok(())
}

fn main() {
    entrypoint(Executable::Announcer { open_settings: run_settings });
}

#[cfg(test)]
mod binary_tests {
    use super::*;

    #[test]
    fn settings_binary_keeps_the_platform_suffix() {
        for (announcer, settings) in [("herald.exe", "herald-settings.exe"), ("herald", "herald-settings"),
            ("herald-win32-x64.exe", "herald-settings-win32-x64.exe"), ("herald-linux-arm64", "herald-settings-linux-arm64")] {
            assert_eq!(settings_binary(std::path::Path::new(announcer)).unwrap(), PathBuf::from(settings));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn settings_binary_resolves_the_main_application_from_the_helper() {
        let helper = std::path::Path::new("/Applications/Herald.app/Contents/Helpers/Herald Announcer.app/Contents/MacOS/Herald Announcer");
        assert_eq!(settings_binary(helper).unwrap(), PathBuf::from("/Applications/Herald.app/Contents/MacOS/Herald"));
    }

    #[test]
    fn announcer_role_defaults_to_playback_without_preview() {
        let role = Executable::Announcer { open_settings: run_settings };
        assert_eq!(role.default_mode(), InteractiveMode::Announcer);
        assert!(role.benchmark_preview().is_none());
    }
}
