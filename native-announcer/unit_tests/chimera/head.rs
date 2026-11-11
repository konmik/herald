use super::*;

fn pose(artic: HumanArtic) -> super::super::Pose {
    super::super::Pose { artic, f0: 110.0, level: 0.0 }
}

#[test]
fn neutral_head_preserves_colored_anatomy_at_portrait_sizes() {
    for size in [128, 256, 384] {
        let mut head = Head::new(size);
        head.draw(&pose(HumanArtic { jaw: 0.0, lip_ap: 0.0, ..NEUTRAL }), 0.0);
        let pixels = head.frame();
        assert_eq!(pixels.dimensions(), (size, size));
        let count = |predicate: fn([u8; 4]) -> bool| pixels.pixels().filter(|p| predicate(p.0)).count();
        assert!(count(|p| p[2] > p[0] && p[2] > 45) > size as usize * 2);
        assert!(count(|p| u16::from(p[0]) > u16::from(p[1]) * 2 && p[0] > 50) > 10);
        assert!(count(|p| p[1] > p[0] && p[1] > p[2] && p[1] > 50) > 10);
        assert!(count(|p| p[0] > p[1] && p[2] > p[1] && p[0] > 80) > 10);
        assert!(pixels.pixels().any(|p| p[3] > 0));
        assert!(pixels.pixels().filter(|p| p[3] == 0).count() > (size * size / 2) as usize);
        for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
            assert_eq!(pixels.get_pixel(x, y).0, [0; 4]);
        }
        assert!(pixels.pixels().all(|p| p[3] > 0 || p.0 == [0; 4]));
    }
}

#[test]
fn speech_articulation_moves_mouth_and_tongue_without_changing_skull() {
    let mut head = Head::new(256);
    head.draw(&pose(HumanArtic { jaw: 0.0, lip_ap: 0.0, ..NEUTRAL }), 0.0);
    let closed = head.frame().clone();
    head.draw(&pose(HumanArtic { jaw: 2.0, lip_ap: 1.5, lip_prot: 0.7, lip_width: 0.2, tb_y: 5.7, ..NEUTRAL }), 0.0);
    let opened = head.frame();
    let changed = closed.pixels().zip(opened.pixels()).filter(|(a, b)| a != b).count();
    assert!(changed > 300);
    assert!(closed.rows().take(65).flatten().eq(opened.rows().take(65).flatten()));
}

#[test]
fn neutral_animation_blinks_without_speech_motion() {
    let mut head = Head::new(128);
    let quiet = pose(HumanArtic { jaw: 0.0, lip_ap: 0.0, ..NEUTRAL });
    head.draw(&quiet, 0.0);
    let first = head.frame().clone();
    head.draw(&quiet, 4.01);
    assert_ne!(&first, head.frame());
    head.draw(&quiet, 0.0);
    assert_eq!(&first, head.frame());
}

#[test]
fn idle_animation_moves_the_skull_within_two_seconds() {
    let mut head = Head::new(384);
    let quiet = pose(HumanArtic { jaw: 0.0, lip_ap: 0.0, ..NEUTRAL });
    let before_camera = Camera::new(384, 0.0, 0.0, 0.0);
    let after_camera = Camera::new(384, 2.0, 0.0, 0.0);
    let displacement = head.fixed.iter().flat_map(|line| &line.points).filter_map(|&point| {
        let before = before_camera.project(point);
        let after = after_camera.project(point);
        (10.0..128.0).contains(&before[1]).then(|| (after[0] - before[0]).hypot(after[1] - before[1]))
    }).fold(0.0, f64::max);
    assert!(displacement >= 4.0, "Upper skull moved only {displacement} pixels");
    head.draw(&quiet, 0.0);
    let before = head.frame().clone();
    head.draw(&quiet, 2.0);
    let changed = before.rows().take(128).flatten().zip(head.frame().rows().take(128).flatten())
        .filter(|(before, after)| before.0[3].abs_diff(after.0[3]) > 32).count();
    assert!(changed > 300, "Only {changed} upper-skull pixels changed");
}

#[test]
fn clipped_lines_and_nonfinite_elapsed_are_safe() {
    let mut head = Head::new(1);
    head.draw(&pose(NEUTRAL), f64::NAN);
    assert_eq!(head.frame().dimensions(), (1, 1));
    let mut image = RgbaImage::new(8, 8);
    let camera = Camera::new(8, 0.0, 0.0, 0.0);
    draw_line(&mut image, &Line::new(vec![[5000.0, 5000.0, 0.0], [5001.0, 5001.0, 0.0]], BLUE, 1.0, 1.0), &camera);
    assert!(image.pixels().all(|p| *p == Rgba([0, 0, 0, 0])));
}

#[test]
fn beam_thickness_matches_original_logical_widths_at_multiple_resolutions() {
    for size in [128, 256, 384, 672] {
        for offset in [0.0, 0.5] {
            let camera = Camera {
                right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], forward: [0.0; 3],
                distance: 1.0, focal: 1.0, center: f64::from(size) / 2.0 + offset, nod: 0.0, tilt: 0.0,
            };
            for (requested, expected) in [(0.85, 0.85), (1.0, 1.05), (1.4, 1.3), (1.5, 1.6), (2.1, 2.1)] {
                let mut image = RgbaImage::new(size, size);
                let line = Line::new(vec![[-20.0, 9.15, 0.0], [20.0, 9.15, 0.0]], BLUE, requested, 1.0);
                let depth = camera.project(line.points[0])[2];
                draw_line(&mut image, &line, &camera);
                let coverage: f64 = (0..size).map(|y| f64::from(image.get_pixel(size / 2, y)[3]) / 255.0).sum();
                let logical_width = coverage / depth * f64::from(super::super::Head::LOGICAL_SIZE) / f64::from(size);
                assert!((logical_width - expected).abs() < 0.2,
                    "Requested {requested} at {size}px offset {offset}: expected {expected}, got {logical_width}");
            }
        }
    }
}
