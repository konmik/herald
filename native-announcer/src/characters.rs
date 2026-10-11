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
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

impl CharacterVoice {
    fn is_default(&self) -> bool {
        matches!(self, Self::Local { speaker: None })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Character {
    pub name: String,
    pub selected: bool,
    pub animation_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary_prompt: String,
    #[serde(skip_serializing_if = "CharacterVoice::is_default")]
    pub voice: CharacterVoice,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            name: String::new(),
            selected: false,
            animation_path: None,
            summary_prompt: String::new(),
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
    pub source_character: String,
    pub local_speaker: Option<String>,
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

pub fn resolve(settings: &Settings, assets: &Path, source_character: &str, character_id: Option<&str>) -> ResolvedCharacter {
    let local_speaker = settings.voices.get(source_character).cloned();
    let default_video = crate::video::select_path(assets, source_character);
    let character_id = character_id.or_else(|| {
        use std::hash::{BuildHasher, Hasher};
        let selected = settings.characters.values().any(|character| character.selected);
        let candidates: Vec<_> = settings.characters.iter().filter(|(_, character)| !selected || character.selected).collect();
        if candidates.is_empty() { return None; }
        let random = std::collections::hash_map::RandomState::new().build_hasher().finish();
        Some(candidates[random as usize % candidates.len()].0.as_str())
    });
    if let Some(id) = character_id {
        if let Some(character) = settings.characters.get(id) {
            let resolved_animation = character.animation_path.as_ref().map(|path| animation_path(id, path, assets));
            let (video_path, video_warning) = match resolved_animation.as_ref() {
                Some(path) if path.is_file() => (path.clone(), None),
                Some(path) => (
                    default_video.clone(),
                    Some(format!("Character video is unavailable: {}", path.display())),
                ),
                None => (default_video, None),
            };
            let voice = match &character.voice {
                CharacterVoice::Local { speaker } => ResolvedVoice::Local { speaker: speaker.clone() },
                CharacterVoice::ElevenLabs { voice_id } => ResolvedVoice::ElevenLabs { voice_id: voice_id.clone() },
            };
            return ResolvedCharacter {
                id: Some(id.to_owned()),
                name: character.name.clone(),
                voice,
                source_character: source_character.to_owned(),
                local_speaker,
                video_path,
                video_warning,
            };
        }
    }
    ResolvedCharacter {
        id: None,
        name: source_character.to_owned(),
        voice: ResolvedVoice::Local { speaker: local_speaker.clone() },
        source_character: source_character.to_owned(),
        local_speaker,
        video_path: default_video,
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
        if !character.summary_prompt.is_empty() {
            crate::settings::validate_summary_prompt(&character.summary_prompt)?;
        }
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
#[path = "../unit_tests/characters.rs"]
mod tests;
