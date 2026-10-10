#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn run(_data: &std::path::Path, _assets: &std::path::Path) -> Result<(), String> {
    Err("The settings app is available on Windows and Linux.".into())
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod app;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub use app::run;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(crate) use app::BenchmarkPreview;
