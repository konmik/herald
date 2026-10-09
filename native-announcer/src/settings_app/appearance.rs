use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum AppearancePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Appearance {
    pub(super) theme: AppearancePreference,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: AppearancePreference::System }
    }
}

impl Appearance {
    pub(super) fn load(data: &Path) -> Result<Self, String> {
        match std::fs::read(data.join("appearance.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub(super) fn load_or_default(data: &Path) -> (Self, Option<String>) {
        match Self::load(data) {
            Ok(appearance) => (appearance, None),
            Err(error) => (
                Self::default(),
                Some(format!("Could not load the saved theme: {error} Using the System theme.")),
            ),
        }
    }

    pub(super) fn save(&self, data: &Path) -> Result<(), String> {
        crate::private::directory(data).map_err(|error| error.to_string())?;
        let temporary = data.join(format!("appearance-{}-{}.tmp", std::process::id(), crate::state::timestamp()));
        let result = (|| {
            let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
            crate::private::write(&temporary, &bytes).map_err(|error| error.to_string())?;
            std::fs::rename(&temporary, data.join("appearance.json")).map_err(|error| error.to_string())
        })();
        if result.is_err() { let _ = std::fs::remove_file(temporary); }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_directory() -> std::path::PathBuf {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let data = std::env::temp_dir().join(format!("herald-appearance-{}-{unique}", std::process::id()));
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
}
