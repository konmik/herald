use super::appearance::AppearancePreference;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Window, WindowAppearance};

pub(super) struct ThemeMonitor {
    appearance: WindowAppearance,
    preference: AppearancePreference,
    effective: ThemeMode,
}

impl ThemeMonitor {
    pub(super) fn preference(&self) -> AppearancePreference {
        self.preference
    }

    pub(super) fn set_preference(&mut self, preference: AppearancePreference, window: Option<&mut Window>, cx: &mut App) -> bool {
        self.preference = preference;
        let effective = effective_mode(preference, self.appearance);
        if effective == self.effective {
            return false;
        }
        self.effective = effective;
        apply(effective, window, cx);
        true
    }

    pub(super) fn refresh(&mut self, window: Option<&mut Window>, cx: &mut App) -> bool {
        let appearance = window.as_ref().map_or_else(|| cx.window_appearance(), |window| window.appearance());
        let effective = effective_mode(self.preference, appearance);
        if appearance == self.appearance && effective == self.effective { return false; }
        self.appearance = appearance;
        if effective == self.effective { return false; }
        self.effective = effective;
        apply(effective, window, cx);
        true
    }
}

fn effective_mode(preference: AppearancePreference, appearance: WindowAppearance) -> ThemeMode {
    match preference {
        AppearancePreference::Light => ThemeMode::Light,
        AppearancePreference::Dark => ThemeMode::Dark,
        AppearancePreference::System => match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
        },
    }
}

fn apply(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    Theme::change(mode, window, cx);
}

pub(super) fn sync(window: Option<&mut Window>, cx: &mut App, preference: AppearancePreference) -> ThemeMonitor {
    let appearance = window.as_ref().map_or_else(|| cx.window_appearance(), |window| window.appearance());
    let effective = effective_mode(preference, appearance);
    apply(effective, window, cx);
    ThemeMonitor { appearance, preference, effective }
}

#[cfg(test)]
#[path = "../../unit_tests/settings_app/theme_linux.rs"]
mod tests;
