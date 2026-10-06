use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static BUNDLED_CHARACTERS: OnceLock<BTreeMap<String, Character>> = OnceLock::new();

pub fn bundled_characters() -> &'static BTreeMap<String, Character> {
    BUNDLED_CHARACTERS.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/characters.json")).expect("Bundled character catalog is invalid")
    })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CharacterVoice {
    Local {
        #[serde(default)]
        speaker: Option<String>,
    },
    #[serde(alias = "elevenlabs")]
    ElevenLabs {
        #[serde(rename = "voiceId")]
        voice_id: String,
    },
}

impl Default for CharacterVoice {
    fn default() -> Self {
        Self::Local { speaker: None }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Character {
    pub name: String,
    pub voice_description: String,
    pub sample_text: String,
    pub animation_path: Option<PathBuf>,
    pub voice: CharacterVoice,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            name: String::new(),
            voice_description: String::new(),
            sample_text: String::new(),
            animation_path: None,
            voice: CharacterVoice::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedVoice {
    Local { speaker: Option<String> },
    ElevenLabs { voice_id: String },
}

#[derive(Clone, Debug)]
pub struct ResolvedCharacter {
    pub id: Option<String>,
    pub name: String,
    pub voice: ResolvedVoice,
    pub fallback_character: String,
    pub fallback_speaker: Option<String>,
    pub video_path: PathBuf,
    pub video_warning: Option<String>,
}

pub fn animation_path(id: &str, path: &Path, assets: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else if bundled_characters().get(id).and_then(|character| character.animation_path.as_deref()) == Some(path) {
        assets.join(path)
    } else {
        path.to_owned()
    }
}

impl ResolvedCharacter {
    pub fn history_identity(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.name)
    }
}

pub fn resolve(settings: &Settings, assets: &Path, source_character: &str) -> ResolvedCharacter {
    let fallback_speaker = settings.voices.get(source_character).cloned();
    let fallback_video = crate::video::select_path(assets, source_character);
    if let Some(id) = settings.selected_character.as_deref() {
        if let Some(character) = settings.characters.get(id) {
            let resolved_animation = character.animation_path.as_ref().map(|path| animation_path(id, path, assets));
            let (video_path, video_warning) = match resolved_animation.as_ref() {
                Some(path) if path.is_file() => (path.clone(), None),
                Some(path) => (
                    fallback_video.clone(),
                    Some(format!("Character video is unavailable: {}", path.display())),
                ),
                None => (fallback_video, None),
            };
            let voice = match &character.voice {
                CharacterVoice::Local { speaker } => ResolvedVoice::Local { speaker: speaker.clone() },
                CharacterVoice::ElevenLabs { voice_id } => ResolvedVoice::ElevenLabs { voice_id: voice_id.clone() },
            };
            return ResolvedCharacter {
                id: Some(id.to_owned()),
                name: character.name.clone(),
                voice,
                fallback_character: source_character.to_owned(),
                fallback_speaker,
                video_path,
                video_warning,
            };
        }
    }
    ResolvedCharacter {
        id: None,
        name: source_character.to_owned(),
        voice: ResolvedVoice::Local { speaker: fallback_speaker.clone() },
        fallback_character: source_character.to_owned(),
        fallback_speaker,
        video_path: fallback_video,
        video_warning: None,
    }
}

pub fn validate_registry(characters: &BTreeMap<String, Character>, selected: Option<&str>) -> Result<(), String> {
    if let Some(selected) = selected {
        if !characters.contains_key(selected) {
            return Err("Selected character does not exist.".into());
        }
    }
    for (id, character) in characters {
        validate_id(id)?;
        validate_name(&character.name)?;
        validate_prose(&character.voice_description, "Character voice description", 1000)?;
        validate_prose(&character.sample_text, "Character sample text", 1000)?;
        if let Some(path) = &character.animation_path {
            validate_path(path)?;
        }
        match &character.voice {
            CharacterVoice::Local { speaker } => {
                if let Some(speaker) = speaker {
                    validate_text(speaker, "Local speaker", 256, false)?;
                }
            }
            CharacterVoice::ElevenLabs { voice_id } => validate_voice_id(voice_id)?,
        }
    }
    Ok(())
}

pub fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 || !id.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_alphanumeric() || byte == b'_' || (byte == b'-' && index > 0)
    }) {
        return Err("Character IDs must use letters, numbers, hyphens, and underscores.".into());
    }
    Ok(())
}

pub fn validate_voice_id(voice_id: &str) -> Result<(), String> {
    if voice_id.is_empty() || voice_id.len() > 256 || !voice_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err("ElevenLabs voice ID is invalid.".into());
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    validate_text(name, "Character name", 160, true)
}

fn validate_text(value: &str, label: &str, max: usize, required: bool) -> Result<(), String> {
    let trimmed = value.trim();
    if required && trimmed.is_empty() {
        return Err(format!("{label} is required."));
    }
    if value.len() > max || value.chars().any(char::is_control) {
        return Err(format!("{label} is invalid."));
    }
    Ok(())
}

fn validate_prose(value: &str, label: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max || value.chars().any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t')) {
        return Err(format!("{label} is invalid."));
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), String> {
    let value = path.to_string_lossy();
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err("Character video path is invalid.".into());
    }
    if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")) {
        return Err("Character video must be an MP4 file.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(voice: CharacterVoice) -> Character {
        Character {
            name: "Royal herald".into(),
            voice_description: "Warm theatrical town crier".into(),
            sample_text: "Hear this announcement from the royal herald.".into(),
            animation_path: Some(PathBuf::from("herald.mp4")),
            voice,
        }
    }

    #[test]
    fn typed_voice_and_selection_round_trip() {
        let mut characters = BTreeMap::new();
        characters.insert("royal-herald".into(), character(CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() }));
        let value = serde_json::to_value(&characters).unwrap();
        assert_eq!(value["royal-herald"]["voice"]["type"], "elevenLabs");
        assert_eq!(serde_json::from_value::<BTreeMap<String, Character>>(value).unwrap(), characters);
        validate_registry(&characters, Some("royal-herald")).unwrap();
    }

    #[test]
    fn missing_custom_video_keeps_remote_voice_and_reports_fallback() {
        let mut settings = Settings::default();
        settings.characters.insert("royal-herald".into(), character(CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() }));
        settings.selected_character = Some("royal-herald".into());
        let resolved = resolve(&settings, Path::new("missing-assets"), "opencode");
        assert_eq!(resolved.voice, ResolvedVoice::ElevenLabs { voice_id: "saved-voice".into() });
        assert!(resolved.video_warning.is_some());
        assert_eq!(resolved.video_path, Path::new("missing-assets/opencode/neutral.mp4"));
    }

    #[test]
    fn bundled_relative_video_uses_the_runtime_asset_root() {
        let directory = std::env::temp_dir().join(format!("civilized-bundled-character-{}", crate::state::timestamp()));
        let video = directory.join("videos/hatted-herald-01.mp4");
        std::fs::create_dir_all(video.parent().unwrap()).unwrap();
        std::fs::write(&video, []).unwrap();
        let mut settings = Settings::default();
        settings.selected_character = Some("hatted-herald-01".into());
        let resolved = resolve(&settings, &directory, "opencode");
        assert_eq!(resolved.video_path, video);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn absolute_custom_video_path_remains_absolute() {
        let directory = std::env::temp_dir().join(format!("civilized-custom-character-{}", crate::state::timestamp()));
        std::fs::create_dir_all(&directory).unwrap();
        let video = directory.join("custom.mp4");
        std::fs::write(&video, []).unwrap();
        let mut settings = Settings::default();
        settings.characters.insert("custom".into(), character(CharacterVoice::default()));
        settings.characters.get_mut("custom").unwrap().animation_path = Some(video.clone());
        settings.selected_character = Some("custom".into());
        let resolved = resolve(&settings, Path::new("relocated-assets"), "opencode");
        assert_eq!(resolved.video_path, video);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_registry_values_are_rejected_at_the_boundary() {
        let mut characters = BTreeMap::new();
        characters.insert("bad id".into(), character(CharacterVoice::default()));
        assert!(validate_registry(&characters, None).is_err());
        characters.clear();
        characters.insert("valid".into(), Character { animation_path: Some(PathBuf::from("voice.wav")), ..character(CharacterVoice::default()) });
        assert!(validate_registry(&characters, None).is_err());
        characters.clear();
        characters.insert("valid".into(), character(CharacterVoice::ElevenLabs { voice_id: "bad id".into() }));
        assert!(validate_registry(&characters, None).is_err());
    }

    #[test]
    fn multiline_prompts_persist_but_names_cannot_contain_control_characters() {
        let mut profile = character(CharacterVoice::default());
        profile.voice_description = "Warm theatrical voice.\nClear diction.".into();
        profile.sample_text = "Hear the herald.\nYour work is ready.".into();
        let mut registry = BTreeMap::from([("herald".into(), profile)]);
        validate_registry(&registry, Some("herald")).unwrap();
        let encoded = serde_json::to_vec(&registry).unwrap();
        let decoded: BTreeMap<String, Character> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded["herald"].voice_description, "Warm theatrical voice.\nClear diction.");
        registry.get_mut("herald").unwrap().name = "Bad\nname".into();
        assert_eq!(validate_registry(&registry, Some("herald")), Err("Character name is invalid.".into()));
    }
}
