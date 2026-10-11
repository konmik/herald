use super::*;

fn character(voice: CharacterVoice) -> Character {
    Character {
        name: "Royal herald".into(),
        selected: false,
        animation_path: Some(PathBuf::from("herald.mp4")),
        summary_prompt: String::new(),
        voice,
    }
}

#[test]
fn random_choices_use_selected_characters_or_the_entire_registry() {
    let mut settings = Settings::default();
    settings.characters = BTreeMap::from([
        ("herald".into(), character(CharacterVoice::default())),
        ("robot".into(), character(CharacterVoice::ElevenLabs { voice_id: "robot-voice".into() })),
    ]);
    settings.selected_character = Some("herald".into());
    for _ in 0..50 {
        let resolved = resolve(&settings, Path::new("missing-assets"), "opencode", None);
        assert!(matches!(resolved.id.as_deref(), Some("herald" | "robot")));
        assert_eq!(resolved.voice, match &settings.characters[resolved.id.as_ref().unwrap()].voice {
            CharacterVoice::Local { speaker } => ResolvedVoice::Local { speaker: speaker.clone() },
            CharacterVoice::ElevenLabs { voice_id } => ResolvedVoice::ElevenLabs { voice_id: voice_id.clone() },
        });
    }
    settings.characters.get_mut("robot").unwrap().selected = true;
    for _ in 0..50 {
        assert_eq!(resolve(&settings, Path::new("missing-assets"), "opencode", None).id.as_deref(), Some("robot"));
    }
    assert_eq!(resolve(&settings, Path::new("missing-assets"), "opencode", Some("herald")).id.as_deref(), Some("herald"));
    settings.characters.clear();
    assert!(resolve(&settings, Path::new("missing-assets"), "opencode", None).id.is_none());
}

#[test]
fn character_prompts_preserve_multiline_text_and_validate_length() {
    let mut characters = BTreeMap::new();
    let mut herald = character(CharacterVoice::default());
    herald.summary_prompt = "Speak as a royal herald.\nReport the outcome briefly.".into();
    herald.selected = true;
    characters.insert("herald".into(), herald);
    validate_registry(&characters, None).unwrap();
    let restored: BTreeMap<String, Character> = serde_json::from_slice(&serde_json::to_vec(&characters).unwrap()).unwrap();
    assert_eq!(restored["herald"].summary_prompt, "Speak as a royal herald.\nReport the outcome briefly.");
    assert!(restored["herald"].selected);
    characters.get_mut("herald").unwrap().summary_prompt = "😀".repeat(16_384);
    validate_registry(&characters, None).unwrap();
    characters.get_mut("herald").unwrap().summary_prompt.push('x');
    assert!(validate_registry(&characters, None).is_err());
    characters.get_mut("herald").unwrap().summary_prompt = "Bad\0prompt".into();
    assert!(validate_registry(&characters, None).is_err());
    characters.get_mut("herald").unwrap().summary_prompt.clear();
    validate_registry(&characters, None).unwrap();
}

#[test]
fn typed_voice_and_selection_round_trip() {
    let mut characters = BTreeMap::new();
    characters.insert("royal-herald".into(), character(CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() }));
    let value = serde_json::to_value(&characters).unwrap();
    assert_eq!(value["royal-herald"]["voice"]["type"], "elevenLabs");
    assert_eq!(value["royal-herald"]["voice"]["voiceId"], "saved-voice");
    assert_eq!(serde_json::from_value::<BTreeMap<String, Character>>(value).unwrap(), characters);
    validate_registry(&characters, Some("royal-herald")).unwrap();
}

#[test]
fn missing_custom_video_keeps_remote_voice_and_reports_default_video() {
    let mut settings = Settings::default();
    settings.characters.insert("royal-herald".into(), character(CharacterVoice::ElevenLabs { voice_id: "saved-voice".into() }));
    settings.selected_character = Some("royal-herald".into());
    let resolved = resolve(&settings, Path::new("missing-assets"), "opencode", Some("royal-herald"));
    assert_eq!(resolved.voice, ResolvedVoice::ElevenLabs { voice_id: "saved-voice".into() });
    assert!(resolved.video_warning.is_some());
    assert_eq!(resolved.video_path, Path::new("missing-assets/opencode/neutral.mp4"));
}

#[test]
fn default_and_legacy_local_voices_are_omitted_from_saved_profiles() {
    for input in [
        serde_json::json!({"name": "Local herald", "voiceDescription": "A warm voice"}),
        serde_json::json!({"name": "Local herald", "voiceDescription": "A warm voice", "voice": {"type": "local", "speaker": null}}),
    ] {
        let profile: Character = serde_json::from_value(input).unwrap();
        assert_eq!(profile.voice, CharacterVoice::Local { speaker: None });
        assert!(!profile.selected);
        assert!(serde_json::to_value(profile).unwrap().get("voice").is_none());
    }
    let profile = character(CharacterVoice::Local { speaker: Some("Luna".into()) });
    assert_eq!(serde_json::to_value(profile).unwrap()["voice"], serde_json::json!({"type": "local", "speaker": "Luna"}));
    assert_eq!(serde_json::to_value(CharacterVoice::default()).unwrap(), serde_json::json!({"type": "local"}));
}

#[test]
fn bundled_relative_video_uses_the_runtime_asset_root() {
    let directory = std::env::temp_dir().join(format!("herald-bundled-character-{}", crate::state::timestamp()));
    let video = directory.join("videos/hatted-herald-01.mp4");
    std::fs::create_dir_all(video.parent().unwrap()).unwrap();
    std::fs::write(&video, []).unwrap();
    let mut settings = Settings::default();
    settings.selected_character = Some("hatted-herald-01".into());
    let resolved = resolve(&settings, &directory, "opencode", Some("hatted-herald-01"));
    assert_eq!(resolved.video_path, video);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn absolute_custom_video_path_remains_absolute() {
    let directory = std::env::temp_dir().join(format!("herald-custom-character-{}", crate::state::timestamp()));
    std::fs::create_dir_all(&directory).unwrap();
    let video = directory.join("custom.mp4");
    std::fs::write(&video, []).unwrap();
    let mut settings = Settings::default();
    settings.characters.insert("custom".into(), character(CharacterVoice::default()));
    settings.characters.get_mut("custom").unwrap().animation_path = Some(video.clone());
    settings.selected_character = Some("custom".into());
    let resolved = resolve(&settings, Path::new("relocated-assets"), "opencode", Some("custom"));
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
fn names_cannot_contain_control_characters() {
    let mut registry = BTreeMap::from([("herald".into(), character(CharacterVoice::default()))]);
    validate_registry(&registry, Some("herald")).unwrap();
    registry.get_mut("herald").unwrap().name = "Bad\nname".into();
    assert_eq!(validate_registry(&registry, Some("herald")), Err("Character name is invalid.".into()));
}
