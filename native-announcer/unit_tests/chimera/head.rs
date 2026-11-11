use super::*;

fn pose(artic: HumanArtic) -> super::super::Pose {
    super::super::Pose { artic, f0: 110.0, level: 0.0 }
}

#[test]
fn neutral_head_preserves_colored_anatomy_at_portrait_sizes() {
    for size in [128, 256] {
        let mut head = Head::new(size);
        head.draw(&pose(HumanArtic { jaw: 0.0, lip_ap: 0.0, ..NEUTRAL }), 0.0);
        let pixels = head.frame();
        assert_eq!(pixels.dimensions(), (size, size));
        let count = |predicate: fn([u8; 4]) -> bool| pixels.pixels().filter(|p| predicate(p.0)).count();
        assert!(count(|p| p[2] > p[0] && p[2] > 45) > size as usize * 2);
        assert!(count(|p| u16::from(p[0]) > u16::from(p[1]) * 2 && p[0] > 50) > 10);
        assert!(count(|p| p[1] > p[0] && p[1] > p[2] && p[1] > 50) > 10);
        assert!(count(|p| p[0] > p[1] && p[2] > p[1] && p[0] > 80) > 10);
        assert!(pixels.pixels().all(|p| p[3] == 255));
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
fn clipped_lines_and_nonfinite_elapsed_are_safe() {
    let mut head = Head::new(1);
    head.draw(&pose(NEUTRAL), f64::NAN);
    assert_eq!(head.frame().dimensions(), (1, 1));
    let mut image = RgbaImage::new(8, 8);
    let camera = Camera::new(8, 0.0, 0.0, 0.0);
    draw_line(&mut image, &Line::new(vec![[5000.0, 5000.0, 0.0], [5001.0, 5001.0, 0.0]], BLUE, 1.0, 1.0), &camera);
    assert!(image.pixels().all(|p| *p == Rgba([0, 0, 0, 0])));
}
