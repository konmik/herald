#[cfg(not(target_os = "windows"))]
pub fn run(_data: &std::path::Path, _assets: &std::path::Path) -> Result<(), String> {
    Err("The settings app is currently available on Windows.".into())
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::run;
