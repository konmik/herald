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
    let assets = Path::new(if cfg!(target_os = "windows") { "C:\\Library\\resources" } else { "/opt/herald/resources" });
    let custom = if cfg!(target_os = "windows") { "C:\\Custom Ω\\herald.mp4" } else { "/home/example/Custom Ω/herald.mp4" };
    assert_eq!(video_picker_path(custom, assets), PathBuf::from(custom));
    assert_eq!(video_picker_path("videos/herald.mp4", assets), assets.join("videos/herald.mp4"));
    assert_eq!(video_picker_path("Choose video…", assets), assets.join("videos"));
    assert_eq!(video_picker_path("", assets), assets.join("videos"));
    #[cfg(target_os = "windows")]
    {
        let installed_assets = Path::new("C:\\Library\\bin\\..\\resources");
        assert_eq!(video_picker_path("Choose video…", installed_assets), assets.join("videos"));
        assert_eq!(video_picker_path("C:\\Library\\bin\\..\\resources\\videos/herald.mp4", installed_assets), assets.join("videos/herald.mp4"));
    }
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
