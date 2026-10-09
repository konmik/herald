use super::instance;
use std::path::{Path, PathBuf};

pub(super) fn pick_video(window: &gpui_kit::Window, path: &Path, library: &Path) -> Result<Option<PathBuf>, String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
        FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;

    let directory = if path.is_dir() { path.to_path_buf() } else { path.parent().unwrap_or(library).to_path_buf() };
    let directory = if directory.is_dir() { directory } else { library.to_path_buf() };
    let initial_directory: Vec<u16> = directory.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let owner = instance::hwnd(window)?;
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }.map_err(|error| error.to_string())?;
    let result = (|| -> windows::core::Result<Option<PathBuf>> {
        let dialog: IFileOpenDialog = unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };
        unsafe {
            dialog.SetOptions(dialog.GetOptions()? | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM)?;
            dialog.SetTitle(w!("Choose character animation"))?;
            dialog.SetFileTypes(&[
                COMDLG_FILTERSPEC { pszName: w!("MP4 video"), pszSpec: w!("*.mp4") },
                COMDLG_FILTERSPEC { pszName: w!("All files"), pszSpec: w!("*.*") },
            ])?;
            let folder: IShellItem = SHCreateItemFromParsingName(PCWSTR(initial_directory.as_ptr()), None)?;
            dialog.SetFolder(&folder)?;
            if path.is_file() {
                let filename: Vec<u16> = path.file_name().unwrap().to_string_lossy().encode_utf16().chain(Some(0)).collect();
                dialog.SetFileName(PCWSTR(filename.as_ptr()))?;
            }
            if let Err(error) = dialog.Show(Some(windows::Win32::Foundation::HWND(owner))) {
                if error.code() == windows::core::HRESULT(0x800704C7u32 as i32) { return Ok(None); }
                return Err(error);
            }
            let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let selected = name.to_string();
            CoTaskMemFree(Some(name.0.cast()));
            Ok(Some(PathBuf::from(selected?)))
        }
    })();
    unsafe { CoUninitialize(); }
    result.map_err(|error| error.to_string())
}

pub(super) fn open_my_voices(window: &gpui_kit::Window) -> Result<(), String> {
    let hwnd = instance::hwnd(window)?;
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let url: Vec<u16> = "https://elevenlabs.io/app/voice-lab".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(hwnd, operation.as_ptr(), url.as_ptr(),
            std::ptr::null(), std::ptr::null(), windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL)
    };
    if result as isize <= 32 { Err("Could not open ElevenLabs My Voices in your browser.".into()) } else { Ok(()) }
}
