use super::appearance::AppearancePreference;
use gpui_kit::component::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, Window};
use std::ffi::c_void;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Graphics::Gdi::{GetSysColor, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::Accessibility::{HIGHCONTRASTW, HCF_HIGHCONTRASTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetectedTheme {
    Light,
    Dark,
    HighContrast,
}

impl DetectedTheme {
    fn mode(self) -> ThemeMode {
        match self {
            Self::Dark => ThemeMode::Dark,
            Self::Light | Self::HighContrast => ThemeMode::Light,
        }
    }

    fn is_high_contrast(self) -> bool {
        matches!(self, Self::HighContrast)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SystemColors {
    window: u32,
    window_text: u32,
    highlight: u32,
    highlight_text: u32,
}

impl SystemColors {
    fn capture() -> Self {
        unsafe {
            Self {
                window: GetSysColor(COLOR_WINDOW),
                window_text: GetSysColor(COLOR_WINDOWTEXT),
                highlight: GetSysColor(COLOR_HIGHLIGHT),
                highlight_text: GetSysColor(COLOR_HIGHLIGHTTEXT),
            }
        }
    }

    fn theme_colors(self, mut colors: ThemeColor) -> ThemeColor {
        let background = colorref_to_hsla(self.window);
        let foreground = colorref_to_hsla(self.window_text);
        let highlight = colorref_to_hsla(self.highlight);
        let highlight_foreground = colorref_to_hsla(self.highlight_text);

        macro_rules! set {
            ($value:expr; $($field:ident),+ $(,)?) => {
                $(colors.$field = $value;)+
            };
        }

        set!(background; accordion, background, button, button_hover, button_secondary, button_secondary_active, button_secondary_hover, description_list_label, group_box, list, list_even, list_head, list_hover, muted, popover, secondary, secondary_active, secondary_hover, sidebar, skeleton, slider_bar, status_bar, switch, tab, tab_bar, tab_bar_segmented, table, table_even, table_head, table_foot, table_hover, title_bar, overlay, scrollbar);
        set!(foreground; border, button_foreground, button_secondary_foreground, group_box_foreground, caret, chart_1, chart_2, chart_3, chart_4, chart_5, chart_bullish, chart_bearish, chart_grid, description_list_label_foreground, foreground, input, list_active_border, muted_foreground, popover_foreground, secondary_foreground, sidebar_border, sidebar_foreground, table_active_border, table_head_foreground, table_foot_foreground, table_row_border, tab_foreground, title_bar_border, status_bar_border, window_border, drag_border, red, red_light, green, green_light, blue, blue_light, yellow, yellow_light, magenta, magenta_light, cyan, cyan_light);
        set!(highlight; accent, button_active, button_danger, button_danger_active, button_danger_hover, button_info, button_info_active, button_info_hover, button_primary, button_primary_active, button_primary_hover, button_success, button_success_active, button_success_hover, button_warning, button_warning_active, button_warning_hover, danger, danger_active, danger_hover, drop_target, info, info_active, info_hover, link, link_active, link_hover, list_active, primary, primary_active, primary_hover, progress_bar, ring, scrollbar_thumb, scrollbar_thumb_hover, selection, sidebar_accent, sidebar_primary, slider_thumb, switch_thumb, success, success_active, success_hover, tab_active, table_active, warning, warning_active, warning_hover);
        set!(highlight_foreground; accent_foreground, button_danger_foreground, button_info_foreground, button_primary_foreground, button_success_foreground, button_warning_foreground, danger_foreground, info_foreground, primary_foreground, sidebar_accent_foreground, sidebar_primary_foreground, tab_active_foreground, success_foreground, warning_foreground);

        colors
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ThemePreferences {
    detected: DetectedTheme,
    system_colors: Option<SystemColors>,
}

impl ThemePreferences {
    pub(super) fn capture(preference: AppearancePreference) -> Self {
        let apps_use_light_theme = read_apps_use_light_theme();
        let high_contrast = high_contrast_enabled();
        let system = theme_from_preferences(apps_use_light_theme, high_contrast);
        let detected = effective_theme(preference, system);
        Self {
            detected,
            system_colors: detected.is_high_contrast().then(SystemColors::capture),
        }
    }

    pub(super) fn apply(&self, window: Option<&mut Window>, cx: &mut App) {
        Theme::change(self.detected.mode(), window, cx);
        if let Some(system_colors) = self.system_colors {
            let colors = system_colors.theme_colors(Theme::global(cx).colors);
            Theme::update(cx, |theme| {
                theme.colors = colors;
                theme.shadow = false;
            });
        } else {
            Theme::update(cx, |theme| theme.shadow = true);
        }
    }
}

pub(super) struct ThemeMonitor {
    preference: AppearancePreference,
    preferences: ThemePreferences,
}

impl ThemeMonitor {
    pub(super) fn preference(&self) -> AppearancePreference {
        self.preference
    }

    pub(super) fn set_preference(&mut self, preference: AppearancePreference, window: Option<&mut Window>, cx: &mut App) -> bool {
        self.preference = preference;
        let preferences = ThemePreferences::capture(preference);
        if preferences == self.preferences {
            return false;
        }
        preferences.apply(window, cx);
        self.preferences = preferences;
        true
    }

    pub(super) fn refresh(&mut self, window: Option<&mut Window>, cx: &mut App) -> bool {
        let preferences = ThemePreferences::capture(self.preference);
        if preferences == self.preferences {
            return false;
        }
        preferences.apply(window, cx);
        self.preferences = preferences;
        true
    }
}

pub(super) fn sync(window: Option<&mut Window>, cx: &mut App, preference: AppearancePreference) -> ThemeMonitor {
    let monitor = ThemeMonitor { preference, preferences: ThemePreferences::capture(preference) };
    monitor.preferences.apply(window, cx);
    monitor
}

fn theme_from_preferences(apps_use_light_theme: Option<u32>, high_contrast: bool) -> DetectedTheme {
    if high_contrast {
        DetectedTheme::HighContrast
    } else if apps_use_light_theme == Some(0) {
        DetectedTheme::Dark
    } else {
        DetectedTheme::Light
    }
}

fn effective_theme(preference: AppearancePreference, system: DetectedTheme) -> DetectedTheme {
    if system.is_high_contrast() {
        return DetectedTheme::HighContrast;
    }
    match preference {
        AppearancePreference::System => system,
        AppearancePreference::Light => DetectedTheme::Light,
        AppearancePreference::Dark => DetectedTheme::Dark,
    }
}

fn colorref_to_rgb(colorref: u32) -> u32 {
    ((colorref & 0xff) << 16) | (colorref & 0xff00) | ((colorref >> 16) & 0xff)
}

fn colorref_to_hsla(colorref: u32) -> Hsla {
    gpui_kit::rgb(colorref_to_rgb(colorref)).into()
}

fn read_apps_use_light_theme() -> Option<u32> {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("AppsUseLightTheme");
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast::<c_void>(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS && size == std::mem::size_of::<u32>() as u32).then_some(data)
}

fn high_contrast_enabled() -> bool {
    let mut settings = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            settings.cbSize,
            (&mut settings as *mut HIGHCONTRASTW).cast::<c_void>(),
            0,
        ) != 0
            && settings.dwFlags & HCF_HIGHCONTRASTON != 0
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
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
}
