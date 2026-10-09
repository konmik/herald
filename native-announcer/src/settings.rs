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

#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub fn parse_time(text: &str, allow_midnight_end: bool) -> Result<u32, String> {
    let (hour, minute) = text.trim().split_once(':').ok_or("Use HH:MM for times.")?;
    let hour = hour.parse::<u32>().map_err(|_| "Use HH:MM for times.")?;
    let minute = minute.parse::<u32>().map_err(|_| "Use HH:MM for times.")?;
    if minute >= 60 || hour > 24 || (hour == 24 && (!allow_midnight_end || minute != 0)) {
        return Err("Use a time between 00:00 and 23:59 (24:00 is allowed for the end).".into());
    }
    Ok(hour * 60 + minute)
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
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
mod tests {
    use super::*;

    #[test]
    fn legacy_lightning_defaults_are_original() {
        let original = crate::lightning::PRESETS[0].settings;
        assert_eq!(Settings::default().lightning, original);
        assert_eq!(Settings::decode(br#"{"volume":35}"#).unwrap().lightning, original);
        assert_eq!(Settings::decode(br#"{"lightning":{}}"#).unwrap().lightning, original);
        assert_eq!(Settings::decode(br#"{"lightning":{"preset":"storm","brightness":1.5}}"#).unwrap().lightning,
            crate::lightning::LightningSettings { brightness: 1.5, ..original });
    }

    #[test]
    fn inherited_capture_selector_is_ignored_by_settings_defaults() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "settings::tests::legacy_lightning_defaults_are_original"])
            .env("HERALD_LIGHTNING_STYLE", "plasma").output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    #[test]
    fn lightning_persists_and_hot_reload_rejects_invalid_external_values() {
        let data = std::env::temp_dir().join("opencode").join(format!("herald-lightning-settings-{}-{}", std::process::id(), crate::state::timestamp()));
        let mut settings = Settings::default();
        settings.save(&data).unwrap();
        let mut store = Store::new(&data).unwrap();
        for preset in &crate::lightning::PRESETS[1..] {
            settings.lightning = preset.settings;
            settings.save(&data).unwrap();
            assert_eq!(Settings::load(&data).unwrap(), settings);
            assert!(store.reload().unwrap());
            assert_eq!(store.current.lightning, preset.settings);
            assert!(!store.reload().unwrap());
        }
        let good = std::fs::read(data.join("settings.json")).unwrap();
        settings.lightning.brightness = f32::NAN;
        assert!(settings.save(&data).is_err());
        assert_eq!(std::fs::read(data.join("settings.json")).unwrap(), good);
        for invalid in [r#"{"brightness":2.01}"#, r#"{"roughness":0}"#, r#"{"coreWidth":null}"#,
            r#"{"glowSpread":25}"#, r#"{"glowStrength":1}"#, r#"{"strokeRadius":0}"#,
            r#"{"entranceForkSpacing":0}"#, r#"{"boltForkSpacing":129}"#, r#"{"outlineForkSpacing":-1}"#,
            r#"{"haloColor":16777216}"#, r#"{"haloColor":16711935}"#, r#"{"coreColor":16711935}"#, r#"{"pulseProfile":"unknown"}"#] {
            std::fs::write(data.join("settings.json"), format!("{{\"lightning\":{invalid}}}")).unwrap();
            assert!(store.reload().is_err());
            assert_eq!(store.current.lightning, crate::lightning::PRESETS[3].settings);
        }
        std::fs::write(data.join("settings.json"), good).unwrap();
        assert!(store.reload().unwrap());
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn silent_sound_defaults_to_zero_and_survives_save_load_and_reload() {
        assert_eq!(Settings::default().silent_sound_seconds, 0);
        assert_eq!(Settings::decode(br#"{"volume":35}"#).unwrap().silent_sound_seconds, 0);
        let data = std::env::temp_dir().join("opencode").join(format!("herald-silent-sound-{}-{}", std::process::id(), crate::state::timestamp()));
        let mut settings = Settings::default();
        settings.save(&data).unwrap();
        let mut store = Store::new(&data).unwrap();
        for seconds in [0, 1, 5, 10] {
            settings.silent_sound_seconds = seconds;
            settings.save(&data).unwrap();
            let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
            assert_eq!(saved["silentSoundSeconds"], seconds);
            assert_eq!(Settings::load(&data).unwrap(), settings);
            assert_eq!(store.reload().unwrap(), seconds != 0);
            assert_eq!(store.current, settings);
            assert!(!store.reload().unwrap());
        }
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn silent_sound_rejects_invalid_seconds() {
        for value in ["11", "-1", "1.5", "\"2\"", "null"] {
            assert!(Settings::decode(format!("{{\"silentSoundSeconds\":{value}}}").as_bytes()).is_err());
        }
        assert!(Settings { silent_sound_seconds: 11, ..Settings::default() }.validate().is_err());
    }

    #[test]
    fn legacy_description_and_sample_text_are_ignored_and_dropped_when_saved() {
        let custom = serde_json::json!({
            "name": "My herald",
            "voiceDescription": "A saved custom voice",
            "sampleText": "This is my saved character sample text.",
            "animationPath": "C:\\Videos\\my-herald.mp4",
            "voice": {"type": "elevenLabs", "voiceId": "saved-voice"}
        });
        let bytes = serde_json::json!({
            "characters": {"hatted-herald-01": custom},
            "selectedCharacter": "hatted-herald-01",
            "voices": {"opencode": "Luna"}
        });
        let settings = Settings::decode(&serde_json::to_vec(&bytes).unwrap()).unwrap();
        let saved = &settings.characters["hatted-herald-01"];
        assert_eq!(saved.name, "My herald");
        assert_eq!(saved.animation_path, Some(PathBuf::from("C:\\Videos\\my-herald.mp4")));
        assert_eq!(saved.voice, crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() });
        assert_eq!(settings.selected_character.as_deref(), Some("hatted-herald-01"));
        let serialized = serde_json::to_value(&settings).unwrap();
        assert!(serialized["characters"]["hatted-herald-01"].get("sampleText").is_none());
        assert!(serialized["characters"]["hatted-herald-01"].get("voiceDescription").is_none());
        let data = std::env::temp_dir().join(format!("herald-legacy-sample-{}", crate::state::timestamp()));
        settings.save(&data).unwrap();
        let saved_json: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
        let saved_character = &saved_json["characters"]["hatted-herald-01"];
        assert!(saved_character.get("sampleText").is_none());
        assert_eq!(saved_character["name"], "My herald");
        assert!(saved_character.get("voiceDescription").is_none());
        assert_eq!(saved_character["animationPath"], "C:\\Videos\\my-herald.mp4");
        assert_eq!(saved_character["voice"]["voiceId"], "saved-voice");
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn deleting_a_bundled_character_survives_save_load_and_reload() {
        let data = std::env::temp_dir().join(format!("herald-bundled-delete-{}", crate::state::timestamp()));
        let deleted = "hatted-herald-01";
        let mut settings = Settings::default();
        settings.characters.remove(deleted);
        settings.save(&data).unwrap();
        assert!(!Settings::load(&data).unwrap().characters.contains_key(deleted));
        let mut store = Store::new(&data).unwrap();
        assert!(!store.current.characters.contains_key(deleted));
        settings.quiet_mode = true;
        settings.save(&data).unwrap();
        assert!(store.reload().unwrap());
        assert!(!store.current.characters.contains_key(deleted));
        drop(store);
        assert!(!Store::new(&data).unwrap().current.characters.contains_key(deleted));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn speech_preferences_persist_and_legacy_gpu_is_ignored() {
        let defaults = Settings::decode(br#"{"useGpu":true}"#).unwrap();
        assert_eq!(defaults.speech_model, crate::elevenlabs::SpeechModel::Flash);
        let settings = Settings { elevenlabs_api_key: Some("test-key".into()), speech_model: crate::elevenlabs::SpeechModel::V4Turbo, ..Settings::default() };
        assert_eq!(Settings::decode(&serde_json::to_vec(&settings).unwrap()).unwrap(), settings);
        #[cfg(target_os = "windows")]
        {
            let json = serde_json::to_string(&settings).unwrap();
            assert!(json.contains("dpapi:"));
            assert!(!json.contains("test-key"));
            assert!(Settings::decode(br#"{"elevenlabsApiKey":"dpapi:not-base64"}"#).is_err());
        }
        assert!(Settings::decode(br#"{"elevenlabsApiKey":"bad\nkey"}"#).is_err());
        assert!(Settings::decode(br#"{"speechModel":"unknown"}"#).is_err());
        assert!(Settings::decode(br#"{"installedBundledCharacters":["bad id"]}"#).is_err());
        assert!(serde_json::to_value(defaults).unwrap().get("useGpu").is_none());
    }

    #[test]
    fn default_voice_id_migrates_persists_and_validates_with_saved_preferences() {
        let data = std::env::temp_dir().join(format!("herald-default-voice-{}-{}", std::process::id(), crate::state::timestamp()));
        std::fs::create_dir_all(&data).unwrap();
        let old = serde_json::json!({
            "voices": {"claude": "Mark", "opencode": "Luna"},
            "characters": {
                "saved-herald": {
                    "name": "Saved herald",
                    "voiceDescription": "A warm saved voice",
                    "animationPath": "C:\\Videos\\saved-herald.mp4",
                    "voice": {"type": "elevenLabs", "voiceId": "saved-voice"}
                }
            },
            "selectedCharacter": "saved-herald"
        });
        std::fs::write(data.join("settings.json"), serde_json::to_vec(&old).unwrap()).unwrap();

        let mut settings = Settings::load(&data).unwrap();
        assert_eq!(settings.default_voice_id, "JBFqnCBsd6RMkjVDRZzb");
        assert_eq!(settings.voices.get("claude").map(String::as_str), Some("Mark"));
        assert_eq!(settings.voices.get("opencode").map(String::as_str), Some("Luna"));
        assert_eq!(settings.characters["saved-herald"].voice, crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() });

        settings.default_voice_id = "configured-default".into();
        settings.save(&data).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["defaultVoiceId"], "configured-default");

        let restored = Settings::load(&data).unwrap();
        assert_eq!(restored.default_voice_id, "configured-default");
        assert_eq!(restored.voices.get("claude").map(String::as_str), Some("Mark"));
        assert_eq!(restored.voices.get("opencode").map(String::as_str), Some("Luna"));
        assert_eq!(restored.characters["saved-herald"].voice, crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() });

        for invalid in [serde_json::json!({"defaultVoiceId": ""}), serde_json::json!({"defaultVoiceId": "bad voice"})] {
            std::fs::write(data.join("settings.json"), serde_json::to_vec(&invalid).unwrap()).unwrap();
            assert!(Settings::load(&data).is_err());
        }
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn volume_is_linear_in_decibels_with_true_mute_and_full_scale() {
        assert_eq!(volume_gain(0), 0.0);
        assert_eq!(volume_gain(100), 1.0);
        assert!((volume_gain(50) - 0.03162277660168379).abs() < 1e-12);
        for volume in 1..100 {
            assert!(volume_gain(volume + 1) > volume_gain(volume));
            assert!((20.0 * (volume_gain(volume + 1) / volume_gain(volume)).log10() - 0.6).abs() < 1e-10);
        }
    }

    #[test]
    fn schedules_support_minutes_overnight_and_manual_quiet() {
        let mut settings = Settings { quiet_start: 22 * 60 + 30, quiet_end: 8 * 60 + 15, ..Settings::default() };
        assert!(!settings.quiet_at(22 * 60 + 29));
        assert!(settings.quiet_at(22 * 60 + 30));
        assert!(settings.quiet_at(8 * 60 + 14));
        assert!(!settings.quiet_at(8 * 60 + 15));
        settings.schedule_enabled = false;
        assert!(!settings.quiet_at(23 * 60));
        settings.quiet_mode = true;
        assert!(settings.quiet_at(12 * 60));
        settings.quiet_mode = false;
        settings.schedule_enabled = true;
        settings.quiet_start = 9 * 60;
        settings.quiet_end = 17 * 60;
        assert!(settings.quiet_at(12 * 60));
        assert!(!settings.quiet_at(18 * 60));
    }

    #[test]
    fn migrates_legacy_hours_and_validates_preferences() {
        let settings = Settings::decode(br#"{"nightStart":0,"nightEnd":24,"voices":{"claude":"Mark"}}"#).unwrap();
        assert!(settings.quiet_at(1439));
        assert_eq!(settings.voices["claude"], "Mark");
        assert_eq!(settings.volume, 100);
        assert!(Settings::decode(br#"{"volume":101}"#).is_err());
        assert!(Settings::decode(br#"{"quietStart":1440}"#).is_err());
        assert_eq!(parse_time("22:30", false).unwrap(), 1350);
        assert!(parse_time("24:00", false).is_err());
        assert_eq!(parse_time("24:00", true).unwrap(), 1440);
        assert!(parse_time("08:60", true).is_err());
        assert_eq!(format_time(495), "08:15");
    }

    #[test]
    fn character_registry_and_selected_profile_persist_with_legacy_preferences() {
        let settings = Settings::decode(br#"{
            "voices":{"opencode":"Luna"},
            "characters":{"herald":{"name":"Herald","voiceDescription":"A warm herald","animationPath":"C:\\Videos\\herald.mp4","voice":{"type":"elevenLabs","voiceId":"saved-herald"}}},
            "selectedCharacter":"herald"
        }"#).unwrap();
        assert_eq!(settings.voices["opencode"], "Luna");
        assert_eq!(settings.selected_character.as_deref(), Some("herald"));
        assert_eq!(settings.characters["herald"].name, "Herald");
        let bytes = serde_json::to_vec(&settings).unwrap();
        let restored = Settings::decode(&bytes).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn selected_profile_must_exist() {
        let mut settings = Settings::default();
        settings.selected_character = Some("missing".into());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn hot_reload_keeps_last_good_settings_and_saves_existing_files() {
        let data = std::env::temp_dir().join("opencode").join(format!("herald-settings-{}", std::process::id()));
        let mut settings = Settings::default();
        settings.save(&data).unwrap();
        let mut store = Store::new(&data).unwrap();
        settings.quiet_mode = true;
        settings.volume = 35;
        settings.output_device = Some("speaker-id".into());
        settings.save(&data).unwrap();
        assert!(store.reload().unwrap());
        assert_eq!(store.current, settings);
        std::fs::write(data.join("settings.json"), "{").unwrap();
        assert!(store.reload().is_err());
        assert_eq!(store.current, settings);
        assert!(!store.reload().unwrap());
        settings.save(&data).unwrap();
        assert!(store.reload().unwrap());
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn startup_restart_preserves_complete_saved_preferences_without_rewriting_settings() {
        let data = std::env::temp_dir().join(format!("herald-settings-restart-{}-{}", std::process::id(), crate::state::timestamp()));
        let saved_bytes = br#"{
            "quietMode":true,"scheduleEnabled":true,"quietStart":1305,"quietEnd":390,
            "volume":35,"outputDevice":"saved-output-device","elevenlabsApiKey":"saved-api-key",
            "speechModel":"eleven_v4_turbo","defaultVoiceId":"configured-default",
            "voices":{"claude":"Mark","opencode":"Luna"},
            "characters":{
                "saved-herald":{"name":"Saved herald","voiceDescription":"A warm saved voice","animationPath":"C:\\Videos\\saved-herald.mp4","voice":{"type":"elevenLabs","voiceId":"saved-voice"}},
                "local-herald":{"name":"Local herald","voiceDescription":"Old local description","animationPath":null,"voice":{"type":"local","speaker":"Jasper"}}
            },
            "selectedCharacter":"saved-herald","installedBundledCharacters":["hatted-herald-01"]
        }"#;
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("settings.json"), saved_bytes).unwrap();
        let settings = Settings::load(&data).unwrap();
        assert_eq!(std::fs::read(data.join("settings.json")).unwrap(), saved_bytes);

        for _ in 0..2 {
            let mut store = Store::new(&data).unwrap();
            assert_eq!(store.current, settings);
            assert!(!store.reload().unwrap());
            assert!(store.current.quiet_mode);
            assert!(store.current.schedule_enabled);
            assert_eq!(store.current.quiet_start, 1305);
            assert_eq!(store.current.quiet_end, 390);
            assert_eq!(store.current.volume, 35);
            assert_eq!(store.current.output_device.as_deref(), Some("saved-output-device"));
            assert_eq!(store.current.elevenlabs_api_key.as_deref(), Some("saved-api-key"));
            assert_eq!(store.current.speech_model, crate::elevenlabs::SpeechModel::V4Turbo);
            assert_eq!(store.current.voices.get("claude").map(String::as_str), Some("Mark"));
            assert_eq!(store.current.voices.get("opencode").map(String::as_str), Some("Luna"));
            assert_eq!(store.current.selected_character.as_deref(), Some("saved-herald"));
            assert_eq!(store.current.characters["saved-herald"].name, "Saved herald");
            assert_eq!(store.current.characters["saved-herald"].animation_path, Some(PathBuf::from("C:\\Videos\\saved-herald.mp4")));
            assert_eq!(store.current.characters["saved-herald"].voice, crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() });
            assert_eq!(store.current.default_voice_id, "configured-default");
            assert_eq!(store.current.characters["local-herald"].voice, crate::characters::CharacterVoice::Local { speaker: Some("Jasper".into()) });
            assert!(!store.current.characters.contains_key("hatted-herald-01"));
            assert_eq!(std::fs::read(data.join("settings.json")).unwrap(), saved_bytes);
        }
        let mut store = Store::new(&data).unwrap();
        let reloaded_bytes = String::from_utf8(saved_bytes.to_vec()).unwrap().replace("\"volume\":35", "\"volume\":36");
        std::fs::write(data.join("settings.json"), &reloaded_bytes).unwrap();
        assert!(store.reload().unwrap());
        assert_eq!(store.current.volume, 36);
        assert_eq!(std::fs::read(data.join("settings.json")).unwrap(), reloaded_bytes.as_bytes());
        store.current.save(&data).unwrap();
        let saved_json: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
        assert!(saved_json["characters"].as_object().unwrap().values().all(|character| character.get("voiceDescription").is_none()));
        assert_eq!(saved_json["characters"]["saved-herald"]["name"], "Saved herald");
        assert_eq!(saved_json["characters"]["saved-herald"]["animationPath"], "C:\\Videos\\saved-herald.mp4");
        assert_eq!(saved_json["characters"]["saved-herald"]["voice"]["voiceId"], "saved-voice");
        assert_eq!(saved_json["characters"]["local-herald"]["voice"]["speaker"], "Jasper");
        assert_eq!(saved_json["defaultVoiceId"], "configured-default");
        assert_eq!(saved_json["selectedCharacter"], "saved-herald");
        assert!(saved_json["characters"].get("hatted-herald-01").is_none());
        assert_eq!(Settings::load(&data).unwrap(), store.current);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn announcement_defaults_use_the_json_contract() {
        let settings = Settings::default();
        assert_eq!(settings.announcement_body_font, FontPreference::new("Century Gothic", 18));
        assert_eq!(settings.announcement_title_font, FontPreference::new("Century Gothic", 14));
        assert_eq!(settings.summary_prompt, DEFAULT_SUMMARY_PROMPT);
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["announcementBodyFont"]["family"], "Century Gothic");
        assert_eq!(value["announcementBodyFont"]["size"], 18);
        assert_eq!(value["announcementTitleFont"]["family"], "Century Gothic");
        assert_eq!(value["announcementTitleFont"]["size"], 14);
        assert_eq!(value["summaryPrompt"], DEFAULT_SUMMARY_PROMPT);
        let restored = Settings::decode(br#"{}"#).unwrap();
        assert_eq!(restored.announcement_body_font, settings.announcement_body_font);
        assert_eq!(restored.announcement_title_font, settings.announcement_title_font);
        assert_eq!(restored.summary_prompt, settings.summary_prompt);
    }

    #[test]
    fn announcement_preferences_persist_without_changing_prompt_text() {
        let data = std::env::temp_dir().join(format!("herald-announcement-settings-{}", crate::state::timestamp()));
        let mut settings = Settings::default();
        settings.announcement_body_font = FontPreference::new("A font that is not installed", 32);
        settings.announcement_title_font = FontPreference::new("Another unavailable font", 8);
        settings.summary_prompt = "Report what was done.\nBe explicit and concise.".into();
        settings.save(&data).unwrap();
        let restored = Settings::load(&data).unwrap();
        assert_eq!(restored, settings);
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
        assert_eq!(json["summaryPrompt"], settings.summary_prompt);
        assert_eq!(json["announcementBodyFont"]["family"], settings.announcement_body_font.family);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn announcement_preferences_validate_sizes_and_summary_prompt_contract() {
        let mut settings = Settings::default();
        for size in [MIN_FONT_SIZE - 1, MAX_FONT_SIZE + 1] {
            settings.announcement_body_font.size = size;
            assert!(settings.validate().is_err());
            settings.announcement_body_font.size = FontPreference::default().size;
            settings.announcement_title_font.size = size;
            assert!(settings.validate().is_err());
            settings.announcement_title_font.size = 14;
        }
        settings.summary_prompt = "".into();
        assert!(settings.validate().is_err());
        settings.summary_prompt = "Report the task outcome.".into();
        assert!(settings.validate().is_ok());
        settings.summary_prompt = "Contains\0prompt".into();
        assert!(settings.validate().is_err());
        settings.summary_prompt = "x".repeat(16_385);
        assert!(settings.validate().is_err());
        settings.summary_prompt = "\u{1f4e3} Summarize the completed task.".into();
        assert!(settings.validate().is_ok());
    }
}
