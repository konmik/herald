use super::*;

#[test]
fn native_argb_conversion_preserves_each_formats_transparency() {
    let keyed = PixelFormat::ColorKeyRgb;
    assert_eq!(keyed.argb(0xff00ff), 0);
    assert_eq!(keyed.argb(0), 0xff000000);
    assert_eq!(keyed.argb(0x1c1b16), 0xff1c1b16);
    for color in [0, 0x80402010, 0xff1c1b16] {
        assert_eq!(PixelFormat::PremultipliedArgb.argb(color), color);
    }
}

#[test]
fn translucent_edges_blend_with_the_desktop_without_black_fringe() {
    let format = PixelFormat::PremultipliedArgb;
    let edge = format.over(format.clear(), 0x4fb6f2, 32);
    assert_eq!(edge >> 24, 32);
    let white = format.on_background(edge, 0xffffff);
    assert!((white >> 16) & 255 > 230);
    assert!((white >> 8) & 255 > 240);
    assert!(white & 255 > 250);
    let rgba = format.rgba(edge);
    assert_eq!(rgba.0[3], 32);
    assert!(rgba.0[2] > 230);
    assert_eq!(format.on_background(0, 0x123456), 0x123456);
}

#[test]
fn overlapping_translucent_strokes_preserve_premultiplied_channels() {
    let format = PixelFormat::PremultipliedArgb;
    let mut color = 0;
    for (rgb, alpha) in [(0xff0000, 70), (0x00ff00, 110), (0x0000ff, 40)] {
        color = format.over(color, rgb, alpha);
        for shift in [0, 8, 16] { assert!((color >> shift) & 255 <= color >> 24); }
    }
    let shaded = format.shade(color, 0x080b14, 120);
    assert_eq!(shaded >> 24, color >> 24);
    assert_eq!(format.shade(0, 0xffffff, 255), 0);
    assert_eq!(format.rgba(0).0, [0; 4]);
}

#[test]
fn keyed_video_pixels_keep_their_existing_conversion() {
    let format = PixelFormat::ColorKeyRgb;
    assert_eq!(format.clear(), 0xff00ff);
    assert_eq!(format.over(0x123456, 0xffffff, 0), 0x123456);
    assert_eq!(format.over(0x123456, 0xffffff, 255), 0xffffff);
    assert_eq!(format.rgba(0x123456).0, [0x12, 0x34, 0x56, 255]);
    assert_eq!(format.on_background(0xff00ff, 0x202020), 0x202020);
}
