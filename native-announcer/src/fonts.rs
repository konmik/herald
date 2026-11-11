use ab_glyph::FontVec;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Weak};

pub type Font = Arc<FontVec>;

#[derive(Default)]
pub struct FontCatalog {
    faces: RefCell<HashMap<String, Weak<FontVec>>>,
}

impl FontCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, family: &str) -> Result<Font, String> {
        self.face(family, || load_font(family))
            .or_else(|| fallback_paths().into_iter().find_map(|(family, path)| {
                self.face(&family, || std::fs::read(path).ok().and_then(parse_font))
            }))
            .or_else(|| family_names().into_iter().find_map(|family| self.face(&family, || load_font(&family))))
            .ok_or_else(|| "No system font found".into())
    }

    fn face(&self, family: &str, load: impl FnOnce() -> Option<Font>) -> Option<Font> {
        let key = family.to_lowercase();
        if let Some(face) = self.faces.borrow().get(&key).and_then(Weak::upgrade) { return Some(face); }
        let face = load()?;
        let mut faces = self.faces.borrow_mut();
        faces.retain(|_, face| face.strong_count() > 0);
        faces.insert(key, Arc::downgrade(&face));
        Some(face)
    }
}

pub fn available_families() -> Result<Vec<String>, String> {
    Ok(family_names().into_iter().filter(|family| supports_family(family)).collect())
}

#[cfg(target_os = "windows")]
fn supports_family(family: &str) -> bool {
    font_bytes(family, Some(4)).is_some_and(|bytes| bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"OTTO"))
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn supports_family(family: &str) -> bool { load_font(family).is_some() }

#[cfg(target_os = "linux")]
fn supports_family(family: &str) -> bool {
    use std::io::Read;
    font_path(family).and_then(|path| std::fs::File::open(path).ok()).is_some_and(|mut file| {
        let mut signature = [0; 4];
        file.read_exact(&mut signature).is_ok() && (signature == [0, 1, 0, 0] || &signature == b"OTTO")
    })
}

fn parse_font(bytes: Vec<u8>) -> Option<Font> {
    FontVec::try_from_vec(bytes).ok().map(Arc::new)
}

fn fallback_paths() -> Vec<(String, PathBuf)> {
    if cfg!(target_os = "windows") {
        let directory = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        vec![
            ("Century Gothic".into(), PathBuf::from(&directory).join("Fonts/GOTHIC.TTF")),
            ("Segoe UI".into(), PathBuf::from(directory).join("Fonts/segoeui.ttf")),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            ("Century Gothic".into(), "/Library/Fonts/Century Gothic.ttf".into()),
            ("Arial".into(), "/System/Library/Fonts/Supplemental/Arial.ttf".into()),
        ]
    } else {
        vec![
            ("Century Gothic".into(), "/usr/share/fonts/truetype/msttcorefonts/Century_Gothic.ttf".into()),
            ("DejaVu Sans".into(), "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            ("DejaVu Sans".into(), "/usr/share/fonts/TTF/DejaVuSans.ttf".into()),
            ("Liberation Sans".into(), "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf".into()),
            ("Liberation Sans".into(), "/usr/share/fonts/liberation/LiberationSans-Regular.ttf".into()),
        ]
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn family_names() -> BTreeSet<String> {
    fallback_paths().into_iter().map(|(family, _)| family).collect()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn load_font(family: &str) -> Option<Font> {
    fallback_paths().into_iter().filter(|(name, _)| name == family)
        .find_map(|(_, path)| std::fs::read(path).ok().and_then(parse_font))
}

#[cfg(target_os = "linux")]
fn family_names() -> BTreeSet<String> {
    linux_font_paths().keys().cloned().collect()
}

#[cfg(target_os = "linux")]
fn font_path(family: &str) -> Option<PathBuf> {
    linux_font_paths().get(family).cloned()
}

#[cfg(target_os = "linux")]
fn linux_font_paths() -> &'static std::collections::BTreeMap<String, PathBuf> {
    static PATHS: std::sync::OnceLock<std::collections::BTreeMap<String, PathBuf>> = std::sync::OnceLock::new();
    PATHS.get_or_init(|| {
        let mut paths: std::collections::BTreeMap<String, PathBuf> = fallback_paths().into_iter().filter(|(_, path)| path.is_file()).collect();
        if let Ok(output) = std::process::Command::new("fc-list").args(["--format", "%{family}\t%{file}\n"]).output() {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if let Some((families, path)) = line.split_once('\t') {
                        for family in families.split(',').map(str::trim).filter(|family| !family.is_empty()) {
                            paths.entry(family.to_owned()).or_insert_with(|| PathBuf::from(path));
                        }
                    }
                }
            }
        }
        paths
    })
}

#[cfg(target_os = "linux")]
fn load_font(family: &str) -> Option<Font> {
    font_path(family).and_then(|path| std::fs::read(path).ok()).and_then(parse_font)
}

#[cfg(target_os = "windows")]
fn load_font(family: &str) -> Option<Font> {
    font_data(family).and_then(parse_font)
}

#[cfg(target_os = "windows")]
fn family_names() -> BTreeSet<String> {
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::Graphics::Gdi::*;

    let mut family_names: BTreeSet<String> = BTreeSet::new();
    unsafe {
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        if !hdc.is_null() {
            let mut logfont = LOGFONTW::default();
            logfont.lfCharSet = DEFAULT_CHARSET;
            EnumFontFamiliesExW(
                hdc,
                &logfont,
                Some(collect_family),
                &mut family_names as *mut BTreeSet<String> as LPARAM,
                0,
            );
            DeleteDC(hdc);
        }
    }

    family_names
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn collect_family(
    logfont: *const windows_sys::Win32::Graphics::Gdi::LOGFONTW,
    _text_metric: *const windows_sys::Win32::Graphics::Gdi::TEXTMETRICW,
    _font_type: u32,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> i32 {
    if logfont.is_null() || lparam == 0 { return 1; }
    let logfont = &*logfont;
    let end = logfont.lfFaceName.iter().position(|value| *value == 0).unwrap_or(logfont.lfFaceName.len());
    let family = String::from_utf16_lossy(&logfont.lfFaceName[..end]);
    if !family.trim().is_empty() && !family.starts_with('@') {
        (*(lparam as *mut BTreeSet<String>)).insert(family);
    }
    1
}

#[cfg(target_os = "windows")]
fn font_data(family: &str) -> Option<Vec<u8>> {
    font_bytes(family, None)
}

#[cfg(target_os = "windows")]
fn font_bytes(family: &str, limit: Option<u32>) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::*;

    unsafe {
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        if hdc.is_null() { return None; }
        let mut logfont = LOGFONTW::default();
        logfont.lfHeight = -16;
        logfont.lfWeight = FW_NORMAL as i32;
        logfont.lfCharSet = DEFAULT_CHARSET;
        let encoded: Vec<u16> = family.encode_utf16().take(logfont.lfFaceName.len() - 1).collect();
        logfont.lfFaceName[..encoded.len()].copy_from_slice(&encoded);
        let font = CreateFontIndirectW(&logfont);
        if font.is_null() {
            DeleteDC(hdc);
            return None;
        }
        let previous = SelectObject(hdc, font as HGDIOBJ);
        let selected = !previous.is_null() && previous as isize != -1;
        let mut realized = [0_u16; 32];
        let length = if selected { GetTextFaceW(hdc, realized.len() as i32, realized.as_mut_ptr()) } else { 0 };
        let end = realized.iter().position(|value| *value == 0).unwrap_or(realized.len());
        let matches_family = length > 0 && String::from_utf16_lossy(&realized[..end]).eq_ignore_ascii_case(family);
        let result = if selected && matches_family {
            let size = GetFontData(hdc, 0, 0, std::ptr::null_mut(), 0);
            if size == u32::MAX {
                None
            } else {
                let size = limit.map_or(size, |limit| size.min(limit));
                let mut bytes = vec![0_u8; size as usize];
                let written = GetFontData(hdc, 0, 0, bytes.as_mut_ptr().cast(), size);
                if written == u32::MAX {
                    None
                } else {
                    bytes.truncate(written as usize);
                    Some(bytes)
                }
            }
        } else {
            None
        };
        if selected { SelectObject(hdc, previous); }
        DeleteObject(font as HGDIOBJ);
        DeleteDC(hdc);
        result
    }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "../unit_tests/fonts.rs"]
mod tests;
