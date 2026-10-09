use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Window, WindowAppearance};

pub(super) struct ThemeMonitor {
    appearance: WindowAppearance,
}

impl ThemeMonitor {
    pub(super) fn refresh(&mut self, window: Option<&mut Window>, cx: &mut App) -> bool {
        let appearance = window.as_ref().map_or_else(|| cx.window_appearance(), |window| window.appearance());
        if appearance == self.appearance { return false; }
        self.appearance = appearance;
        apply(appearance, window, cx);
        true
    }
}

fn apply(appearance: WindowAppearance, window: Option<&mut Window>, cx: &mut App) {
    let mode = match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
    };
    Theme::change(mode, window, cx);
}

pub(super) fn sync(window: Option<&mut Window>, cx: &mut App) -> ThemeMonitor {
    let appearance = window.as_ref().map_or_else(|| cx.window_appearance(), |window| window.appearance());
    apply(appearance, window, cx);
    ThemeMonitor { appearance }
}
