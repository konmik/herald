use super::*;

fn data_directory() -> std::path::PathBuf {
    // Parallel tests can read the same clock value, so a counter keeps their directories apart.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let data = std::env::temp_dir().join(format!("herald-appearance-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    data
}

#[test]
fn preference_serializes_as_lowercase() {
    assert_eq!(serde_json::to_string(&Appearance { theme: AppearancePreference::Dark }).unwrap(), r#"{"theme":"dark"}"#);
    assert_eq!(serde_json::to_string(&Appearance { theme: AppearancePreference::Light }).unwrap(), r#"{"theme":"light"}"#);
    assert_eq!(serde_json::to_string(&Appearance { theme: AppearancePreference::System }).unwrap(), r#"{"theme":"system"}"#);
}

#[test]
fn missing_appearance_uses_system_without_warning() {
    let data = data_directory();
    let (appearance, warning) = Appearance::load_or_default(&data);
    assert_eq!(appearance.theme, AppearancePreference::System);
    assert!(warning.is_none());
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn malformed_appearance_uses_system_with_warning() {
    let data = data_directory();
    std::fs::write(data.join("appearance.json"), br#"{"theme":"blue"}"#).unwrap();
    let (appearance, warning) = Appearance::load_or_default(&data);
    assert_eq!(appearance.theme, AppearancePreference::System);
    assert!(warning.is_some());
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn appearance_save_load_is_atomic_and_does_not_change_settings() {
    let data = data_directory();
    let settings = br#"{"volume":37,"summaryPrompt":"keep this exact file"}"#;
    std::fs::write(data.join("settings.json"), settings).unwrap();
    Appearance { theme: AppearancePreference::Dark }.save(&data).unwrap();
    Appearance { theme: AppearancePreference::Light }.save(&data).unwrap();
    assert_eq!(std::fs::read(data.join("settings.json")).unwrap(), settings);
    assert_eq!(Appearance::load(&data).unwrap().theme, AppearancePreference::Light);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(data.join("appearance.json")).unwrap()).unwrap(), serde_json::json!({"theme": "light"}));
    let _ = std::fs::remove_dir_all(data);
}
