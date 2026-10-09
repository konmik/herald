use crate::characters::{animation_path, Character, CharacterVoice};
use crate::settings::{format_time, parse_time, Settings};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) struct SettingsDraft {
    pub(super) settings: Settings,
    pub(super) characters: BTreeMap<String, Character>,
    pub(super) removed_characters: BTreeSet<String>,
    pub(super) selected_character: Option<String>,
    pub(super) quiet_start_text: String,
    pub(super) quiet_end_text: String,
}

impl SettingsDraft {
    pub(super) fn new(settings: Settings) -> Self {
        Self {
            quiet_start_text: format_time(settings.quiet_start),
            quiet_end_text: format_time(settings.quiet_end),
            selected_character: settings.selected_character.clone(),
            characters: settings.characters.clone(),
            removed_characters: BTreeSet::new(),
            settings,
        }
    }

    pub(super) fn candidate(&self) -> Result<Settings, String> {
        let mut settings = self.settings.clone();
        settings.quiet_start = parse_time(&self.quiet_start_text, false)?;
        settings.quiet_end = parse_time(&self.quiet_end_text, true)?;
        settings.characters = self.settings.characters.clone();
        for (id, character) in &self.characters {
            if character.name.trim().is_empty()
                && !self.settings.characters.contains_key(id)
                && self.selected_character.as_deref() != Some(id.as_str())
            {
                continue;
            }
            settings.characters.insert(id.clone(), character.clone());
        }
        for id in &self.removed_characters {
            settings.characters.remove(id);
        }
        settings.selected_character = self
            .selected_character
            .clone()
            .filter(|id| settings.characters.contains_key(id));
        Ok(settings)
    }

    pub(super) fn commit(&mut self, settings: Settings) {
        *self = Self::new(settings);
    }

    pub(super) fn character(&mut self, id: &str) -> &mut Character {
        self.characters.entry(id.to_owned()).or_default()
    }

    pub(super) fn remove_character(&mut self, id: &str) {
        self.characters.remove(id);
        if self.settings.characters.contains_key(id) {
            self.removed_characters.insert(id.to_owned());
        }
        if self.selected_character.as_deref() == Some(id) {
            self.selected_character = None;
        }
    }
}

pub(super) fn voice_id_text(voice: &CharacterVoice) -> &str {
    match voice {
        CharacterVoice::Local { .. } => "",
        CharacterVoice::ElevenLabs { voice_id } => voice_id,
    }
}

pub(super) fn capture_draft_voice(draft: &mut Character, entered_id: &str) {
    let voice_id = entered_id.trim();
    if !voice_id.is_empty() {
        if !matches!(&draft.voice, CharacterVoice::ElevenLabs { voice_id: current } if current == voice_id) {
            draft.selected = true;
        }
        draft.voice = CharacterVoice::ElevenLabs { voice_id: voice_id.to_owned() };
    } else if matches!(draft.voice, CharacterVoice::ElevenLabs { .. }) {
        draft.voice = CharacterVoice::default();
    }
}

pub(super) fn video_picker_path(video: &str, assets: &Path) -> PathBuf {
    let path = if video.is_empty() || video == "Choose video…" {
        assets.join("videos")
    } else {
        let path = PathBuf::from(video);
        if path.is_absolute() { path } else { assets.join(path) }
    };
    std::path::absolute(&path).unwrap_or(path)
}

pub(super) fn capture_draft_video(id: &str, character: &mut Character, text: &str, assets: &Path) {
    if text.is_empty() || text == "Choose video…" { return; }
    let path = PathBuf::from(text);
    if character.animation_path.as_ref().is_some_and(|stored| animation_path(id, stored, assets) == path) { return; }
    character.animation_path = Some(path);
}

pub(super) fn character_video(id: &str, character: &Character, assets: &Path) -> String {
    character
        .animation_path
        .as_ref()
        .map(|path| animation_path(id, path, assets).to_string_lossy().into_owned())
        .unwrap_or_else(|| "Choose video…".into())
}

pub(super) fn new_character_id(characters: &BTreeMap<String, Character>) -> String {
    let timestamp = crate::state::timestamp();
    let mut id = format!("character-{timestamp}");
    let mut suffix = 1;
    while characters.contains_key(&id) {
        id = format!("character-{timestamp}-{suffix}");
        suffix += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_the_resolved_video_keeps_the_portable_bundled_path() {
        let assets = Path::new("C:/herald/resources");
        let mut character = Character { animation_path: Some(PathBuf::from("videos/royal.mp4")), ..Character::default() };
        let displayed = character_video("royal", &character, assets);
        capture_draft_video("royal", &mut character, &displayed, assets);
        assert_eq!(character.animation_path, Some(PathBuf::from("videos/royal.mp4")));
        capture_draft_video("royal", &mut character, "D:/custom/new.mp4", assets);
        assert_eq!(character.animation_path, Some(PathBuf::from("D:/custom/new.mp4")));
    }

    fn empty_draft() -> Character {
        Character::default()
    }

    #[test]
    fn video_picker_starts_at_the_current_video_or_library() {
        let assets = Path::new("C:\\Library\\resources");
        assert_eq!(video_picker_path("C:\\Custom Ω\\herald.mp4", assets), PathBuf::from("C:\\Custom Ω\\herald.mp4"));
        assert_eq!(video_picker_path("videos/herald.mp4", assets), assets.join("videos/herald.mp4"));
        assert_eq!(video_picker_path("Choose video…", assets), assets.join("videos"));
        assert_eq!(video_picker_path("", assets), assets.join("videos"));
        let installed_assets = Path::new("C:\\Library\\bin\\..\\resources");
        assert_eq!(video_picker_path("Choose video…", installed_assets), assets.join("videos"));
        assert_eq!(video_picker_path("C:\\Library\\bin\\..\\resources\\videos/herald.mp4", installed_assets), assets.join("videos/herald.mp4"));
    }

    #[test]
    fn entering_a_voice_id_selects_the_character_without_overriding_manual_deselection() {
        let mut draft = empty_draft();
        assert!(!draft.selected);
        capture_draft_voice(&mut draft, "own-voice");
        assert!(draft.selected);
        draft.selected = false;
        capture_draft_voice(&mut draft, "own-voice");
        assert!(!draft.selected);
        capture_draft_voice(&mut draft, "replacement-voice");
        assert!(draft.selected);
    }

    #[test]
    fn entered_voice_id_is_trimmed_and_clearing_restores_local() {
        let mut draft = empty_draft();
        capture_draft_voice(&mut draft, "  own-voice_123  ");
        assert_eq!(draft.voice, CharacterVoice::ElevenLabs { voice_id: "own-voice_123".into() });
        capture_draft_voice(&mut draft, "  ");
        assert_eq!(draft.voice, CharacterVoice::Local { speaker: None });
    }

    #[test]
    fn local_speaker_is_preserved_until_an_id_is_entered() {
        let mut draft = empty_draft();
        draft.voice = CharacterVoice::Local { speaker: Some("Luna".into()) };
        capture_draft_voice(&mut draft, "");
        assert_eq!(draft.voice, CharacterVoice::Local { speaker: Some("Luna".into()) });
        capture_draft_voice(&mut draft, "own-voice");
        assert_eq!(draft.voice, CharacterVoice::ElevenLabs { voice_id: "own-voice".into() });
    }

    #[test]
    fn name_and_video_edits_keep_the_entered_voice_id() {
        let mut draft = empty_draft();
        capture_draft_voice(&mut draft, "own-voice");
        draft.name = "Renamed character".into();
        draft.animation_path = Some(PathBuf::from("changed.mp4"));
        assert_eq!(voice_id_text(&draft.voice), "own-voice");
    }

    #[test]
    fn draft_retention_keeps_invalid_character_edits_and_unmodified_preferences() {
        let mut settings = Settings::default();
        settings.volume = 37;
        let mut draft = SettingsDraft::new(settings.clone());
        draft.characters.insert("new-character".into(), Character::default());
        draft.character("new-character").name = "".into();
        draft.quiet_start_text = "bad".into();
        assert!(draft.candidate().is_err());
        assert_eq!(draft.settings.volume, 37);
        assert_eq!(settings.voices, draft.settings.voices);
    }
}
