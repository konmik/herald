use crate::characters::{validate_registry, Character};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub const MIN_FONT_SIZE: u16 = 8;
pub const MAX_FONT_SIZE: u16 = 32;
pub const DEFAULT_SUMMARY_PROMPT: &str = "Report the outcome of the task you just finished in one explicit, concise spoken sentence. State what was done and any important failure or remaining blocker. Use plain English, no Markdown. Do not run tools. Output only that sentence.";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FontPreference {
    pub family: String,
    pub size: u16,
}

impl FontPreference {
    pub fn new(family: impl Into<String>, size: u16) -> Self {
        Self { family: family.into(), size }
    }

    fn validate(&self, label: &str) -> Result<(), String> {
        if self.family.trim().is_empty() || self.family.contains('\0') {
            return Err(format!("{label} font family must not be empty."));
        }
        if !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&self.size) {
            return Err(format!("{label} font size must be between {MIN_FONT_SIZE} and {MAX_FONT_SIZE} logical pixels."));
        }
        Ok(())
    }
}

impl Default for FontPreference {
    fn default() -> Self { Self::new("Century Gothic", 18) }
}

fn default_body_font() -> FontPreference { FontPreference::new("Century Gothic", 18) }

fn default_title_font() -> FontPreference { FontPreference::new("Century Gothic", 14) }

fn default_summary_prompt() -> String { DEFAULT_SUMMARY_PROMPT.into() }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub quiet_mode: bool,
    pub schedule_enabled: bool,
    pub quiet_start: u32,
    pub quiet_end: u32,
    pub volume: u16,
    pub silent_sound_seconds: u16,
    pub output_device: Option<String>,
    #[serde(with = "api_key_storage")]
    pub elevenlabs_api_key: Option<String>,
    pub speech_model: crate::elevenlabs::SpeechModel,
    pub default_voice_id: String,
    pub voices: HashMap<String, String>,
    pub characters: BTreeMap<String, Character>,
    pub selected_character: Option<String>,
    #[serde(default)]
    pub installed_bundled_characters: BTreeSet<String>,
    #[serde(default = "default_body_font")]
    pub announcement_body_font: FontPreference,
    #[serde(default = "default_title_font")]
    pub announcement_title_font: FontPreference,
    #[serde(default = "default_summary_prompt")]
    pub summary_prompt: String,
    pub lightning: crate::lightning::LightningSettings,
}

mod api_key_storage {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(key: &Option<String>, serializer: S) -> Result<S::Ok, S::Error> {
        key.as_deref().map(protect).transpose().map_err(serde::ser::Error::custom)?.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
        Option::<String>::deserialize(deserializer)?.map(|key| unprotect(&key).map_err(serde::de::Error::custom)).transpose()
    }

    #[cfg(target_os = "windows")]
    fn crypt(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
        use windows_sys::Win32::Security::Cryptography::*;
        let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        unsafe {
            let result = if encrypt {
                CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            };
            if result == 0 { return Err("Could not access the saved ElevenLabs key for this Windows user.".into()); }
            let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            windows_sys::Win32::Foundation::LocalFree(output.pbData as *mut _);
            Ok(bytes)
        }
    }

    fn protect(key: &str) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        {
            use base64::Engine;
            Ok(format!("dpapi:{}", base64::engine::general_purpose::STANDARD.encode(crypt(key.as_bytes(), true)?)))
        }
        #[cfg(not(target_os = "windows"))]
        { Ok(key.to_owned()) }
    }

    fn unprotect(key: &str) -> Result<String, String> {
        if let Some(encoded) = key.strip_prefix("dpapi:") {
            #[cfg(target_os = "windows")]
            {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| "Invalid saved ElevenLabs key.".to_string())?;
                String::from_utf8(crypt(&bytes, false)?).map_err(|_| "Invalid saved ElevenLabs key.".into())
            }
            #[cfg(not(target_os = "windows"))]
            { let _ = encoded; Err("This saved key belongs to a Windows user. Enter a new key.".into()) }
        } else { Ok(key.to_owned()) }
    }
}

impl Default for Settings {
    fn default() -> Self {
        let mut settings = Self {
            quiet_mode: false,
            schedule_enabled: true,
            quiet_start: 22 * 60,
            quiet_end: 8 * 60,
            volume: 100,
            silent_sound_seconds: 0,
            output_device: None,
            elevenlabs_api_key: None,
            speech_model: crate::elevenlabs::SpeechModel::default(),
            default_voice_id: crate::elevenlabs::DEFAULT_VOICE_ID.into(),
            voices: HashMap::new(),
            characters: BTreeMap::new(),
            selected_character: None,
            installed_bundled_characters: BTreeSet::new(),
            announcement_body_font: default_body_font(),
            announcement_title_font: default_title_font(),
            summary_prompt: default_summary_prompt(),
            lightning: crate::lightning::LightningSettings::default(),
        };
        settings.reconcile_bundled_characters();
        settings
    }
}

impl Settings {
    pub fn quiet_at(&self, minute: u32) -> bool {
        self.quiet_mode || (self.schedule_enabled && crate::state::is_night(minute, self.quiet_start, self.quiet_end))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.quiet_start >= 1440 || self.quiet_end > 1440 {
            return Err("Enter a valid daily quiet schedule.".into());
        }
        if self.volume > 100 { return Err("Volume must be between 0 and 100%.".into()); }
        if self.silent_sound_seconds > 10 { return Err("Silent sound must be between 0 and 10 seconds.".into()); }
        if let Some(key) = &self.elevenlabs_api_key {
            crate::elevenlabs::validate_api_key(key)?;
        }
        crate::characters::validate_voice_id(&self.default_voice_id)?;
        for id in &self.installed_bundled_characters {
            crate::characters::validate_id(id)?;
        }
        validate_registry(&self.characters, self.selected_character.as_deref())?;
        self.announcement_body_font.validate("Announcement body")?;
        self.announcement_title_font.validate("Announcement title")?;
        validate_summary_prompt(&self.summary_prompt)?;
        self.lightning.validate()?;
        Ok(())
    }

    pub fn reconcile_bundled_characters(&mut self) {
        for (id, character) in crate::characters::bundled_characters() {
            if self.installed_bundled_characters.insert(id.clone()) {
                self.characters.entry(id.clone()).or_insert_with(|| character.clone());
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        for (old, new) in [("nightStart", "quietStart"), ("nightEnd", "quietEnd")] {
            if value.get(new).is_none() {
                if let Some(hour) = value.get(old).and_then(|value| value.as_u64()) {
                    value[new] = serde_json::json!(hour.checked_mul(60).ok_or("Invalid quiet schedule")?);
                }
            }
        }
        let mut settings: Self = serde_json::from_value(value).map_err(|error| error.to_string())?;
        settings.reconcile_bundled_characters();
        settings.validate()?;
        Ok(settings)
    }

    pub fn load(data: &Path) -> Result<Self, String> {
        match std::fs::read(data.join("settings.json")) {
            Ok(bytes) => Self::decode(&bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn save(&self, data: &Path) -> Result<(), String> {
        self.validate()?;
        crate::private::directory(data).map_err(|error| error.to_string())?;
        let temporary = data.join(format!("settings-{}-{}.tmp", std::process::id(), crate::state::timestamp()));
        let result = (|| {
            crate::private::write(&temporary, &serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
            std::fs::rename(&temporary, data.join("settings.json")).map_err(|error| error.to_string())
        })();
        if result.is_err() { let _ = std::fs::remove_file(temporary); }
        result
    }
}

pub struct Store {
    path: PathBuf,
    bytes: Vec<u8>,
    pub current: Settings,
}

impl Store {
    pub fn new(data: &Path) -> Result<Self, String> {
        let path = data.join("settings.json");
        if !path.exists() { Settings::default().save(data)?; }
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
        let current = Settings::decode(&bytes)?;
        Ok(Self { path, bytes, current })
    }

    pub fn reload(&mut self) -> Result<bool, String> {
        let bytes = std::fs::read(&self.path).map_err(|error| error.to_string())?;
        if bytes == self.bytes { return Ok(false); }
        self.bytes = bytes;
        self.current = Settings::decode(&self.bytes)?;
        Ok(true)
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", test))]
pub fn parse_time(text: &str, allow_midnight_end: bool) -> Result<u32, String> {
    let (hour, minute) = text.trim().split_once(':').ok_or("Use HH:MM for times.")?;
    let hour = hour.parse::<u32>().map_err(|_| "Use HH:MM for times.")?;
    let minute = minute.parse::<u32>().map_err(|_| "Use HH:MM for times.")?;
    if minute >= 60 || hour > 24 || (hour == 24 && (!allow_midnight_end || minute != 0)) {
        return Err("Use a time between 00:00 and 23:59 (24:00 is allowed for the end).".into());
    }
    Ok(hour * 60 + minute)
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", test))]
pub fn format_time(minute: u32) -> String { format!("{:02}:{:02}", minute / 60, minute % 60) }

pub fn volume_gain(volume: u16) -> f64 {
    if volume == 0 { 0.0 } else { 10_f64.powf((-60.0 + 0.6 * f64::from(volume.min(100))) / 20.0) }
}

pub fn validate_summary_prompt(prompt: &str) -> Result<(), String> {
    if prompt.is_empty() { return Err("Summary prompt must not be empty.".into()); }
    if prompt.chars().count() > 16_384 { return Err("Summary prompt must be at most 16384 characters.".into()); }
    if prompt.contains('\0') { return Err("Summary prompt must not contain NUL characters.".into()); }
    Ok(())
}

#[cfg(test)]
#[path = "../unit_tests/settings.rs"]
mod tests;
