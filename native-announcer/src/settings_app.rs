#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn run(_data: &std::path::Path, _assets: &std::path::Path) -> Result<(), String> {
    Err("The settings app is available on Windows and Linux.".into())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod app;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use app::run;
