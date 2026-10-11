use super::*;

#[test]
fn theme_preference_defaults_to_light_and_detects_dark() {
    assert_eq!(theme_from_preferences(Some(1), false), DetectedTheme::Light);
    assert_eq!(theme_from_preferences(Some(0), false), DetectedTheme::Dark);
    assert_eq!(theme_from_preferences(None, false), DetectedTheme::Light);
    assert_eq!(theme_from_preferences(Some(2), false), DetectedTheme::Light);
    assert_eq!(theme_from_preferences(Some(u32::MAX), false), DetectedTheme::Light);
}

#[test]
fn high_contrast_takes_precedence_over_the_app_theme_preference() {
    assert_eq!(theme_from_preferences(Some(0), true), DetectedTheme::HighContrast);
    assert_eq!(theme_from_preferences(Some(1), true), DetectedTheme::HighContrast);
    assert_eq!(theme_from_preferences(None, true), DetectedTheme::HighContrast);
    assert_eq!(DetectedTheme::HighContrast.mode(), ThemeMode::Light);
}

#[test]
fn explicit_theme_preferences_ignore_light_and_dark_system_changes() {
    assert_eq!(effective_theme(AppearancePreference::Light, DetectedTheme::Light), DetectedTheme::Light);
    assert_eq!(effective_theme(AppearancePreference::Light, DetectedTheme::Dark), DetectedTheme::Light);
    assert_eq!(effective_theme(AppearancePreference::Dark, DetectedTheme::Light), DetectedTheme::Dark);
    assert_eq!(effective_theme(AppearancePreference::Dark, DetectedTheme::Dark), DetectedTheme::Dark);
    assert_eq!(effective_theme(AppearancePreference::System, DetectedTheme::Light), DetectedTheme::Light);
    assert_eq!(effective_theme(AppearancePreference::System, DetectedTheme::Dark), DetectedTheme::Dark);
}

#[test]
fn high_contrast_overrides_explicit_theme_preferences() {
    assert_eq!(effective_theme(AppearancePreference::Light, DetectedTheme::HighContrast), DetectedTheme::HighContrast);
    assert_eq!(effective_theme(AppearancePreference::Dark, DetectedTheme::HighContrast), DetectedTheme::HighContrast);
    assert_eq!(effective_theme(AppearancePreference::System, DetectedTheme::HighContrast), DetectedTheme::HighContrast);
}

#[test]
fn colorref_channels_are_converted_from_windows_bgr() {
    assert_eq!(colorref_to_rgb(0x00332211), 0x112233);
    assert_eq!(colorref_to_rgb(0x00ff8001), 0x0180ff);
}

#[test]
fn high_contrast_palette_uses_system_color_roles() {
    let colors = SystemColors {
        window: 0x00332211,
        window_text: 0x00665544,
        highlight: 0x00998877,
        highlight_text: 0x00ccbbaa,
    }
    .theme_colors(*ThemeColor::light());
    assert_eq!(colors.background, colorref_to_hsla(0x00332211));
    assert_eq!(colors.input, colorref_to_hsla(0x00665544));
    assert_eq!(colors.foreground, colorref_to_hsla(0x00665544));
    assert_eq!(colors.selection, colorref_to_hsla(0x00998877));
    assert_eq!(colors.accent_foreground, colorref_to_hsla(0x00ccbbaa));
}
