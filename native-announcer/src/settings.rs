use crate::characters::{validate_registry, Character};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub quiet_mode: bool,
    pub schedule_enabled: bool,
    pub quiet_start: u32,
    pub quiet_end: u32,
    pub volume: u16,
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
            output_device: None,
            elevenlabs_api_key: None,
            speech_model: crate::elevenlabs::SpeechModel::default(),
            default_voice_id: crate::elevenlabs::DEFAULT_VOICE_ID.into(),
            voices: HashMap::new(),
            characters: BTreeMap::new(),
            selected_character: None,
            installed_bundled_characters: BTreeSet::new(),
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
        if let Some(key) = &self.elevenlabs_api_key {
            crate::elevenlabs::validate_api_key(key)?;
        }
        crate::characters::validate_voice_id(&self.default_voice_id)?;
        for id in &self.installed_bundled_characters {
            crate::characters::validate_id(id)?;
        }
        validate_registry(&self.characters, self.selected_character.as_deref())?;
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

#[cfg(any(target_os = "windows", test))]
pub fn parse_time(text: &str, allow_midnight_end: bool) -> Result<u32, String> {
    let (hour, minute) = text.trim().split_once(':').ok_or("Use HH:MM for times.")?;
    let hour = hour.parse::<u32>().map_err(|_| "Use HH:MM for times.")?;
    let minute = minute.parse::<u32>().map_err(|_| "Use HH:MM for times.")?;
    if minute >= 60 || hour > 24 || (hour == 24 && (!allow_midnight_end || minute != 0)) {
        return Err("Use a time between 00:00 and 23:59 (24:00 is allowed for the end).".into());
    }
    Ok(hour * 60 + minute)
}

#[cfg(any(target_os = "windows", test))]
pub fn format_time(minute: u32) -> String { format!("{:02}:{:02}", minute / 60, minute % 60) }

pub fn volume_gain(volume: u16) -> f64 {
    if volume == 0 { 0.0 } else { 10_f64.powf((-60.0 + 0.6 * f64::from(volume.min(100))) / 20.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_seed_the_bundled_character_catalog() {
        let settings = Settings::default();
        let ids: BTreeSet<_> = crate::characters::bundled_characters().keys().cloned().collect();
        assert_eq!(ids.len(), 11);
        assert_eq!(settings.installed_bundled_characters, ids);
        assert_eq!(settings.characters.keys().cloned().collect::<BTreeSet<_>>(), ids);
        assert!(settings.characters.values().all(|character| matches!(character.voice, crate::characters::CharacterVoice::Local { speaker: None }) && character.animation_path.as_ref().is_some_and(|path| !path.is_absolute())));
    }

    #[test]
    fn legacy_sample_text_is_ignored_and_dropped_when_saved() {
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
        assert_eq!(saved.voice_description, "A saved custom voice");
        assert_eq!(saved.animation_path, Some(PathBuf::from("C:\\Videos\\my-herald.mp4")));
        assert_eq!(saved.voice, crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() });
        assert_eq!(settings.selected_character.as_deref(), Some("hatted-herald-01"));
        assert_eq!(settings.characters.len(), 11);
        assert_eq!(settings.installed_bundled_characters.len(), 11);
        let serialized = serde_json::to_value(&settings).unwrap();
        assert!(serialized["characters"]["hatted-herald-01"].get("sampleText").is_none());
        let data = std::env::temp_dir().join(format!("civilized-legacy-sample-{}", crate::state::timestamp()));
        settings.save(&data).unwrap();
        let saved_json: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
        let saved_character = &saved_json["characters"]["hatted-herald-01"];
        assert!(saved_character.get("sampleText").is_none());
        assert_eq!(saved_character["name"], "My herald");
        assert_eq!(saved_character["voiceDescription"], "A saved custom voice");
        assert_eq!(saved_character["animationPath"], "C:\\Videos\\my-herald.mp4");
        assert_eq!(saved_character["voice"]["voiceId"], "saved-voice");
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn deleting_a_bundled_character_survives_save_load_and_reload() {
        let data = std::env::temp_dir().join(format!("civilized-bundled-delete-{}", crate::state::timestamp()));
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
        let data = std::env::temp_dir().join(format!("civilized-default-voice-{}-{}", std::process::id(), crate::state::timestamp()));
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
        let data = std::env::temp_dir().join("opencode").join(format!("civilized-settings-{}", std::process::id()));
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
        let data = std::env::temp_dir().join(format!("civilized-settings-restart-{}-{}", std::process::id(), crate::state::timestamp()));
        let mut settings = Settings::default();
        settings.quiet_mode = true;
        settings.schedule_enabled = true;
        settings.quiet_start = 21 * 60 + 45;
        settings.quiet_end = 6 * 60 + 30;
        settings.volume = 35;
        settings.output_device = Some("saved-output-device".into());
        settings.elevenlabs_api_key = Some("saved-api-key".into());
        settings.speech_model = crate::elevenlabs::SpeechModel::V4Turbo;
        settings.voices.insert("claude".into(), "Mark".into());
        settings.voices.insert("opencode".into(), "Luna".into());
        settings.characters.insert("saved-herald".into(), Character {
            name: "Saved herald".into(),
            voice_description: "A warm saved voice".into(),
            animation_path: Some(PathBuf::from("C:\\Videos\\saved-herald.mp4")),
            voice: crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() },
        });
        settings.selected_character = Some("saved-herald".into());
        settings.save(&data).unwrap();
        let saved_bytes = std::fs::read(data.join("settings.json")).unwrap();

        for _ in 0..2 {
            let store = Store::new(&data).unwrap();
            assert_eq!(store.current, settings);
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
            assert_eq!(store.current.characters["saved-herald"].voice_description, "A warm saved voice");
            assert_eq!(store.current.characters["saved-herald"].animation_path, Some(PathBuf::from("C:\\Videos\\saved-herald.mp4")));
            assert_eq!(store.current.characters["saved-herald"].voice, crate::characters::CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() });
            assert_eq!(std::fs::read(data.join("settings.json")).unwrap(), saved_bytes);
        }
        std::fs::remove_dir_all(data).unwrap();
    }
}
