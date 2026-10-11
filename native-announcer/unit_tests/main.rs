use super::*;

#[cfg(target_os = "macos")]
#[test]
fn macos_announcer_helper_uses_the_main_application_resources_and_stays_an_announcer() {
    let root = std::env::temp_dir().join(format!("herald-helper-test-{}", std::process::id()));
    let main = root.join("Herald.app/Contents");
    let helper = main.join("Helpers/Herald Announcer.app/Contents");
    std::fs::create_dir_all(helper.join("MacOS")).unwrap();
    std::fs::create_dir_all(main.join("Resources")).unwrap();
    std::fs::write(main.join("Resources/characters.json"), "{}").unwrap();
    assert_eq!(bundled_assets(&helper.join("MacOS")), Some(main.join("Resources")));
    assert!(macos_settings_bundle(&main.join("MacOS/Herald")));
    assert!(!macos_settings_bundle(&helper.join("MacOS/Herald Announcer")));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_application_bundles_find_resources_and_settings_mode() {
    let root = std::env::temp_dir().join(format!("herald-bundle-test-{}", std::process::id()));
    let contents = root.join("Herald Settings.app/Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    assert_eq!(bundled_assets(&contents.join("MacOS")), None);
    std::fs::create_dir_all(contents.join("Resources")).unwrap();
    std::fs::write(contents.join("Resources/characters.json"), "{}").unwrap();
    assert_eq!(bundled_assets(&contents.join("MacOS")), Some(contents.join("Resources")));
    assert_eq!(bundled_assets(&root.join("native-announcer/bin")), None);
    assert!(macos_settings_bundle(&contents.join("MacOS/Herald Settings")));
    assert!(!macos_settings_bundle(&root.join("bin/Herald Settings")));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn waits_for_speech_and_minimum_display_time() {
    let started = Instant::now();
    let mut active = Active {
        notification: Notification {
            id: "test".into(),
            session_id: "test".into(),
            presence_session_id: String::new(),
            completed: 1,
            text: "Done.".into(),
            title: "Test".into(),
            character: "opencode".into(),
            character_id: None,
            emotion: "neutral".into(),
        },
        character: characters::resolve(&settings::Settings::default(), &PathBuf::new(), "opencode", None),
        started,
        expires: started + Duration::from_secs(10),
        end: None,
        speaking: true,
        speech_finished: false,
        silent: false,
        video: None,
        video_path: PathBuf::new(),
        history_recorded: false,
        placement: CardPlacement { rect: PhysicalRect { x: 100, y: 100, width: 320, height: 260 }, scale: 1.0 },
        entrance: None,
        transition_duration: state::TRANSITION_DURATION,
        presented_bounds: None,
    };
    assert!(!active.ready_to_end(started + Duration::from_secs(20)));
    active.speech_finished = true;
    assert!(!active.ready_to_end(started + Duration::from_secs(9)));
    assert!(active.ready_to_end(started + Duration::from_secs(10)));
    active.silent = true;
    active.speech_finished = false;
    assert!(active.ready_to_end(started + Duration::from_secs(10)));
    assert_eq!(active.entrance_strike(started + Duration::from_millis(120)), Some(Duration::from_millis(120)));
    active.silent = false;
    assert_eq!(active.entrance_strike(started + Duration::from_millis(120)), Some(Duration::from_millis(120)));
    assert_eq!(active.entrance_strike(started + Duration::from_millis(650)), None);
    assert_eq!(active.entrance_strike(started + Duration::from_secs(5)), None);
    active.entrance = Some(EntranceScene::new(PhysicalRect { x: -500, y: -200, width: 1500, height: 1200 }, active.placement, 1234, lightning::LightningSettings::default()));
    assert_eq!(active.viewport(started + Duration::from_millis(140)).bottom(), 1000);
    assert_eq!(active.viewport(started + Duration::from_millis(650)), PhysicalRect { x: 100, y: 100, width: 320, height: 260 });
    active.transition_duration = lightning::LightningSettings { speed: 0.25, ..lightning::LightningSettings::default() }.transition_duration();
    assert_eq!(active.entrance_strike(started + Duration::from_millis(2000)), Some(Duration::from_millis(2000)));
    assert_eq!(active.viewport(started + Duration::from_millis(2000)).bottom(), 1000);
    assert_eq!(active.entrance_strike(started + Duration::from_millis(2600)), None);
    assert_eq!(active.viewport(started + Duration::from_millis(2600)), active.placement.rect);
    active.end = Some(started + Duration::from_millis(80));
    assert_eq!(active.entrance_strike(started + Duration::from_millis(120)), None);
    assert_eq!(active.viewport(started + Duration::from_millis(120)), PhysicalRect { x: 100, y: 100, width: 320, height: 260 });
}

#[test]
fn pending_presentation_stays_hidden_until_ready() {
    let notification = Notification {
        id: "pending".into(),
        session_id: "test".into(),
        presence_session_id: String::new(),
        completed: 1,
        text: "Done.".into(),
        title: "Test".into(),
        character: "opencode".into(),
        character_id: None,
        emotion: "neutral".into(),
    };
    let requested = Instant::now();
    let mut live_settings = settings::Settings { silent_sound_seconds: 10, ..settings::Settings::default() };
    let mut presentation = Presentation::Preparing(Pending {
        notification,
        character: characters::resolve(&settings::Settings::default(), &PathBuf::new(), "opencode", None),
        requested,
        readiness_timeout: speech_readiness_timeout(&live_settings),
        duration: Duration::from_secs(10),
        video: None,
        video_path: PathBuf::new(),
        placement: CardPlacement { rect: PhysicalRect { x: 100, y: 100, width: 320, height: 260 }, scale: 1.0 },
        entrance: None,
        transition_duration: state::TRANSITION_DURATION,
    });
    assert!(matches!(&presentation, Presentation::Preparing(_)));
    assert!(presentation.take_preparing(Some("other")).is_none());
    assert!(matches!(&presentation, Presentation::Preparing(_)));
    let pending = presentation.take_preparing(Some("pending")).unwrap();
    live_settings.silent_sound_seconds = 0;
    assert_eq!(speech_readiness_timeout(&live_settings), Duration::from_secs(30));
    assert!(!pending.readiness_timed_out(requested + Duration::from_secs(30)));
    assert!(!pending.readiness_timed_out(requested + Duration::from_secs(39)));
    assert!(pending.readiness_timed_out(requested + Duration::from_secs(40)));
    assert!(matches!(presentation, Presentation::Idle));
    let active = pending.activate(true);
    assert!(active.started >= requested);
}
