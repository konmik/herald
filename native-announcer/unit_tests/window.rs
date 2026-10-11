use super::*;

#[test]
fn decorative_pixels_pass_hit_testing_but_the_card_remains_passive_and_interactive() {
    let card = crate::render::PhysicalRect { x: -500, y: -200, width: 480, height: 390 };
    CARD_REGION.with(|region| *region.borrow_mut() = Some(card));
    let point = |x: i32, y: i32| ((y as u16 as u32) << 16 | x as u16 as u32) as isize;
    unsafe {
        assert_eq!(procedure(std::ptr::null_mut(), WM_NCHITTEST, 0, point(-400, -100)), HTCLIENT as isize);
        assert_eq!(procedure(std::ptr::null_mut(), WM_NCHITTEST, 0, point(-600, -100)), HTTRANSPARENT as isize);
        assert_eq!(procedure(std::ptr::null_mut(), WM_NCHITTEST, 0, point(-20, 0)), HTTRANSPARENT as isize);
        assert_eq!(procedure(std::ptr::null_mut(), WM_MOUSEACTIVATE, 0, 0), MA_NOACTIVATE as isize);
    }
}

#[test]
fn expanded_decorations_select_cross_process_transparency_only_outside_the_card() {
    let card = crate::render::PhysicalRect { x: 600, y: 400, width: 320, height: 260 };
    let canvas = crate::render::PhysicalRect { x: 440, y: 0, width: 480, height: 660 };
    assert_eq!(card_input_style(0x08080088, canvas, Some(card), POINT { x: 480, y: 100 }), 0x080800a8);
    assert_eq!(card_input_style(0x080800a8, canvas, Some(card), POINT { x: 700, y: 500 }), 0x08080088);
    assert_eq!(card_input_style(0x080800a8, card, Some(card), POINT { x: 480, y: 100 }), 0x08080088);
}

#[test]
fn monitor_edge_is_separate_from_work_area_and_preserves_negative_coordinates() {
    let monitor = Monitor { work_area: physical_rect(RECT { left: -1920, top: -160, right: 0, bottom: 880 }),
        bounds: physical_rect(RECT { left: -1920, top: -200, right: 0, bottom: 880 }), scale: 1.5 };
    assert_eq!((monitor.position().x, monitor.position().y), (-1920, -160));
    assert_eq!((monitor.size().width, monitor.size().height), (1920, 1040));
    let card = crate::render::CardPlacement { rect: crate::render::PhysicalRect { x: -504, y: 250, width: 480, height: 390 }, scale: monitor.scale_factor() as f32 };
    let scene = crate::render::EntranceScene::new(monitor.bounds(), card, 17, crate::lightning::LightningSettings::default());
    assert_eq!(scene.source[0], -1.0);
    assert_eq!(scene.canvas.right(), 0);
    assert_eq!(scene.card.rect.x, -504);
    assert_eq!(scene.card.scale, 1.5);
}
