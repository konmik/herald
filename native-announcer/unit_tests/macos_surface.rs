use super::*;

#[test]
fn desktop_background_is_transparent_and_content_is_opaque() {
    let pixels = layer_pixels(&[0xff00ff, 0x1c1b16, 0xeef2f7], crate::pixels::PixelFormat::ColorKeyRgb);
    assert_eq!(&*pixels, &[0, 0xff1c1b16, 0xffeef2f7]);
}

#[test]
fn chimera_layer_preserves_transparent_and_translucent_pixels() {
    let pixels = [0, 0x80402010, 0xff1c1b16];
    assert_eq!(&*layer_pixels(&pixels, crate::pixels::PixelFormat::PremultipliedArgb), &pixels);
}
