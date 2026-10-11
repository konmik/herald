use super::*;

#[test]
fn bgra_conversion_clears_the_sentinel_and_keeps_scene_and_card_origins() {
    let stage = PhysicalRect { x: -10, y: 3, width: 8, height: 6 };
    let card = PhysicalRect { x: -8, y: 5, width: 2, height: 2 };
    let colors = [0x123456, 0xff00ff, 0xabcdef, 0x010203];
    let image = compose(&colors, card, stage);
    assert_eq!(image.get_pixel(2, 2).0, [0x56, 0x34, 0x12, 255]);
    assert_eq!(image.get_pixel(3, 2).0, [0, 0, 0, 0]);
    assert_eq!(image.get_pixel(2, 3).0, [0xef, 0xcd, 0xab, 255]);
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 0]);
    let scene = PhysicalRect { x: -9, y: 4, width: 6, height: 5 };
    let mut pixels = vec![0xff00ff; 30];
    for row in 0..2 { pixels[(row + 1) * 6 + 1..(row + 1) * 6 + 3].copy_from_slice(&colors[row * 2..row * 2 + 2]); }
    assert_eq!(compose(&pixels, scene, stage), image);
}

#[test]
fn preview_matches_announcement_dimensions_padding_and_pixels_at_display_scales() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    for scale in [1.0, 1.5, 2.0] {
        let settings = Settings::default();
        let mut playback = Playback::new(&assets, &settings, "", scale).unwrap();
        let card = playback.scene.card;
        assert_eq!(card.scale, scale);
        assert_eq!(card.rect.width, (320.0 * scale) as u32);
        assert_eq!(card.rect.height, (playback.renderer.announcement_height(scale, 700) as f32 * scale) as u32);
        assert_eq!(playback.stage.right() - card.rect.right(), (16.0 * scale) as i32);
        assert_eq!(playback.stage.bottom() - card.rect.bottom(), (64.0 * scale) as i32);
        let mut renderer = Renderer::with_settings(&settings).unwrap();
        renderer.text = MESSAGE.into();
        renderer.title = "Lightning preview".into();
        let mut expected = vec![0; card.rect.width as usize * card.rect.height as usize];
        renderer.draw(&mut expected, card.rect.width as usize, card.rect.height as usize, scale, Some(playback.video.frame()), 0.0, None);
        let pixels = playback.renderer.draw_scene(&playback.scene, Some(playback.video.frame()), 0.0, None);
        assert_eq!(pixels, expected);
        let image = compose(&pixels, card.rect, playback.stage);
        for y in 0..card.rect.height {
            for x in 0..card.rect.width {
                let color = expected[(y * card.rect.width + x) as usize];
                let expected = if color == 0xff00ff { [0, 0, 0, 0] }
                    else { [color as u8, (color >> 8) as u8, (color >> 16) as u8, 255] };
                assert_eq!(image.get_pixel(x + card.rect.x as u32, y + card.rect.y as u32).0, expected);
            }
        }
    }
}

#[test]
fn preview_card_backdrop_outside_the_bubble_and_portrait_is_transparent() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    for scale in [1.0, 2.0] {
        let mut playback = Playback::new(&assets, &Settings::default(), "", scale).unwrap();
        let card = playback.scene.card.rect;
        let pixels = playback.renderer.draw_scene(&playback.scene, Some(playback.video.frame()), 0.0, None);
        let image = compose(&pixels, card, playback.stage);
        for (x, y) in [(card.x + 1, card.y + 1), (card.x + 1, card.bottom() - 2)] {
            assert_eq!(image.get_pixel(x as u32, y as u32).0[3], 0, "Opaque backdrop at ({x}, {y}) at scale {scale}");
        }
    }
}

#[test]
fn bundled_selection_excludes_custom_files_and_replay_avoids_the_previous_character() {
    let root = std::env::temp_dir().join("opencode").join(format!("herald-lightning-pool-{}-{}", std::process::id(), crate::state::timestamp()));
    std::fs::create_dir_all(root.join("videos")).unwrap();
    std::fs::write(root.join("videos/custom.mp4"), []).unwrap();
    assert!(bundled_video(&root, "").is_err());
    let characters = crate::characters::bundled_characters().iter().take(2).collect::<Vec<_>>();
    for (_, character) in &characters { std::fs::write(root.join(character.animation_path.as_ref().unwrap()), []).unwrap(); }
    let previous = characters[0].0;
    for _ in 0..20 { assert_eq!(&bundled_video(&root, previous).unwrap().0, characters[1].0); }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn edits_rebuild_geometry_without_resetting_video_identity_elapsed_or_seed() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let mut playback = Playback::new(&assets, &Settings::default(), "", 1.0).unwrap();
    playback.frame(Duration::from_millis(900));
    let identity = playback.character_id.clone();
    let elapsed = playback.video_elapsed;
    let decoded = playback.video.decoded_frames;
    let seed = playback.seed;
    let settings = crate::lightning::PRESETS[1].settings;
    playback.update(settings);
    assert_eq!(playback.character_id, identity);
    assert_eq!(playback.video_elapsed, elapsed);
    assert_eq!(playback.video.decoded_frames, decoded);
    assert_eq!(playback.seed, seed);
    assert_eq!(playback.effect_elapsed, Duration::ZERO);
    let frame = playback.frame(Duration::from_millis(140));
    assert_eq!(frame.phase, AnnouncementPhase::Impact);
    playback.effect_elapsed = playback.duration + playback.scene.lightning().transition_duration() - Duration::from_millis(1);
    assert_eq!(playback.frame(Duration::from_millis(2)).phase, AnnouncementPhase::Leader);
    assert_eq!(playback.character_id, identity);
}

#[test]
fn worker_resets_effect_delta_only_for_replay() {
    let delta = Duration::from_millis(37);
    assert_eq!(delta_after_intent_change(delta, false), delta);
    assert_eq!(delta_after_intent_change(delta, true), Duration::ZERO);
}

#[test]
fn worker_coalesces_edits_in_one_slot_and_joins_on_drop_even_while_inactive() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let preview = Preview::start(assets, Settings::default(), false, 1.0).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert!(preview.shared.0.lock().unwrap().frame.is_none());
    for index in 0..20 {
        preview.update(LightningSettings { brightness: 1.0 + index as f32 * 0.01, ..LightningSettings::default() });
    }
    preview.set_active(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready = {
            let shared = preview.shared.0.lock().unwrap();
            shared.frame.as_ref().is_some_and(|(revision, frame)| {
                assert_eq!(*revision, shared.intent.revision);
                assert_eq!(frame.as_ref().unwrap().image.width(), 352);
                assert!(frame.as_ref().unwrap().image.height() >= 320);
                true
            })
        };
        if ready { break; }
        assert!(Instant::now() < deadline, "The worker did not publish the latest intent");
        std::thread::sleep(Duration::from_millis(10));
    }
    preview.set_active(false);
    let shared = preview.shared.clone();
    drop(preview);
    let shared = shared.0.lock().unwrap();
    assert!(shared.stop);
    assert!(shared.frame.is_none());
}
