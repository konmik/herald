use super::*;

const CHIMERA_MESSAGE: &str = "Attention, human. The build is ready and the checks have passed. Please admire my moving mouth, glowing tongue, and deeply questionable voice. Mission complete.";

#[test]
fn chimera_signal_breaks_animate_without_filling_the_background_or_closing_the_mouth_gap() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.text = CHIMERA_MESSAGE.into();
    let head = crate::chimera::Head::new();
    for scale in [1.0, 1.5] {
        let width = (600.0 * scale) as usize;
        let height = (432.0 * scale) as usize;
        let mut bubble = vec![0; width * height];
        renderer.draw(&mut bubble, width, height, scale, None, 0.0, None);
        let seed = renderer.seed;
        let mut clean = vec![0; width * height];
        renderer.draw(&mut clean, width, height, scale, Some(head.frame()), 0.0, None);
        assert_eq!(renderer.seed, seed);
        let mut previous = clean.clone();
        for _ in 0..4 {
            let seed = renderer.seed;
            let mut broken = vec![0; width * height];
            renderer.draw(&mut broken, width, height, scale, Some(head.frame()), 0.62, None);
            assert_eq!(renderer.seed, xorshift_jump(seed, 1));
            assert_ne!(broken, previous);
            assert!(broken.iter().zip(&clean).filter(|(a, b)| (**a >> 24) != (**b >> 24)).count() > (300.0 * scale * scale) as usize);
            assert!(broken.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
            assert!(broken.chunks(width).zip(clean.chunks(width)).all(|(a, b)| a[..(325.0 * scale) as usize] == b[..(325.0 * scale) as usize]));
            assert!(broken.chunks(width).flat_map(|row| &row[(405.0 * scale) as usize..]).filter(|pixel| **pixel == 0).count() > width * height / 6);
            assert_eq!(broken[(410.0 * scale) as usize * width + (330.0 * scale) as usize], 0);
            for y in (225.0 * scale) as usize..(310.0 * scale) as usize {
                let bubble_right = bubble[y * width..y * width + (405.0 * scale) as usize].iter().rposition(|pixel| *pixel != 0);
                if let Some(right) = bubble_right {
                    if let Some(gap) = broken[y * width + right + 1..(y + 1) * width].iter().position(|pixel| *pixel != 0) {
                        assert!(gap >= (3.0 * scale) as usize, "Signal closed the mouth gap at {scale}, row {y}");
                    }
                }
            }
            previous = broken;
        }
        let seed = renderer.seed;
        renderer.draw(&mut previous, width, height, scale, Some(head.frame()), 0.0, None);
        assert_eq!(previous, clean);
        assert_eq!(renderer.seed, seed);
    }
}

#[test]
fn signal_maps_wire_paths_to_torn_pixels_without_long_diagonal_bridges() {
    for scale in [1.0, 1.5] {
        let size = (336.0 * scale) as u32;
        let signal = chimera::SignalDistortion::new(size as usize, scale, 0.62, 177);
        let x = (size as f32 * 0.6) as u32;
        let image = RgbaImage::from_fn(size, size, |column, row| image::Rgba(if column.abs_diff(x) <= 1 && row > 8 && row + 8 < size { [255; 4] } else { [0; 4] }));
        let broken = signal.apply(&image);
        let path = signal.path(&[[x as f32 / size as f32, 0.05], [x as f32 / size as f32, 0.95]]);
        assert!(path.len() > 2);
        for point in &path {
            let column = (point[0] * size as f32).floor() as u32;
            let row = (point[1] * size as f32).floor() as u32;
            assert!(broken.get_pixel(column, row)[3] > 0);
        }
        for pair in path.windows(2) {
            if (pair[1][0] - pair[0][0]).abs() * size as f32 > 1.0 {
                assert!((pair[1][1] - pair[0][1]).abs() * (size as f32) < 0.1);
            }
        }
        assert!(broken.pixels().filter(|pixel| pixel[3] > 0).count() < size as usize * 10);
        assert_eq!(broken.get_pixel(0, 0).0, [0; 4]);
        assert_eq!(broken.get_pixel(size - 1, size - 1).0, [0; 4]);
    }
}

#[test]
fn signal_keeps_eye_and_mouth_discharge_attached_to_the_distorted_head() {
    let head = crate::chimera::Head::new();
    let origins: Vec<_> = head.eye_rays().iter().map(|ray| ray[0]).chain(head.mouth_discharge().map(|mouth| mouth.ray[0])).collect();
    for scale in [1.0, 1.5] {
        let size = (336.0 * scale) as usize;
        let seed = (1..5000).find(|seed| {
            let signal = chimera::SignalDistortion::new(size, scale, 0.62, xorshift_jump(*seed, 1));
            origins.iter().all(|point| (signal.point(*point)[0] - point[0]) * 336.0 >= 3.9)
        }).unwrap();
        let signal = chimera::SignalDistortion::new(size, scale, 0.62, xorshift_jump(seed, 1));
        let mut renderer = Renderer::new().unwrap();
        renderer.set_style(AnnouncementStyle::Chimera);
        renderer.text = CHIMERA_MESSAGE.into();
        renderer.set_anatomy_paths(head.anatomy_paths());
        let width = (600.0 * scale) as usize;
        let height = (432.0 * scale) as usize;
        let mut original = vec![0; width * height];
        renderer.seed = seed;
        renderer.draw(&mut original, width, height, scale, Some(head.frame()), 0.62, Some(Duration::from_millis(430)));
        renderer.set_eye_rays(head.eye_rays());
        renderer.set_mouth_discharge(head.mouth_discharge());
        renderer.seed = seed;
        let mut firing = vec![0; width * height];
        renderer.draw(&mut firing, width, height, scale, Some(head.frame()), 0.62, Some(Duration::from_millis(430)));
        for origin in &origins {
            let mapped = signal.point(*origin);
            let x = ((325.0 + mapped[0] * 336.0) * scale).round() as usize;
            let y = ((84.0 + mapped[1] * 336.0) * scale).round() as usize;
            assert!((y - 1..=y + 1).any(|row| (x - 1..=x + 1).any(|column| {
                [0, 8, 16].into_iter().any(|shift| ((firing[row * width + column] >> shift) as u8).abs_diff((original[row * width + column] >> shift) as u8) > 40)
            })), "Discharge missed its displaced origin at {scale}, {x}, {y}");
        }
    }
}

#[test]
fn mouth_discharge_travels_through_the_tract_before_shooting_out() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.text = CHIMERA_MESSAGE.into();
    let image = RgbaImage::new(384, 384);
    let discharge = crate::chimera::MouthDischarge {
        tract: vec![[0.7, 0.85], [0.7, 0.6], [0.2, 0.6]],
        ray: [[0.2, 0.6], [0.04, 0.6]],
    };
    let duration = Duration::from_secs(13);
    let burst_at = |progress: f32| (650..3000).map(Duration::from_millis).find(|elapsed| {
        ambient_burst(*elapsed, duration, 177, Settings::default().lightning)
            .is_some_and(|burst| (burst.progress - progress).abs() < 0.015)
    }).unwrap();
    for holding in [false, true] {
        for late in [false, true] {
            let entrance = (!holding).then(|| Duration::from_millis(if late { 430 } else { 240 }));
            renderer.lightning_activity = holding.then(|| LightningActivity::Holding(burst_at(if late { 0.8 } else { 0.2 }), duration, 177));
            let mut original = vec![0; 600 * 432];
            renderer.set_mouth_discharge(None);
            renderer.draw(&mut original, 600, 432, 1.0, Some(&image), 0.0, entrance);
            let mut firing = vec![0; 600 * 432];
            renderer.set_mouth_discharge(Some(&discharge));
            renderer.draw(&mut firing, 600, 432, 1.0, Some(&image), 0.0, entrance);
            assert_ne!(firing[350 * 600 + 560], original[350 * 600 + 560]);
            let outlet_pixels = (275..300).flat_map(|y| (345..392).map(move |x| y * 600 + x))
                .filter(|index| firing[*index] != original[*index]).count();
            if late { assert!(outlet_pixels > 10, "No mouth discharge, holding {holding}"); }
            else { assert_eq!(outlet_pixels, 0, "Outlet fired before energy reached the lips, holding {holding}"); }
            assert!(firing.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
        }
    }
}

#[test]
fn eye_bolts_leave_the_head_and_stop_between_flashes_and_after_consumption() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.text = CHIMERA_MESSAGE.into();
    let head = crate::chimera::Head::new();
    renderer.set_anatomy_paths(head.anatomy_paths());
    let left = head.frame().enumerate_pixels().filter(|(_, _, pixel)| pixel[3] > 0).map(|(x, _, _)| x).min().unwrap();
    let duration = Duration::from_secs(13);
    let elapsed = (650..3000).map(Duration::from_millis).find(|elapsed| {
        ambient_burst(*elapsed, duration, 177, renderer.lightning).is_some_and(|burst| burst.intensity > 0.8)
    }).unwrap();
    for scale in [1.0, 1.5] {
        let width = (600.0 * scale) as usize;
        let height = (432.0 * scale) as usize;
        let mut original = vec![0; width * height];
        renderer.set_eye_rays(&[]);
        renderer.draw(&mut original, width, height, scale, Some(head.frame()), 0.0, Some(Duration::from_millis(300)));
        let mut firing = vec![0; width * height];
        renderer.set_eye_rays(head.eye_rays());
        renderer.draw(&mut firing, width, height, scale, Some(head.frame()), 0.0, Some(Duration::from_millis(300)));
        let boundary = ((325.0 + left as f32 / head.frame().width() as f32 * 336.0) * scale) as usize;
        let outside = firing.iter().zip(&original).enumerate().filter(|(index, (a, b))| index % width + 2 < boundary && a != b).count();
        assert!(outside > 20, "Eye bolts were clipped to the head at {scale}: {outside} pixels");
        assert!(firing.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
        renderer.lightning_activity = Some(LightningActivity::Holding(elapsed, duration, 177));
        renderer.set_eye_rays(&[]);
        renderer.draw(&mut original, width, height, scale, Some(head.frame()), 0.0, None);
        renderer.set_eye_rays(head.eye_rays());
        renderer.draw(&mut firing, width, height, scale, Some(head.frame()), 0.0, None);
        assert!(firing.iter().zip(&original).enumerate().filter(|(index, (a, b))| index % width + 2 < boundary && a != b).count() > 20);
        for activity in [None, Some(LightningActivity::Closing(Duration::from_millis(500), 177))] {
            renderer.lightning_activity = activity;
            renderer.set_eye_rays(&[]);
            renderer.draw(&mut original, width, height, scale, Some(head.frame()), 0.0, None);
            renderer.set_eye_rays(head.eye_rays());
            renderer.draw(&mut firing, width, height, scale, Some(head.frame()), 0.0, None);
            assert_eq!(firing, original);
        }
        renderer.lightning_activity = None;
    }
}

#[test]
fn anatomy_lightning_tracks_open_paths_without_bridging_their_ends() {
    let mut coverage = PaintCoverage::new(vec![true; 120 * 120], PixelFormat::PremultipliedArgb);
    coverage.wires = vec![vec![[20.0, 20.0], [20.0, 100.0], [100.0, 100.0]]];
    let mut full = LightningPaint::new(120, 120, Settings::default().lightning);
    coverage.draw_wires(&mut full, 1.0, 1, 1.0, 1.0, 1);
    full.rasterize();
    assert!(full.glow[60 * 120 + 20] > 0.3);
    assert!(full.glow[100 * 120 + 60] > 0.3);
    assert_eq!(full.glow[60 * 120 + 60], 0.0);
    let mut partial = LightningPaint::new(120, 120, Settings::default().lightning);
    coverage.draw_wires(&mut partial, 1.0, 1, 1.0, 0.25, 1);
    partial.rasterize();
    assert!(partial.glow[40 * 120 + 20] > 0.3);
    assert_eq!(partial.glow[80 * 120 + 20], 0.0);
    assert_eq!(partial.glow[100 * 120 + 60], 0.0);
}

#[test]
fn anatomy_lightning_flashes_on_internal_wires_during_holding() {
    let lightning = Settings::default().lightning;
    let duration = Duration::from_secs(13);
    let elapsed = (650..3000).map(Duration::from_millis).find(|elapsed| {
        ambient_burst(*elapsed, duration, 177, lightning).is_some_and(|burst| burst.intensity > 0.8)
    }).unwrap();
    let activity = LightningActivity::Holding(elapsed, duration, 177);
    let mut coverage = PaintCoverage::new(vec![true; 120 * 120], PixelFormat::PremultipliedArgb);
    let mut original = vec![0xff101010; 120 * 120];
    coverage.draw_activity(&mut original, 120, 120, 1.0, activity, lightning);
    coverage.wires = [30.0, 60.0, 90.0].into_iter().map(|x| vec![[x, 30.0], [x, 90.0]]).collect();
    let mut illuminated = vec![0xff101010; 120 * 120];
    coverage.draw_activity(&mut illuminated, 120, 120, 1.0, activity, lightning);
    for x in [30, 60, 90] { assert_ne!(illuminated[60 * 120 + x], original[60 * 120 + x]); }
}

#[test]
fn chimera_lightning_reaches_internal_eye_and_throat_wires() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.text = CHIMERA_MESSAGE.into();
    let head = crate::chimera::Head::new();
    for scale in [1.0, 1.5] {
        let width = (600.0 * scale) as usize;
        let height = (432.0 * scale) as usize;
        for closing in [false, true] {
            let entrance = (!closing).then(|| Duration::from_millis(300));
            renderer.lightning_activity = closing.then(|| LightningActivity::Closing(Duration::from_millis(100), 177));
            let mut original = vec![0; width * height];
            renderer.set_anatomy_paths(&[]);
            renderer.draw(&mut original, width, height, scale, Some(head.frame()), 0.0, entrance);
            let mut illuminated = vec![0; width * height];
            renderer.set_anatomy_paths(head.anatomy_paths());
            renderer.draw(&mut illuminated, width, height, scale, Some(head.frame()), 0.0, entrance);
            for throat in [false, true] {
                let changes = head.anatomy_paths().iter().flatten().filter(|point| (point[1] > 0.65) == throat).filter(|point| {
                    let x = ((325.0 + point[0] * 336.0) * scale).round() as usize;
                    let y = ((84.0 + point[1] * 336.0) * scale).round() as usize;
                    illuminated[y * width + x] != original[y * width + x]
                }).count();
                assert!(changes > 5, "Missing anatomy lightning: throat {throat}, closing {closing}, scale {scale}, changed points {changes}");
            }
            assert!(illuminated.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
        }
    }
    renderer.set_anatomy_paths(&[]);
    assert!(renderer.anatomy_paths.is_empty());
}

#[test]
fn chimera_preserves_partial_alpha_through_text_head_and_lightning() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.text = CHIMERA_MESSAGE.into();
    renderer.title = "CHIMERA announcement".into();
    let head = crate::chimera::Head::new();
    let mut pixels = vec![0; 600 * 432];
    renderer.draw(&mut pixels, 600, 432, 1.0, Some(head.frame()), 0.0, None);
    assert!(pixels.chunks(600).flat_map(|row| &row[405..]).any(|pixel| (1..255).contains(&(pixel >> 24))));
    assert_eq!(pixels[410 * 600 + 330], 0);
    for phase in [None, Some(Duration::from_millis(140)), Some(Duration::from_millis(300))] {
        renderer.draw(&mut pixels, 600, 432, 1.0, Some(head.frame()), 0.0, phase);
        assert!(pixels.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
    }
    for milliseconds in [700, 1200, 3000] {
        renderer.lightning_activity = Some(LightningActivity::Holding(Duration::from_millis(milliseconds), Duration::from_secs(5), 1234));
        renderer.draw(&mut pixels, 600, 432, 1.0, Some(head.frame()), 0.0, None);
        assert!(pixels.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
    }
    for milliseconds in [100, 300, 650] {
        renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(milliseconds), 1234));
        renderer.draw(&mut pixels, 600, 432, 1.0, Some(head.frame()), 0.0, None);
        assert!(pixels.iter().all(|pixel| [0, 8, 16].into_iter().all(|shift| (pixel >> shift) & 255 <= pixel >> 24)));
    }
}

#[test]
fn chimera_card_fits_small_and_high_density_monitors() {
    let style = AnnouncementStyle::Chimera;
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(style);
    for (width, height, requested) in [(1440, 1080, 2.5), (1280, 720, 2.0), (800, 600, 1.5), (640, 480, 1.0), (3840, 2160, 2.5)] {
        let scale = style.fit_scale(width, height, requested);
        for message in [CHIMERA_MESSAGE.to_owned(), CHIMERA_MESSAGE.repeat(6)] {
            renderer.text = message;
            let layout = renderer.announcement_layout(scale as f32, (f64::from(height) / scale - 80.0) as u32);
            assert!((f64::from(layout.width) + 32.0) * scale <= f64::from(width) + 0.001);
            assert!((f64::from(layout.height) + 80.0) * scale <= f64::from(height) + 0.001);
        }
        assert_eq!(AnnouncementStyle::Video.fit_scale(width, height, requested), requested);
    }
}

#[test]
fn chimera_text_fits_the_ribbed_frame_at_display_scales() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    for message in ["Ready.", CHIMERA_MESSAGE, &CHIMERA_MESSAGE.repeat(3)] {
        renderer.text = message.into();
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5] {
            let layout = renderer.announcement_layout(scale, 1800);
            let block = TextBlock {
                text: &renderer.text, x: layout.body[0], y: layout.body[1], width: layout.body[2],
                line_height: Some(f32::from(renderer.body_preference.size) * 1.65),
                size: f32::from(renderer.body_preference.size), color: 0xffffff, max_height: f32::MAX,
            };
            let glyphs = layout_glyphs(&renderer.body_font, &block, scale);
            assert!(glyphs.iter().all(|glyph| glyph.y + glyph.height as f32 <= layout.body[3] * scale), "Text clipped at {scale}, message length {}, last glyph bottom {}, cutoff {}, measured height {}", message.len(), glyphs.iter().map(|glyph| glyph.y + glyph.height as f32).fold(0.0, f32::max), layout.body[3] * scale, renderer.message_height(scale));
            assert!(glyphs.iter().all(|glyph| glyph.x + glyph.width as f32 <= (layout.body[0] + layout.body[2]) * scale + 1.0));
            assert_eq!(layout.width, 600);
            assert_eq!(layout.portrait[2], 336.0);
        }
    }
}

#[test]
fn chimera_halves_bottom_padding_without_moving_text_or_head() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.title = "CHIMERA announcement".into();
    for message in ["Ready.".to_owned(), CHIMERA_MESSAGE.into(), CHIMERA_MESSAGE.repeat(3)] {
        renderer.text = message;
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let layout = renderer.announcement_layout(scale, 1800);
            let width = (layout.width as f32 * scale).round() as usize;
            let height = (layout.height as f32 * scale).round() as usize;
            let body = TextBlock {
                text: &renderer.text, x: layout.body[0], y: layout.body[1], width: layout.body[2],
                line_height: Some(f32::from(renderer.body_preference.size) * 1.65),
                size: f32::from(renderer.body_preference.size), color: 0xffffff, max_height: layout.body[3],
            };
            let glyphs = layout_glyphs(&renderer.body_font, &body, scale);
            let mut text = vec![0; width * height];
            renderer.text(&mut text, width, height, scale, &renderer.body_font, body);
            let text_bottom = (text.chunks(width).rposition(|row| row.iter().any(|pixel| *pixel != 0)).unwrap() + 1) as f32;
            let mut pixels = vec![0; width * height];
            renderer.draw(&mut pixels, width, height, scale, None, 0.0, None);
            let bubble_bottom = (pixels.chunks(width).rposition(|row| row.iter().any(|pixel| *pixel != 0)).unwrap() + 1) as f32;
            let original_padding = height as f32 - 126.0 * scale - text_bottom;
            assert!((bubble_bottom - text_bottom - original_padding * 0.5).abs() <= 2.0,
                "Bottom padding was not halved at {scale}, text bottom {text_bottom}, bubble bottom {bubble_bottom}, original padding {original_padding}");
            for glyph in glyphs.iter().filter(|glyph| glyph.pixels.iter().any(|alpha| *alpha > 0)) {
                for y in glyph.y as usize - 1..=glyph.y as usize + glyph.height {
                    for x in glyph.x as usize - 1..=glyph.x as usize + glyph.width {
                        assert!(pixels[y * width + x] >> 24 >= 240, "Text reaches the curved rim at {scale}, pixel {x}, {y}");
                    }
                }
            }
            assert_eq!(layout.body[0..3], [65.0, 105.6, 288.0]);
            assert_eq!(layout.portrait, [325.0, layout.height as f32 - 348.0, 336.0, 336.0]);
        }
    }
}

#[test]
fn chimera_remeasures_padding_when_same_size_messages_change() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    let mut previous_bottom = None;
    for message in ["Ready.", "The build passed.\nThe tests passed.\nDeployment finished.", "Ready."] {
        renderer.text = message.into();
        let mut pixels = vec![0; 600 * 432];
        renderer.draw(&mut pixels, 600, 432, 1.0, None, 0.0, None);
        let bottom = pixels.chunks(600).rposition(|row| row.iter().any(|pixel| *pixel != 0)).unwrap();
        if let Some(previous) = previous_bottom { assert_ne!(bottom, previous); }
        previous_bottom = Some(bottom);
    }
}

#[test]
fn chimera_style_switch_restores_video_fonts_and_rendering() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = CHIMERA_MESSAGE.into();
    renderer.title = "CHIMERA announcement".into();
    let mut original = vec![0; 320 * 300];
    renderer.draw(&mut original, 320, 300, 1.0, None, 0.0, None);
    renderer.set_style(AnnouncementStyle::Chimera);
    assert_eq!(renderer.body_preference, FontPreference::new("Berkeley Mono", 14));
    assert_eq!(renderer.title_preference, FontPreference::new("Berkeley Mono", 12));
    let mut chimera = vec![0; 600 * 432];
    renderer.draw(&mut chimera, 600, 432, 1.0, None, 0.0, None);
    renderer.set_style(AnnouncementStyle::Video);
    let mut restored = vec![0; original.len()];
    renderer.seed = 567891;
    renderer.draw(&mut restored, 320, 300, 1.0, None, 0.0, None);
    assert_eq!(original, restored);
    renderer.set_preferences(&FontPreference::new("Consolas", 17), &FontPreference::new("Consolas", 15));
    renderer.set_style(AnnouncementStyle::Chimera);
    assert_eq!(renderer.body_preference, FontPreference::new("Consolas", 17));
    assert_eq!(renderer.title_preference, FontPreference::new("Consolas", 15));
}

#[test]
fn chimera_portrait_keeps_transparent_corners_and_a_mouth_gap() {
    let mut renderer = Renderer::new().unwrap();
    renderer.set_style(AnnouncementStyle::Chimera);
    renderer.text = CHIMERA_MESSAGE.into();
    let mut head = crate::chimera::Head::new();
    let speech = crate::chimera::prepare("Attention human. Please admire my mouth.", &std::sync::atomic::AtomicBool::new(false)).unwrap();
    head.attach_capture(speech, Duration::ZERO);
    for elapsed in [0.0, 0.4, 0.8, 1.5, 2.2, 4.5, 6.7, 9.0, 12.2] {
        head.advance(Duration::from_secs_f64(elapsed));
        for (x, _, pixel) in head.frame().enumerate_pixels().filter(|(_, _, pixel)| pixel.0[3] > 0) {
            assert!(325.0 + x as f32 / head.frame().width() as f32 * 336.0 < 600.0, "Head clipped at {elapsed}s, pixel {x}: {:?}", pixel.0);
        }
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let width = (600.0 * scale) as usize;
            let height = (432.0 * scale) as usize;
            let mut pixels = vec![0; width * height];
            renderer.draw(&mut pixels, width, height, scale, None, 0.0, None);
            let transparent_corner = pixels[(100.0 * scale) as usize * width + (330.0 * scale) as usize];
            renderer.draw(&mut pixels, width, height, scale, Some(head.frame()), 0.0, None);
            assert_eq!(pixels[(100.0 * scale) as usize * width + (330.0 * scale) as usize], transparent_corner);
            assert_eq!(pixels[(410.0 * scale) as usize * width + (330.0 * scale) as usize], 0);
            for y in (225.0 * scale) as usize..(310.0 * scale) as usize {
                let row = &pixels[y * width..(y + 1) * width];
                let bubble_right = row[..(405.0 * scale) as usize].iter().rposition(|pixel| *pixel != 0);
                let head_left = row[(405.0 * scale) as usize..].iter().position(|pixel| *pixel != 0).map(|x| x + (405.0 * scale) as usize);
                if let (Some(bubble_right), Some(head_left)) = (bubble_right, head_left) {
                    assert!(head_left - bubble_right >= (12.0 * scale) as usize, "Mouth gap closed at {elapsed}s, scale {scale}, row {y}: {bubble_right}..{head_left}");
                }
            }
        }
    }
}

#[test]
fn xorshift_jump_matches_repeated_draws() {
    let mut state = 0x1234_5678_u32;
    for steps in [0_usize, 1, 2, 3, 31, 32, 255, 256, 1000, 65_537] {
        let mut serial = state;
        for _ in 0..steps { serial ^= serial << 13; serial ^= serial >> 17; serial ^= serial << 5; }
        assert_eq!(xorshift_jump(state, steps), serial, "{steps} steps");
        state = serial.wrapping_add(0x9e37_79b9);
    }
}

#[test]
fn distance_field_matches_the_sequential_chamfer_passes() {
    fn reference(glow: &[f32], width: usize, height: usize) -> Vec<f32> {
        let mut distance: Vec<f32> = glow.iter().map(|strength| if *strength > 0.25 { 0.0 } else { 10000.0 }).collect();
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if x > 0 { distance[index] = distance[index].min(distance[index - 1] + 1.0); }
                if y > 0 { distance[index] = distance[index].min(distance[index - width] + 1.0); }
                if x > 0 && y > 0 { distance[index] = distance[index].min(distance[index - width - 1] + 1.414); }
                if x + 1 < width && y > 0 { distance[index] = distance[index].min(distance[index - width + 1] + 1.414); }
            }
        }
        for y in (0..height).rev() {
            for x in (0..width).rev() {
                let index = y * width + x;
                if x + 1 < width { distance[index] = distance[index].min(distance[index + 1] + 1.0); }
                if y + 1 < height { distance[index] = distance[index].min(distance[index + width] + 1.0); }
                if x + 1 < width && y + 1 < height { distance[index] = distance[index].min(distance[index + width + 1] + 1.414); }
                if x > 0 && y + 1 < height { distance[index] = distance[index].min(distance[index + width - 1] + 1.414); }
            }
        }
        distance
    }
    let mut seed = 7_u32;
    for (width, height) in [(1, 1), (1, 9), (9, 1), (37, 23), (320, 260)] {
        let mut paint = LightningPaint::new(width, height, crate::lightning::PRESETS[0].settings);
        for strength in &mut paint.glow {
            seed = crate::state::noise_hash(seed);
            *strength = if seed % 97 == 0 { 1.0 } else { 0.0 };
        }
        let expected: Vec<u32> = reference(&paint.glow, width, height).iter().map(|value| value.to_bits()).collect();
        let actual: Vec<u32> = paint.distance_field().iter().map(|value| value.to_bits()).collect();
        assert_eq!(actual, expected, "{width}x{height}");
    }
}

#[test]
fn card_template_matches_direct_bubble_and_scanline_painting() {
    for (scale, logical_height) in [(1.0, 240.0), (1.25, 263.0), (1.5, 300.0), (2.0, 310.0)] {
        let (width, height) = ((320.0 * scale) as usize, (logical_height * scale) as usize);
        let bottom = height as f32 / scale - 152.0;
        let template = CardTemplate::new(width, height, scale, bottom);
        for fill in [0x191f2a, 0x000000, 0xffffff, 0x080908, 0xff00ff] {
            let mut expected = vec![0xff00ff; width * height];
            let mut expected_coverage = PaintCoverage::new(vec![false; width * height], PixelFormat::ColorKeyRgb);
            draw_bubble(&mut expected, width, height, scale, bottom, fill, Some(&mut expected_coverage));
            shade_bubble_scanlines(&mut expected, width, height, scale, bottom);
            let mut actual = vec![0xff00ff; width * height];
            let mut coverage = PaintCoverage::new(vec![false; width * height], PixelFormat::ColorKeyRgb);
            template.paint(&mut actual, fill, Some(&mut coverage));
            template.shade(&mut actual);
            assert!(actual == expected, "Template pixels differ at scale {scale}, fill {fill:06x}");
            assert!(coverage.pixels == expected_coverage.pixels, "Template coverage differs at scale {scale}");
        }
    }
}

#[test]
fn every_exposed_parameter_changes_actual_lightning_pixels() {
    let render = |lightning: LightningSettings| {
        let mut paint = LightningPaint::new(320, 260, lightning);
        paint.bolt([15.0, 235.0], [290.0, 25.0], 1.0, 567891, 0.7, 1.0);
        let mut pixels = vec![0xff00ff; 320 * 260];
        paint.composite(&mut pixels, PixelFormat::ColorKeyRgb);
        let mut renderer = Renderer::with_settings(&Settings { lightning, ..Settings::default() }).unwrap();
        for time in [1000, 1250, 1500, 1750, 2000] {
            let mut frame = vec![0; 320 * 260];
            renderer.lightning_activity = Some(LightningActivity::Holding(Duration::from_millis(time), Duration::from_secs(5), 1234));
            renderer.draw(&mut frame, 320, 260, 1.0, None, 0.0, None);
            pixels.extend(frame);
        }
        pixels
    };
    let original = LightningSettings::default();
    let baseline = render(original);
    for spec in crate::lightning::PARAMETERS {
        let mut settings = original;
        spec.parameter.set(&mut settings, spec.max);
        settings.validate().unwrap();
        let pixels = render(settings);
        assert_eq!(pixels, render(settings));
        assert!(pixels.iter().zip(&baseline).filter(|(a, b)| a != b).count() > 50, "{} had no useful pixel effect", spec.label);
    }
}

#[test]
fn all_presets_preserve_characterized_bolts_entrance_holding_and_exit_pixels() {
    use sha2::{Digest, Sha256};
    let pins = [
        "608ba67c3018469631b55ebbb5fd77018637829db015a5ef20734f1b527c54b8",
        "f857c6fdea2c8e3103ee6017723fba47d3b8f539059c2a5927524d7ec98b4bca",
        "7f6d4b900cb07c67a7464811615122899ca1b9205b811626250d9f5b6f955e75",
        "1f94a0ab398424f0dcbb9a47f9cec1baab71185e2b80f6ede63bc57dd7eb11fa",
    ];
    for (preset, expected) in crate::lightning::PRESETS.iter().zip(pins) {
        let mut digest = Sha256::new();
        let mut pin = |pixels: &[u32]| {
            let bytes = pixels.iter().flat_map(|pixel| pixel.to_le_bytes()).collect::<Vec<_>>();
            digest.update(format!("{:x}", Sha256::digest(bytes)).as_bytes());
        };
        for (scale, seed, reveal) in [(0.75, 1234, 0.35), (1.0, 567891, 1.0), (1.5, 99, 1.0)] {
            let mut paint = LightningPaint::new(320, 260, preset.settings);
            paint.bolt([15.0, 235.0], [290.0, 25.0], scale, seed, preset.settings.pulse_profile.entrance(160.0), reveal);
            let mut pixels = vec![0xff00ff; 320 * 260];
            paint.composite(&mut pixels, PixelFormat::ColorKeyRgb);
            pin(&pixels);
        }
        let monitor = PhysicalRect { x: -800, y: -200, width: 800, height: 1100 };
        for scale in [1.0, 1.5] {
            let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 250, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
            for seed in [1234, 1235] {
                let scene = EntranceScene::new(monitor, card, seed, preset.settings);
                let mut renderer = Renderer::with_settings(&Settings { lightning: preset.settings, ..Settings::default() }).unwrap();
                for time in [60, 140, 260, 480] {
                    renderer.seed = 567891;
                    pin(&renderer.draw_scene(&scene, None, 0.0, Some(Duration::from_millis(time))));
                }
                let duration = Duration::from_secs(5);
                let active = (3000..4500).find(|time| ambient_burst(Duration::from_millis(*time), duration, seed, preset.settings).is_some_and(|burst| burst.intensity > 0.8)).unwrap();
                for activity in [
                    LightningActivity::Holding(Duration::from_millis(700), duration, seed),
                    LightningActivity::Holding(Duration::from_millis(active), duration, seed),
                    LightningActivity::Closing(Duration::from_millis(300), seed),
                    LightningActivity::Closing(Duration::from_millis(650), seed),
                ] {
                    renderer.seed = 567891;
                    renderer.lightning_activity = Some(activity);
                    pin(&renderer.draw_scene(&scene, None, 0.0, None));
                }
            }
        }
        assert_eq!(format!("{:x}", digest.finalize()), expected, "{} changed its characterized output", preset.label);
    }
}

#[test]
fn saved_reload_changes_the_next_snapshot_without_changing_an_active_scene() {
    let data = std::env::temp_dir().join("opencode").join(format!("herald-lightning-snapshot-{}-{}", std::process::id(), crate::state::timestamp()));
    let mut settings = Settings::default();
    settings.save(&data).unwrap();
    let mut store = crate::settings::Store::new(&data).unwrap();
    let mut renderer = Renderer::with_settings(&store.current).unwrap();
    let monitor = PhysicalRect { x: 0, y: 0, width: 420, height: 380 };
    let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: 20, y: 20, width: 320, height: 260 }, scale: 1.0 };
    let active = EntranceScene::new(monitor, card, 1234, store.current.lightning);
    let before = renderer.draw_scene(&active, None, 0.0, Some(Duration::from_millis(260)));
    settings.lightning = crate::lightning::PRESETS[1].settings;
    settings.save(&data).unwrap();
    assert!(store.reload().unwrap());
    assert_eq!(active.lightning, LightningSettings::default());
    assert_eq!(renderer.lightning, active.lightning);
    renderer.seed = 567891;
    assert_eq!(renderer.draw_scene(&active, None, 0.0, Some(Duration::from_millis(260))), before);
    renderer.set_lightning(store.current.lightning);
    let next = EntranceScene::new(monitor, card, 1234, store.current.lightning);
    renderer.seed = 567891;
    assert_ne!(renderer.draw_scene(&next, None, 0.0, Some(Duration::from_millis(260))), before);
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
#[should_panic]
fn mismatched_scene_and_renderer_snapshots_are_rejected() {
    let mut renderer = Renderer::new().unwrap();
    let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: 20, y: 20, width: 320, height: 260 }, scale: 1.0 };
    let scene = EntranceScene::new(PhysicalRect { x: 0, y: 0, width: 420, height: 380 }, card, 1234, crate::lightning::PRESETS[1].settings);
    renderer.draw_scene(&scene, None, 0.0, Some(Duration::from_millis(140)));
}

#[test]
fn incoming_bolt_uses_the_scene_snapshot_for_leader_and_return_stroke() {
    let monitor = PhysicalRect { x: -800, y: -200, width: 800, height: 1100 };
    for preset in crate::lightning::PRESETS {
        let lightning = preset.settings;
        for scale in [1.0, 1.5] {
            let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 250, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
            for seed in [1234, 1235] {
                let mut scene = EntranceScene::new(monitor, card, seed, lightning);
                scene.channel = vec![scene.source, scene.impact];
                scene.forks.clear();
                let width = scene.canvas.width as usize;
                let height = scene.canvas.height as usize;
                let from = [scene.source[0] - scene.canvas.x as f32, scene.source[1] - scene.canvas.y as f32];
                let impact = [scene.impact[0] - scene.canvas.x as f32, scene.impact[1] - scene.canvas.y as f32];
                for (time, reveal, intensity) in [(60, 0.5, 0.65), (160, 1.0, lightning.pulse_profile.entrance(160.0))] {
                    let to = [from[0] + (impact[0] - from[0]) * reveal, from[1] + (impact[1] - from[1]) * reveal];
                    let mut paint = LightningPaint::new(width, height, lightning);
                    paint.stroke(from, to, scale, 0.9, intensity);
                    let mut expected = vec![0xff00ff; width * height];
                    paint.composite(&mut expected, PixelFormat::ColorKeyRgb);
                    let mut pixels = vec![0xff00ff; width * height];
                    scene.draw_incoming_bolt(&mut pixels, Duration::from_millis(time));
                    assert_eq!(pixels, expected, "Wrong incoming paint for {} at {time}ms", preset.label);
                    assert_ne!(pixels, vec![0xff00ff; width * height]);
                }
                for time in [0, 650] {
                    let mut pixels = vec![0xff00ff; width * height];
                    scene.draw_incoming_bolt(&mut pixels, Duration::from_millis(time));
                    assert!(pixels.iter().all(|pixel| *pixel == 0xff00ff));
                }
            }
        }
    }
}

#[test]
fn lightning_variations_are_distinct_and_repeatable() {
    let render = |lightning: LightningSettings| {
        let mut paint = LightningPaint::new(160, 100, lightning);
        paint.bolt([15.0, 85.0], [145.0, 15.0], 1.0, 1234, lightning.pulse_profile.entrance(160.0), 1.0);
        let mut pixels = vec![0xff00ff; 160 * 100];
        paint.composite(&mut pixels, PixelFormat::ColorKeyRgb);
        pixels
    };
    let settings = crate::lightning::PRESETS.map(|preset| preset.settings);
    for (index, lightning) in settings.into_iter().enumerate() {
        let pixels = render(lightning);
        assert_eq!(pixels, render(lightning));
        assert!(pixels.iter().any(|pixel| *pixel != 0xff00ff));
        assert_eq!(pixels[0], 0xff00ff);
        for other in &settings[..index] { assert_ne!(pixels, render(*other)); }
    }
}

#[test]
fn lightning_variations_keep_screen_edge_sources_and_video_impacts() {
    let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1400 };
    for preset in &crate::lightning::PRESETS[1..] {
        for scale in [1.0, 1.5, 2.0] {
            let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 300, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
            for seed in 0..40 {
                let scene = EntranceScene::new(monitor, card, seed, preset.settings);
                assert!(scene.source[0] == -1.0 || scene.source[1] == 1199.0);
                assert_eq!(scene.channel.last(), Some(&scene.impact));
                assert!(scene.canvas.contains(card.rect.x, card.rect.y));
                assert!(scene.canvas.contains(card.rect.right() - 1, card.rect.bottom() - 1));
                for point in scene.channel.iter().chain(scene.forks.iter().flat_map(|(_, points, _)| points)) {
                    assert!(monitor.contains(point[0] as i32, point[1] as i32));
                }
            }
        }
    }
}

#[test]
fn sources_use_only_bottom_and_right_edges_across_seeds_and_scales() {
    for scale in [1.0, 1.5, 2.0] {
        let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1400 };
        let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 300, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
        for seed in 0..40 {
            let scene = EntranceScene::new(monitor, card, seed, LightningSettings::default());
            assert!(scene.source[0] == -1.0 || scene.source[1] == 1199.0);
            assert!(scene.source[0] > monitor.x as f32 && scene.source[1] > monitor.y as f32);
            assert!(scene.canvas.contains(card.rect.x, card.rect.y));
            assert!(scene.canvas.contains(card.rect.right() - 1, card.rect.bottom() - 1));
            assert_eq!(scene.channel.last(), Some(&scene.impact));
        }
    }
}

#[test]
fn exit_flashes_never_restore_erased_geometry() {
    for scale in [1.0, 1.5] {
        let width = (320.0 * scale) as usize;
        let height = (260.0 * scale) as usize;
        let mut renderer = Renderer::new().unwrap();
        let mut previous = vec![0; width * height];
        renderer.draw(&mut previous, width, height, scale, None, 0.0, None);
        for time in (300..=650).step_by(25) {
            renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(time), 1234));
            let mut frame = vec![0; previous.len()];
            renderer.draw(&mut frame, width, height, scale, None, 0.0, None);
            assert!(previous.iter().zip(&frame).all(|(old, new)| *old != 0xff00ff || *new == 0xff00ff), "A flash restored vanished geometry at {time}ms");
            previous = frame;
        }
        assert!(previous.iter().all(|pixel| *pixel == 0xff00ff));
    }
}

#[test]
fn exit_consumes_geometry_monotonically_at_brightness_and_glow_bounds() {
    for brightness in [0.1, 2.0] {
        for glow_strength in [0.05, 0.6] {
            let mut settings = Settings::default();
            settings.lightning.brightness = brightness;
            settings.lightning.glow_strength = glow_strength;
            let mut renderer = Renderer::with_settings(&settings).unwrap();
            let mut previous = usize::MAX;
            for time in [300, 400, 450, 500, 550, 600, 650] {
                renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(time), 1234));
                let mut frame = vec![0; 320 * 260];
                renderer.draw(&mut frame, 320, 260, 1.0, None, 0.0, None);
                let remaining = frame.iter().filter(|pixel| **pixel != 0xff00ff).count();
                assert!(remaining <= previous, "Exit restored geometry at brightness {brightness} and glow strength {glow_strength} at {time}ms");
                previous = remaining;
            }
            assert_eq!(previous, 0, "Exit did not consume geometry at brightness {brightness} and glow strength {glow_strength}");
        }
    }
}

#[test]
fn sporadic_flashes_stay_near_the_outline_without_projecting_outward() {
    let mut renderer = Renderer::new().unwrap();
    let mut plain = vec![0; 320 * 260];
    renderer.draw(&mut plain, 320, 260, 1.0, None, 0.0, None);
    let coverage = PaintCoverage::new(plain.iter().map(|pixel| *pixel != 0xff00ff).collect(), PixelFormat::ColorKeyRgb);
    let mut early = LightningPaint::new(320, 260, LightningSettings::default());
    let mut late = LightningPaint::new(320, 260, LightningSettings::default());
    draw_ambient_edges(&coverage, &mut early, 1.0, 1234, 1.0, 0.0);
    draw_ambient_edges(&coverage, &mut late, 1.0, 1234, 1.0, 1.0);
    let lit = |paint: &mut LightningPaint| { paint.rasterize(); paint.glow.iter().filter(|strength| **strength > 0.2).count() };
    assert!(lit(&mut late) > lit(&mut early) * 3);
    let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: 400, y: 400, width: 320, height: 260 }, scale: 1.0 };
    let scene = EntranceScene::new(PhysicalRect { x: 0, y: 0, width: 1200, height: 900 }, card, 1234, LightningSettings::default());
    let time = (3000..4500).find(|time| ambient_burst(Duration::from_millis(*time), Duration::from_secs(5), 1234, LightningSettings::default()).is_some_and(|burst| burst.intensity > 0.8)).unwrap();
    renderer.seed = 567891;
    renderer.lightning_activity = Some(LightningActivity::Holding(Duration::from_millis(time), Duration::from_secs(5), 1234));
    let frame = renderer.draw_scene(&scene, None, 0.0, None);
    for (index, pixel) in plain.iter().enumerate() {
        if *pixel != 0xff00ff { continue; }
        assert_eq!(frame[index], *pixel, "Lightning projected outside the surface");
    }
    for (left, top, right, bottom) in [(60, 25, 260, 85), (210, 140, 290, 220)] {
        let changed = (top..bottom).flat_map(|y| (left..right).map(move |x| y * 320 + x))
            .filter(|index| frame[*index] != plain[*index]).count();
        assert_eq!(changed, 0, "An edge flash crossed the surface interior");
    }
}

#[test]
fn holding_frequency_and_speed_control_event_count_and_flash_duration() {
    let sample = |lightning: LightningSettings| {
        let mut count = 0;
        let mut visible = 0;
        let mut previous = None;
        for time in 0..30000 {
            if let Some(AmbientBurst { seed, .. }) = ambient_burst(lightning.transition_duration() + Duration::from_millis(time), lightning.transition_duration() + Duration::from_secs(30), 1234, lightning) {
                visible += 1;
                if previous != Some(seed) { count += 1; }
                previous = Some(seed);
            }
        }
        (count, visible)
    };
    let baseline = sample(LightningSettings { frequency: 1.0, ..LightningSettings::default() });
    let frequent = sample(LightningSettings::default());
    let fast = sample(LightningSettings { frequency: 1.0, speed: 2.0, ..LightningSettings::default() });
    assert!(frequent.0 > baseline.0 * 2);
    assert_eq!(fast.0, baseline.0);
    assert!((fast.1 as f32 / baseline.1 as f32 - 0.5).abs() < 0.02);
}

#[test]
fn speed_scales_the_complete_entrance_and_exit() {
    let monitor = PhysicalRect { x: 0, y: 0, width: 800, height: 700 };
    let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: 400, y: 300, width: 320, height: 260 }, scale: 1.0 };
    let render = |speed, time, closing: bool| {
        let lightning = LightningSettings { speed, ..LightningSettings::default() };
        let scene = EntranceScene::new(monitor, card, 1234, lightning);
        let mut renderer = Renderer::with_settings(&Settings { lightning, ..Settings::default() }).unwrap();
        let elapsed = Duration::from_secs_f32(time);
        renderer.lightning_activity = closing.then_some(LightningActivity::Closing(elapsed, 1234));
        renderer.draw_scene(&scene, None, 0.0, (!closing).then_some(elapsed))
    };
    for time in [0.06, 0.14, 0.26, 0.48, 0.6] {
        let baseline = render(1.0, time, false);
        for speed in [0.25, 0.5, 2.0, 4.0] {
            assert_eq!(render(speed, time / speed, false), baseline);
        }
    }
    assert_ne!(render(0.5, 0.26, false), render(2.0, 0.26, false));
    for time in [0.06, 0.14, 0.3, 0.48, 0.65] {
        let baseline = render(1.0, time, true);
        for speed in [0.25, 0.5, 2.0, 4.0] {
            assert_eq!(render(speed, time / speed, true), baseline);
        }
    }
    assert_ne!(render(0.5, 0.3, true), render(2.0, 0.3, true));
}

#[test]
fn sporadic_bolts_are_fully_visible_and_stationary_during_each_flash() {
    let duration = Duration::from_secs(5);
    let mut previous = None;
    for milliseconds in 650..5000 {
        if let Some(AmbientBurst { seed, buildup, .. }) = ambient_burst(Duration::from_millis(milliseconds), duration, 1234, LightningSettings::default()) {
            if let Some((last_seed, last_buildup)) = previous {
                if seed == last_seed { assert_eq!(buildup, last_buildup); }
            }
            previous = Some((seed, buildup));
        }
    }
}

#[test]
fn sporadic_timing_has_uneven_gaps_and_quick_clusters() {
    let duration = Duration::from_secs(30);
    let mut starts = Vec::new();
    let mut last_seed = None;
    for time in (650..30000).step_by(5) {
        if let Some(AmbientBurst { seed, .. }) = ambient_burst(Duration::from_millis(time), duration, 1234, LightningSettings::default()) {
            if last_seed != Some(seed) { starts.push(time); }
            last_seed = Some(seed);
        }
    }
    let intervals: Vec<_> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(starts.len() >= 90 && starts.len() <= 150, "Expected frequent holding flashes, got {}", starts.len());
    assert!(intervals.iter().any(|gap| *gap < 300));
    assert!(intervals.iter().any(|gap| *gap > 300));
    assert!(intervals.iter().max().unwrap() - intervals.iter().min().unwrap() > 200);
}

#[test]
fn ambient_arcs_build_up_over_short_and_long_announcements() {
    for seconds in [5, 30] {
        let duration = Duration::from_secs(seconds);
        let total = duration.as_millis() as u64 - 650;
        let mut samples = [0, 0];
        let mut bursts = [0, 0];
        for notification in 0..32 {
            for (region, start) in [650, 650 + total * 2 / 3].into_iter().enumerate() {
                let mut previous_seed = None;
                for time in (start..start + total / 3).step_by(5) {
                    if let Some(AmbientBurst { seed, .. }) = ambient_burst(Duration::from_millis(time), duration, notification, LightningSettings::default()) {
                        samples[region] += 1;
                        if previous_seed != Some(seed) { bursts[region] += 1; }
                        previous_seed = Some(seed);
                    }
                }
            }
        }
        assert!(samples[1] > samples[0], "Later arcs should stay visible longer");
        assert!(bursts[1] >= bursts[0], "Later arcs should arrive more frequently");
    }
}

#[test]
fn holding_has_frequent_bursts_and_exit_consumes_both_interiors() {
    let mut renderer = Renderer::new().unwrap();
    let mut plain = vec![0; 320 * 260];
    renderer.draw(&mut plain, 320, 260, 1.0, None, 0.0, None);
    let seed = 1234;
    let duration = Duration::from_secs(5);
    let active: Vec<_> = (650..5000).step_by(20).filter(|time| ambient_burst(Duration::from_millis(*time), duration, seed, LightningSettings::default()).is_some()).collect();
    assert!(active.len() >= 100 && active.len() < 210);
    let inactive = (650..5000).find(|time| ambient_burst(Duration::from_millis(*time), duration, seed, LightningSettings::default()).is_none()).unwrap();
    let peak = (650..5000).find(|time| ambient_burst(Duration::from_millis(*time), duration, seed, LightningSettings::default()).is_some_and(|burst| burst.intensity > 0.8)).unwrap();
    for time in [inactive, peak] {
        let mut pixels = vec![0; plain.len()];
        renderer.seed = 567891;
        renderer.lightning_activity = Some(LightningActivity::Holding(Duration::from_millis(time), duration, seed));
        renderer.draw(&mut pixels, 320, 260, 1.0, None, 0.0, None);
        assert_eq!(pixels == plain, ambient_burst(Duration::from_millis(time), duration, seed, LightningSettings::default()).is_none());
        assert!(pixels.iter().zip(&plain).all(|(pixel, before)| *before != 0xff00ff || pixel == before));
    }
    let mut remaining = Vec::new();
    for time in [0, 300, 400, 650] {
        let mut pixels = vec![0; plain.len()];
        renderer.seed = 567891;
        renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(time), seed));
        renderer.draw(&mut pixels, 320, 260, 1.0, None, 0.0, None);
        remaining.push(pixels.iter().filter(|pixel| **pixel != 0xff00ff).count());
        if time == 300 {
            for (top, bottom) in [(25, 80), (145, 200)] {
                let lit = (top..bottom).flat_map(|y| (210..285).map(move |x| y * 320 + x))
                    .filter(|index| pixels[*index] != 0xff00ff && pixels[*index] & 255 > 180).count();
                assert!(lit > 20, "Exit did not cross the interior");
            }
        }
    }
    assert!(remaining[0] > remaining[1] && remaining[1] > remaining[2]);
    assert_eq!(remaining[3], 0);
}

#[test]
fn seeded_screen_edge_channel_is_stable_forked_tapered_and_outside_the_card_until_video_hit() {
    let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: -504, y: 250, width: 480, height: 390 }, scale: 1.5 };
    let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1080 };
    let scene = EntranceScene::new(monitor, card, 7345, LightningSettings::default());
    let repeated = EntranceScene::new(monitor, card, 7345, LightningSettings::default());
    assert_eq!(scene.channel, repeated.channel);
    assert_eq!(scene.forks, repeated.forks);
    assert!(scene.channel.len() > 10 && !scene.forks.is_empty());
    assert_eq!(scene.channel[0][0], -1.0);
    assert_eq!(*scene.channel.last().unwrap(), [-30.75, 535.0]);
    for points in std::iter::once(&scene.channel).chain(scene.forks.iter().map(|(_, points, _)| points)) {
        for pair in points.windows(2) {
            for fraction in [0.0, 0.25, 0.5, 0.75] {
                let x = (pair[0][0] + (pair[1][0] - pair[0][0]) * fraction - card.rect.x as f32) / card.scale;
                let y = (pair[0][1] + (pair[1][1] - pair[0][1]) * fraction - card.rect.y as f32) / card.scale;
                assert!(!bubble_contains(x, y, 108.0), "A premature fork hit the bubble at {x},{y}");
                assert!(!(188.5..315.5).contains(&x) || !(120.5..247.5).contains(&y), "A fork hit the video before the main channel");
            }
        }
    }
    let mut early = vec![0xff00ff; scene.canvas.width as usize * scene.canvas.height as usize];
    scene.draw_incoming_bolt(&mut early, Duration::from_millis(60));
    let mut later = vec![0xff00ff; early.len()];
    scene.draw_incoming_bolt(&mut later, Duration::from_millis(100));
    assert!(early.iter().filter(|color| **color != 0xff00ff).count() > 1000);
    assert!(early.iter().zip(&later).filter(|(a, b)| **a != 0xff00ff && a == b).count() > 1000, "Leader regenerated instead of revealing a planted channel");
    assert!(scene.forks.iter().all(|(_, _, width)| *width < 0.9));
    let mut paint = LightningPaint::new(100, 40, LightningSettings::default());
    paint.stroke([10.0, 20.0], [90.0, 20.0], 1.0, 1.0, 1.0);
    let mut pixels = vec![0xff00ff; 4000];
    paint.composite(&mut pixels, PixelFormat::ColorKeyRgb);
    assert_eq!(pixels[20 * 100 + 50], 0xf7fbff);
    assert!(pixels[23 * 100 + 50] & 255 > (pixels[23 * 100 + 50] >> 16) & 255);
    assert_eq!(pixels[30 * 100 + 50], 0xff00ff);
}

#[test]
fn disconnected_bubble_receives_charge_through_its_tail_after_the_video() {
    let mut pixels = vec![0xff00ff; 320 * 260];
    let mut coverage = PaintCoverage::new(vec![false; pixels.len()], PixelFormat::ColorKeyRgb);
    draw_bubble(&mut pixels, 320, 260, 1.0, 88.0, 0x191f2a, Some(&mut coverage));
    for y in 120..248 {
        for x in 188..316 { coverage.pixels[y * 320 + x] = true; pixels[y * 320 + x] = 0x191f2a; }
    }
    let plain = pixels.clone();
    coverage.draw_entrance_lightning(&mut pixels, 320, 260, 1.0, Duration::from_millis(200), [187.5, 169.5], [252.0, 126.0], LightningSettings::default());
    assert_eq!(&pixels[..110 * 320], &plain[..110 * 320]);
    assert!(pixels[170 * 320 + 188] & 255 > 200);
    pixels.copy_from_slice(&plain);
    coverage.draw_entrance_lightning(&mut pixels, 320, 260, 1.0, Duration::from_millis(260), [187.5, 169.5], [252.0, 126.0], LightningSettings::default());
    assert!(pixels[110 * 320 + 252] & 255 > 100, "No visible transfer to the bubble tail");
    pixels.copy_from_slice(&plain);
    coverage.draw_entrance_lightning(&mut pixels, 320, 260, 1.0, Duration::from_millis(480), [187.5, 169.5], [252.0, 126.0], LightningSettings::default());
    let bright_top = (3..10).flat_map(|y| (40..280).map(move |x| y * 320 + x))
        .filter(|index| pixels[*index] & 255 > 220).count();
    assert!(bright_top >= 100, "Charge never reached the full far bubble edge");
}

#[test]
fn restored_scene_is_identical_to_the_normal_card_without_changing_the_portrait_stream() {
    let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: 700, y: 500, width: 320, height: 260 }, scale: 1.0 };
    let scene = EntranceScene::new(PhysicalRect { x: 0, y: 0, width: 1200, height: 900 }, card, 1, LightningSettings::default());
    let mut renderer = Renderer::new().unwrap();
    let mut normal = vec![0; 320 * 260];
    renderer.draw(&mut normal, 320, 260, 1.0, None, 0.0, None);
    renderer.seed = 567891;
    assert_eq!(renderer.draw_scene(&scene, None, 0.0, None), normal);
}

#[test]
fn screen_edge_leader_hits_video_before_bidirectional_full_outline_charge() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = "The task passed.".into();
    renderer.title = "Checks passed".into();
    let image = RgbaImage::from_pixel(128, 128, image::Rgba([70, 40, 20, 255]));
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for logical_height in [260, 340] {
            let width = (320.0 * scale) as usize;
            let height = (logical_height as f32 * scale) as usize;
            let bottom = logical_height as f32 - 152.0;
            let card = CardPlacement { style: crate::render::AnnouncementStyle::Video, rect: PhysicalRect { x: 1400, y: 600, width: width as u32, height: height as u32 }, scale };
            let scene = EntranceScene::new(PhysicalRect { x: -200, y: -100, width: 2600, height: 1700 }, card, 1234, LightningSettings::default());
            let zones = [(150.0, 6.0), (6.0, 50.0), (100.0, bottom), (244.0, bottom + 8.0),
                (294.0, bottom + 12.0), (188.0, bottom + 76.0), (315.0, bottom + 76.0), (252.0, bottom + 139.0)];
            for milliseconds in [60, 140, 200, 480] {
                renderer.seed = 567891;
                let mut plain = vec![0; width * height];
                renderer.draw(&mut plain, width, height, scale, Some(&image), 0.0, None);
                renderer.seed = 567891;
                let struck = renderer.draw_scene(&scene, Some(&image), 0.0, Some(Duration::from_millis(milliseconds)));
                let offset_x = (card.rect.x - scene.canvas.x) as usize;
                let offset_y = (card.rect.y - scene.canvas.y) as usize;
                let scene_width = scene.canvas.width as usize;
                let mut reached = [false; 8];
                let mut white = 0;
                for (index, &before) in plain.iter().enumerate() {
                    let after = struck[(offset_y + index / width) * scene_width + offset_x + index % width];
                    if before == after { continue; }
                    let x = (index % width) as f32 / scale;
                    let y = (index / width) as f32 / scale;
                    let red = (after >> 16) & 255;
                    let green = (after >> 8) & 255;
                    let blue = after & 255;
                    white += usize::from(red > 200 && green > 200 && blue > 200);
                    if before == 0xff00ff { assert!(blue >= red && blue >= green, "Clear pixel acquired a magenta halo"); }
                    assert!(!(40.0..290.0).contains(&x) || !(26.0..bottom - 22.0).contains(&y), "Text interior changed at {x},{y}");
                    assert!(!(214.0..290.0).contains(&x) || !(bottom + 38.0..bottom + 114.0).contains(&y), "Portrait center changed at {x},{y}");
                    for (zone, &(zx, zy)) in zones.iter().enumerate() {
                        if (x - zx).abs() < 6.0 && (y - zy).abs() < 6.0 && blue > red + 10 {
                            reached[zone] = true;
                        }
                    }
                }
                if milliseconds == 60 {
                    assert_eq!(reached, [false; 8]);
                    let source_x = (scene.source[0] - scene.canvas.x as f32) as usize;
                    let source_y = (scene.source[1] - scene.canvas.y as f32) as usize;
                    assert!(struck[source_y * scene_width + source_x] & 255 > 100, "Leader is not planted on the screen edge");
                } else if milliseconds == 140 {
                    assert!(!reached[0] && !reached[1] && !reached[2]);
                    assert!(white > 5, "No video impact flare");
                } else if milliseconds == 200 {
                    assert!(reached[7], "Charge did not leave the hit along the video");
                    assert!(!reached[0], "Bubble lit before the charge reached it");
                    for direction in [-25.0, 25.0] {
                        let y = ((logical_height as f32 - 13.0) * scale).round() as usize;
                        let x = ((252.0 + direction) * scale) as usize;
                        let radius = (3.0 * scale).ceil() as usize;
                        assert!((y - radius..=y + radius).any(|row| struck[(offset_y + row) * scene_width + offset_x + x] & 255 > 150), "Charge did not propagate in both directions");
                    }
                } else {
                    assert_eq!(reached, [true; 8], "Missing complete outline at scale {scale}, height {logical_height}");
                    assert!(white > 50);
                    let mut coverage = PaintCoverage::new(vec![false; width * height], PixelFormat::ColorKeyRgb);
                    let mut mask = vec![0; width * height];
                    draw_bubble(&mut mask, width, height, scale, bottom, 0, Some(&mut coverage));
                    for y in ((logical_height as f32 - 140.0) * scale) as usize..((logical_height as f32 - 12.0) * scale) as usize {
                        for x in (188.0 * scale) as usize..(316.0 * scale) as usize { coverage.pixels[y * width + x] = true; }
                    }
                    for contour in coverage.contours(width, height).iter() {
                        let lit = contour.iter().filter(|point| {
                            let radius = (3.0 * scale).ceil() as i32;
                            (-radius..=radius).any(|dy| (-radius..=radius).any(|dx| {
                                let x = point[0].round() as i32 + dx;
                                let y = point[1].round() as i32 + dy;
                                if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 { return false; }
                                let color = struck[(offset_y + y as usize) * scene_width + offset_x + x as usize];
                                color & 255 >= 180 && (color >> 8) & 255 >= 130 && (color >> 16) & 255 >= 100
                            }))
                        }).count();
                        assert!(lit * 100 >= contour.len() * 95, "Only {lit}/{} painted perimeter points energized", contour.len());
                    }
                }
            }
        }
    }
}

#[test]
fn entrance_lightning_is_animated_deterministic_and_leaves_the_portrait_random_stream_alone() {
    let mut renderer = Renderer::new().unwrap();
    let mut plain = vec![0; 320 * 260];
    renderer.draw(&mut plain, 320, 260, 1.0, None, 0.62, None);
    let mut next_plain = vec![0; plain.len()];
    renderer.draw(&mut next_plain, 320, 260, 1.0, None, 0.0, None);
    let mut struck = vec![0; plain.len()];
    renderer.seed = 567891;
    renderer.draw(&mut struck, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(140)));
    assert!(struck.iter().filter(|pixel| (**pixel & 255) > 220 && ((**pixel >> 16) & 255) > 220).count() >= 10);
    let mut next_struck = vec![0; plain.len()];
    renderer.draw(&mut next_struck, 320, 260, 1.0, None, 0.0, None);
    assert_eq!(next_plain, next_struck);
    let mut repeated = vec![0; plain.len()];
    renderer.seed = 567891;
    renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(140)));
    assert_eq!(struck, repeated);
    renderer.seed = 567891;
    renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(240)));
    assert!(struck.iter().zip(&repeated).filter(|(a, b)| a != b).count() >= 300);
    for milliseconds in [0, 650, 900, 5000] {
        renderer.seed = 567891;
        renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(milliseconds)));
        assert_eq!(plain, repeated, "Lightning outside its envelope at {milliseconds}ms");
    }
}

#[test]
fn announcement_height_fits_the_last_word_at_windows_display_scales() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = display_text("[excited] Committed and deployed announcement cleanup and text-editing improvements! Checks passed!");
    for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5] {
        let height = renderer.message_height(scale) + 238;
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 26.0 * scale,
            y: 26.0 * scale,
            max_width: Some(268.0 * scale),
            ..LayoutSettings::default()
        });
        layout.append(&[&renderer.body_font], &TextStyle::new(&renderer.text, f32::from(renderer.body_preference.size) * scale, 0));
        let cutoff = (height as f32 - 198.0) * scale;
        for glyph in layout.glyphs() {
            assert!(glyph.y + glyph.height as f32 <= cutoff, "The final line is clipped at scale {scale}: {} > {cutoff}", glyph.y + glyph.height as f32);
        }
    }
}

#[test]
fn displayed_text_removes_tags_and_collapses_spaces() {
    for (input, expected) in [
        ("[excited] Added Ctrl+A  and preserved scroll position; verified.", "Added Ctrl+A and preserved scroll position; verified."),
        (" [happy]  Done [pause]   successfully. [laughs] ", "Done successfully."),
        ("[excited][laughs] Hello   ä¸–ç•Œ ðŸ˜€", "Hello ä¸–ç•Œ ðŸ˜€"),
        ("First  line.\nSecond   line.", "First line.\nSecond line."),
        ("Ordinary text.", "Ordinary text."),
        ("Keep [unfinished", "Keep [unfinished"),
        ("[pause] []", ""),
        ("", ""),
    ] {
        assert_eq!(display_text(input), expected);
    }
}

#[test]
fn display_cleanup_preserves_the_original_speech_text() {
    let notification: crate::state::Notification = serde_json::from_value(serde_json::json!({
        "id": "display-cleanup", "sessionID": "opencode", "completed": 1,
        "text": "[excited] The  task passed.", "title": "[happy] Checks   passed",
    })).unwrap();
    assert_eq!(display_text(&notification.text), "The task passed.");
    assert_eq!(display_text(&notification.title), "Checks passed");
    assert_eq!(notification.text, "[excited] The  task passed.");
    assert_eq!(notification.title, "[happy] Checks   passed");
}

#[test]
fn bubble_has_a_frame_and_keeps_the_footer_and_tail() {
    let mut buffer = vec![0xff00ff; 320 * 260];
    draw_bubble(&mut buffer, 320, 260, 1.0, 108.0, 0x191f2a, None);
    assert_eq!(buffer[6 * 320 + 150], 0x080908);
    assert_eq!(buffer[8 * 320 + 150], 0x191f2a);
    assert_eq!(buffer[30 * 320 + 314], 0x080908);
    assert_eq!(buffer[17 * 320 + 40], 0x191f2a);
    assert_eq!(buffer[62 * 320 + 40], blend(0x191f2a, 0xffffff, 30));
    assert_eq!(buffer[106 * 320 + 150], 0x191f2a);
    assert_eq!(buffer[108 * 320 + 150], 0x080908);
    assert_eq!(buffer[104 * 320 + 252], 0x191f2a);
    assert_eq!(buffer[122 * 320 + 252], 0x191f2a);
    assert_eq!(buffer[124 * 320 + 252], 0x080908);
    assert_eq!(buffer[30 * 320 + 6], 0x080908);
    assert_eq!(buffer[6 * 320 + 6], 0xff00ff);
    assert_eq!(buffer[30 * 320 + 315], 0x080908);
    assert_eq!(buffer[30 * 320 + 316], 0xff00ff);
}

#[test]
fn bubble_scales_without_changing_its_layout() {
    let mut normal = vec![0xff00ff; 320 * 260];
    let mut doubled = vec![0xff00ff; 640 * 520];
    draw_bubble(&mut normal, 320, 260, 1.0, 108.0, 0x191f2a, None);
    draw_bubble(&mut doubled, 640, 520, 2.0, 108.0, 0x191f2a, None);
    for y in 0..260 {
        for x in 0..320 {
            assert_eq!(normal[y * 320 + x], doubled[y * 2 * 640 + x * 2]);
        }
    }
}

#[test]
fn bubble_uses_the_average_of_both_video_top_corners() {
    let frame = RgbaImage::from_fn(128, 128, |x, y| {
        if y < 10 && x < 10 {
            image::Rgba([20, 30, 40, 255])
        } else if y < 10 && x > 117 {
            image::Rgba([40, 50, 60, 255])
        } else {
            image::Rgba([220, 180, 140, 255])
        }
    });
    assert_eq!(video_background(Some(&frame)), 0x1e2832);
    let mut renderer = Renderer::new().unwrap();
    let mut buffer = vec![0; 320 * 260];
    renderer.draw(&mut buffer, 320, 260, 1.0, Some(&frame), 0.0, None);
    assert_eq!(buffer[30 * 320 + 150], 0x182029);
}

#[test]
fn video_background_ignores_transparent_samples_and_handles_missing_frames() {
    let frame = RgbaImage::from_fn(128, 128, |x, _| {
        if x < 64 {
            image::Rgba([255, 0, 255, 0])
        } else {
            image::Rgba([20, 30, 40, 255])
        }
    });
    assert_eq!(video_background(Some(&frame)), 0x141e28);
    assert_eq!(video_background(None), 0x191f2a);
    assert_eq!(video_background(Some(&RgbaImage::new(0, 0))), 0x191f2a);
}

#[test]
fn renderer_scanlines_shade_bubble_contents_but_not_background() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = "Done.".into();
    renderer.title = "Session".into();
    let mut buffer = vec![0; 320 * 260];
    renderer.draw(&mut buffer, 320, 260, 1.0, None, 0.0, None);
    assert_eq!(buffer[30 * 320 + 150], 0x141922);
    assert_eq!(buffer[31 * 320 + 150], 0x191f2a);
    assert_eq!(buffer[110 * 320 + 252], 0x141922);
    assert_eq!(buffer[111 * 320 + 252], 0x191f2a);
    assert_eq!(buffer[62 * 320 + 40], 0x2a2e36);
    assert_eq!(buffer[0], 0xff00ff);
    for (font, text, y, size, color, cutoff) in [
        (&renderer.body_font, renderer.text.as_str(), 26.0, renderer.body_preference.size, 0xeef2f7, 62.0),
        (&renderer.title_font, renderer.title.as_str(), 72.0, renderer.title_preference.size, 0x9daabd, 72.0 + f32::from(renderer.title_preference.size) + 8.0),
    ] {
        let mut unshaded = vec![0; 320 * 260];
        draw_bubble(&mut unshaded, 320, 260, 1.0, 108.0, 0x191f2a, None);
        let background = unshaded.clone();
        renderer.text(&mut unshaded, 320, 260, 1.0, font, TextBlock {
            text, x: 26.0, width: 268.0, line_height: None, y, size: f32::from(size), color, max_height: cutoff,
        });
        let mut glyph_pixels = [0; 2];
        for index in 0..unshaded.len() {
            if unshaded[index] == background[index] { continue; }
            let row = index / 320;
            glyph_pixels[row % 2] += 1;
            let expected = if row % 2 == 0 {
                let pixel = unshaded[index];
                let red = (((pixel >> 16) & 255) as f32 * 0.82) as u32;
                let green = (((pixel >> 8) & 255) as f32 * 0.82) as u32;
                let blue = ((pixel & 255) as f32 * 0.82) as u32;
                (red << 16) | (green << 8) | blue
            } else { unshaded[index] };
            assert_eq!(buffer[index], expected, "Glyph pixel at {}, {row}", index % 320);
        }
        assert!(glyph_pixels[0] > 0 && glyph_pixels[1] > 0, "Missing glyph pixels on either scanline parity for {text}");
    }
}

#[test]
fn renderer_scanlines_follow_logical_rows_at_multiple_scales() {
    let mut renderer = Renderer::new().unwrap();
    for (scale, width, height, even_row, odd_row) in [
        (1.0, 320, 240, 30, 31),
        (1.5, 480, 360, 45, 47),
        (2.0, 640, 480, 60, 63),
    ] {
        let mut buffer = vec![0; width * height];
        renderer.draw(&mut buffer, width, height, scale, None, 0.0, None);
        let x = (150.0 * scale) as usize;
        assert_eq!(buffer[even_row * width + x], 0x141922);
        assert_eq!(buffer[odd_row * width + x], 0x191f2a);
    }
}

#[test]
fn text_tearing_leaves_bubble_and_picture_unchanged() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = "The task is complete.".into();
    renderer.title = "My session".into();
    let mut clean = vec![0; 320 * 240];
    renderer.draw(&mut clean, 320, 240, 1.0, None, 0.0, None);
    renderer.seed = 567891;
    renderer.text_interference = 0.62;
    let mut distorted = vec![0; 320 * 240];
    renderer.draw(&mut distorted, 320, 240, 1.0, None, 0.0, None);
    assert_ne!(clean, distorted);
    for y in 0..240 {
        for x in 0..320 {
            if !(26..75).contains(&y) || !(14..306).contains(&x) {
                assert_eq!(clean[y * 320 + x], distorted[y * 320 + x]);
            }
        }
    }
}

#[test]
fn scales_the_whole_large_video_into_the_portrait_frame() {
    let frame = RgbaImage::from_fn(256, 256, |x, y| {
        if x >= 128 && y >= 128 {
            image::Rgba([20, 40, 220, 255])
        } else {
            image::Rgba([220, 40, 20, 255])
        }
    });
    let mut renderer = Renderer::new().unwrap();
    let mut buffer = vec![0; 320 * 240];
    renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.0, None);
    let bottom_right = buffer[196 * 320 + 284];
    assert!(bottom_right & 255 > (bottom_right >> 16) & 255);
}

#[test]
fn renders_decoded_video_frames_without_a_file_cache() {
    let mut renderer = Renderer::new().unwrap();
    let mut buffer = vec![0; 320 * 240];
    for _ in 0..15 {
        let frame = RgbaImage::from_pixel(128, 128, image::Rgba([20, 40, 80, 255]));
        renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.0, None);
    }
    let center = buffer[164 * 320 + 252];
    assert!(center & 255 > (center >> 16) & 255);
}

#[test]
fn static_has_monochrome_grain_and_title_renders_below_message() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = "Done.".into();
    renderer.title = "My session".into();
    let mut buffer = vec![0; 320 * 240];
    renderer.draw(&mut buffer, 320, 240, 1.0, None, 1.0, None);
    let face: Vec<_> = (100..228)
        .flat_map(|y| (188..316).map(move |x| y * 320 + x))
        .map(|i| buffer[i])
        .collect();
    assert!(face.iter().any(|p| *p > 0x606060));
    assert!(face.iter().any(|p| *p < 0x404040));
    assert!(
        face.iter()
            .filter(|p| (**p & 255).abs_diff((**p >> 8) & 255) <= 2
                && (**p & 255).abs_diff((**p >> 16) & 255) <= 2)
            .count()
            > face.len() * 9 / 10
    );
    assert!((54..68)
        .flat_map(|y| (26..180).map(move |x| y * 320 + x))
        .any(|index| buffer[index] > 0x606060));
}

#[test]
fn interference_overlays_the_picture_instead_of_replacing_it() {
    let frame = RgbaImage::from_pixel(128, 128, image::Rgba([220, 60, 20, 255]));
    let mut renderer = Renderer::new().unwrap();
    let mut buffer = vec![0; 320 * 240];
    renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.82, None);
    let center = buffer[164 * 320 + 252];
    assert!((center >> 16) & 255 > center & 255);
    assert_eq!(buffer[100 * 320 + 188], 0x080908);
}

#[test]
fn interference_displaces_picture_features_before_adding_noise() {
    let frame = RgbaImage::from_fn(128, 128, |x, _| {
        image::Rgba([if (60..64).contains(&x) { 255 } else { 0 }, 0, 0, 255])
    });
    let mut clean = vec![0; 320 * 240];
    let mut distorted = vec![0; 320 * 240];
    Renderer::new()
        .unwrap()
        .draw(&mut clean, 320, 240, 1.0, Some(&frame), 0.0, None);
    Renderer::new()
        .unwrap()
        .draw(&mut distorted, 320, 240, 1.0, Some(&frame), 0.62, None);
    let peak = |buffer: &[u32], y: usize| {
        (210..290)
            .max_by_key(|x| (buffer[y * 320 + x] >> 16) & 255)
            .unwrap()
    };
    assert!((120..210).any(|y| peak(&clean, y).abs_diff(peak(&distorted, y)) >= 5));
}

#[test]
fn separate_font_sizes_change_body_height_and_preserve_unavailable_preferences() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = "A message that wraps across more than one line.".into();
    let body = FontPreference::new("Unavailable body family", 32);
    let title = FontPreference::new("Unavailable title family", 8);
    renderer.set_preferences(&body, &title);
    let large_height = renderer.message_height(1.0);
    assert_eq!(renderer.body_preference(), &body);
    assert_eq!(renderer.title_preference(), &title);
    renderer.set_preferences(&FontPreference::new("Unavailable body family", 8), &FontPreference::new("Unavailable title family", 32));
    assert!(renderer.message_height(1.0) < large_height);
    assert_eq!(renderer.body_preference().family, "Unavailable body family");
    assert_eq!(renderer.title_preference().family, "Unavailable title family");
}

#[test]
fn title_font_size_changes_title_rendering_without_changing_message_height() {
    let mut renderer = Renderer::new().unwrap();
    renderer.text = "Message".into();
    renderer.title = "A title".into();
    let mut small = vec![0; 320 * 240];
    renderer.set_preferences(&FontPreference::new("Unavailable body family", 18), &FontPreference::new("Unavailable title family", 8));
    let height = renderer.message_height(1.0);
    renderer.draw(&mut small, 320, 240, 1.0, None, 0.0, None);
    let mut large = vec![0; 320 * 240];
    renderer.set_preferences(&FontPreference::new("Unavailable body family", 18), &FontPreference::new("Unavailable title family", 32));
    assert_eq!(renderer.message_height(1.0), height);
    renderer.seed = 567891;
    renderer.draw(&mut large, 320, 240, 1.0, None, 0.0, None);
    assert_eq!(&small[..320 * 52], &large[..320 * 52]);
    assert_ne!(&small[320 * 52..320 * 92], &large[320 * 52..320 * 92]);
}
