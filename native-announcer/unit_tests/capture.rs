use super::*;

#[test]
fn exports_only_rendered_pixels_with_a_plain_background_and_timing() {
    let directory = std::env::temp_dir().join("opencode").join(format!("herald-frames-{}-{}", std::process::id(), crate::state::timestamp()));
    let mut frames = Frames::new(&directory).unwrap();
    frames.save(&[0xff00ff, 0x123456], 2, 1, Duration::from_millis(15), None).unwrap();
    frames.save(&[0xffffff, 0], 2, 1, Duration::from_millis(80), Some(Duration::from_millis(70))).unwrap();
    drop(frames);
    let image = image::open(directory.join("frame-000000.png")).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [32, 32, 32]);
    assert_eq!(image.get_pixel(1, 0).0, [18, 52, 86]);
    let timeline = std::fs::read_to_string(directory.join("timeline.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = timeline.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["elapsed"], 0.015);
    assert_eq!(entries[1]["elapsed"], 0.08);
    assert!(entries[0]["closingStart"].is_null());
    assert_eq!(entries[1]["closingStart"], 0.07);
    assert!(entries[0].get("geometry").is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn expanded_and_restored_captures_keep_physical_geometry_and_the_sampled_phase() {
    let directory = std::env::temp_dir().join("opencode").join(format!("herald-scene-{}", std::process::id()));
    let card = crate::render::CardPlacement { rect: crate::render::PhysicalRect { x: -504, y: 250, width: 480, height: 390 }, scale: 1.5 };
    let scene = crate::render::EntranceScene::new(crate::render::PhysicalRect { x: -1920, y: -200, width: 1920, height: 1080 }, card, 1234, crate::lightning::LightningSettings::default());
    let mut frames = Frames::new(&directory).unwrap();
    let pixels = vec![0x123456; scene.canvas.width as usize * scene.canvas.height as usize];
    frames.save_scene(&pixels, scene.canvas.width, scene.canvas.height, Duration::from_millis(140), None, card, Some(&scene), scene.canvas).unwrap();
    frames.save_scene(&vec![0; 480 * 390], 480, 390, Duration::from_millis(700), None, card, Some(&scene), card.rect).unwrap();
    frames.finish().unwrap();
    let timeline = std::fs::read_to_string(directory.join("timeline.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = timeline.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(entries[0]["geometry"]["phase"], "impact");
    assert_eq!(entries[0]["geometry"]["monitor"]["y"], -200);
    assert_eq!(entries[0]["geometry"]["source"][1], 879.0);
    assert_eq!(entries[0]["geometry"]["card"]["x"], -504);
    assert_eq!(entries[0]["geometry"]["scale"], 1.5);
    assert_eq!(entries[1]["geometry"]["phase"], "holding");
    assert_eq!(entries[1]["geometry"]["viewport"], serde_json::json!({"x": -504, "y": 250, "width": 480, "height": 390}));
    assert_eq!(image::open(directory.join("frame-000001.png")).unwrap().width(), 480);
    std::fs::remove_dir_all(directory).unwrap();
}
