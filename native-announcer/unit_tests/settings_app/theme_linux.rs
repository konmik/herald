use super::*;

#[test]
fn explicit_theme_preferences_ignore_system_appearance() {
    assert_eq!(effective_mode(AppearancePreference::Light, WindowAppearance::Dark), ThemeMode::Light);
    assert_eq!(effective_mode(AppearancePreference::Dark, WindowAppearance::Light), ThemeMode::Dark);
}

#[test]
fn system_preference_follows_light_and_dark_appearance() {
    assert_eq!(effective_mode(AppearancePreference::System, WindowAppearance::Light), ThemeMode::Light);
    assert_eq!(effective_mode(AppearancePreference::System, WindowAppearance::VibrantLight), ThemeMode::Light);
    assert_eq!(effective_mode(AppearancePreference::System, WindowAppearance::Dark), ThemeMode::Dark);
    assert_eq!(effective_mode(AppearancePreference::System, WindowAppearance::VibrantDark), ThemeMode::Dark);
}
