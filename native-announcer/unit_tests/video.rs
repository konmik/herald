#[cfg(not(target_os = "windows"))]
#[test]
fn ffmpeg_is_found_outside_the_minimal_path_of_a_finder_launch() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("herald-ffmpeg-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    // A name no system installs, so an ffmpeg in /usr/bin (common on Linux) cannot satisfy the lookup.
    let name = "herald-test-ffmpeg";
    let program = directory.join(name);
    std::fs::write(&program, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Finder, the Dock and Launchpad start apps with this PATH, which misses Homebrew.
    let finder = std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
    assert_eq!(super::find_program(name, Some(finder), &[directory.as_path()]), Some(program.clone()));
    assert_eq!(super::find_program(name, None, &[directory.as_path()]), Some(program.clone()));
    let on_path = std::env::join_paths([directory.as_path()]).unwrap();
    assert_eq!(super::find_program(name, Some(&on_path), &[]), Some(program));
    assert_eq!(super::find_program(name, Some(finder), &[]), None);
    std::fs::remove_dir_all(directory).unwrap();
}

use super::*;

#[test]
fn decoder_failure_preserves_the_last_frame_and_stops_retrying() {
    struct FailingSource(bool);
    impl FrameSource for FailingSource {
        fn read(&mut self, frame: &mut RgbaImage) -> Result<bool, String> {
            if self.0 { return Err("Damaged video frame".into()); }
            self.0 = true;
            frame.put_pixel(0, 0, image::Rgba([1, 2, 3, 255]));
            Ok(false)
        }
    }
    let mut video = Video { decoder: Some(Box::new(FailingSource(false))), fps: 16.0, frame: RgbaImage::new(1, 1), tick: None, decoded_frames: 0, loops: 0 };
    video.advance(Duration::ZERO).unwrap();
    assert!(video.advance(Duration::from_millis(100)).is_err());
    assert!(video.decoder.is_none());
    video.advance(Duration::from_millis(200)).unwrap();
    assert_eq!(video.frame().get_pixel(0, 0).0, [1, 2, 3, 255]);
    assert_eq!(video.decoded_frames, 1);
}

#[test]
fn uses_the_integration_video_without_a_shared_library() {
    let assets = Path::new("missing-announcer-test-assets");
    assert_eq!(select_path(assets, "claude"), assets.join("claude/neutral.mp4"));
    assert_eq!(select_path(assets, "pi"), assets.join("opencode/neutral.mp4"));
}

#[test]
fn selects_only_videos_from_the_shared_library() {
    let directory = std::env::temp_dir().join(format!("herald-video-selection-{}-{}", std::process::id(), crate::state::timestamp()));
    let library = directory.join("videos");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(library.join("first.mp4"), []).unwrap();
    std::fs::write(library.join("second.MP4"), []).unwrap();
    std::fs::write(library.join("portrait.png"), []).unwrap();
    std::fs::create_dir(library.join("directory.mp4")).unwrap();
    for _ in 0..100 {
        let selected = select_path(&directory, "opencode");
        assert!(selected == library.join("first.mp4") || selected == library.join("second.MP4"));
    }
    std::fs::remove_dir_all(directory).unwrap();
}
