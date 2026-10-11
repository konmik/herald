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
#[path = "../../unit_tests/settings_app/appearance.rs"]
mod tests;
