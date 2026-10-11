use super::*;

#[test]
fn desktop_background_is_transparent_and_content_is_opaque() {
    assert_eq!(argb(0xff00ff), 0x00000000);
    assert_eq!(argb(0x1c1b16), 0xff1c1b16);
    assert_eq!(argb(0xeef2f7), 0xffeef2f7);
}
