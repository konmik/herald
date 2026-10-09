#[path = "draft.rs"]
mod draft;
#[path = "appearance.rs"]
mod appearance;
#[path = "keyboard.rs"]
mod keyboard;
use keyboard::RevealFocused;
#[cfg_attr(target_os = "windows", path = "instance.rs")]
#[cfg_attr(target_os = "linux", path = "instance_linux.rs")]
mod instance;
#[cfg_attr(target_os = "windows", path = "theme.rs")]
#[cfg_attr(target_os = "linux", path = "theme_linux.rs")]
mod theme;
#[cfg(target_os = "windows")]
#[path = "dialogs_windows.rs"]
mod dialogs;

use crate::characters::{Character, CharacterVoice};
use crate::elevenlabs::{Client, SpeechModel, VoiceUsage};
use crate::platform::Preview;
use crate::settings::{format_time, FontPreference, Settings, DEFAULT_SUMMARY_PROMPT, MAX_FONT_SIZE, MIN_FONT_SIZE};
use appearance::{Appearance, AppearancePreference};
use chrono::Timelike;
use draft::{capture_draft_video, capture_draft_voice, character_video, new_character_id, voice_id_text, SettingsDraft};
#[cfg(target_os = "windows")]
use draft::video_picker_path;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputContentType, InputEvent, InputState, TextareaState};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, FocusableExt, IndexPath, Root, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, size, AnyElement, App, Context, Entity, FontWeight, IntoElement, ParentElement,
    FocusHandle, Focusable, Render, SharedString, Subscription, Window, WindowBounds, WindowOptions, ScrollHandle,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Characters,
    Audio,
    QuietHours,
    SpeechService,
    OfflineVoice,
    Announcements,
}

#[derive(Clone, Copy)]
struct Layout {
    compact: bool,
    character_list_height: f32,
}

impl Layout {
    fn for_window(window: &Window) -> Self {
        let viewport = window.viewport_size();
        let compact = viewport.width < px(900.);
        Self {
            compact,
            character_list_height: if compact { (f32::from(viewport.height) * 0.22).clamp(72., 160.) } else { 360. },
        }
    }
}

struct PageSpec {
    page: Page,
    key: &'static str,
    label: &'static str,
    description: &'static str,
}

const PAGES: [PageSpec; 6] = [
    PageSpec {
        page: Page::Characters,
        key: "characters",
        label: "Characters",
        description: "Choose the characters used for announcements.",
    },
    PageSpec {
        page: Page::Audio,
        key: "audio",
        label: "Audio",
        description: "Set announcement volume, output device, and preview options.",
    },
    PageSpec {
        page: Page::QuietHours,
        key: "quiet-hours",
        label: "Quiet hours",
        description: "Mute speech or schedule quiet hours in local time.",
    },
    PageSpec {
        page: Page::SpeechService,
        key: "speech-service",
        label: "Speech service",
        description: "Choose a speech model and configure the speech service.",
    },
    PageSpec {
        page: Page::OfflineVoice,
        key: "offline-voice",
        label: "Offline voice",
        description: "Use a local voice when the online speech service is unavailable.",
    },
    PageSpec {
        page: Page::Announcements,
        key: "announcements",
        label: "Announcements",
        description: "Choose announcement fonts, sizes, and summary options.",
    },
];

impl Page {
    fn spec(self) -> &'static PageSpec {
        PAGES.iter().find(|spec| spec.page == self).unwrap()
    }
}

#[derive(Clone)]
struct Choice {
    value: String,
    label: String,
}

impl Choice {
    fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into() }
    }
}

impl SearchableListItem for Choice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone().into()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().id(format!("choice-{}", self.value)).role(gpui_kit::Role::Group).aria_label(self.label.clone()).child(self.label.clone())
    }
}

enum VoiceUsageState {
    NoKey,
    NotLoaded,
    Loading { key: String },
    Failed(String),
    Ready(VoiceUsage),
}

type ChoiceSelect = SelectState<Vec<Choice>>;

struct SettingsView {
    initial_focus_pending: bool,
    navigation_focus: [FocusHandle; 6],
    character_list_focus: FocusHandle,
    volume_focus: FocusHandle,
    silent_sound_focus: FocusHandle,
    page_scroll: ScrollHandle,
    navigation_scroll: ScrollHandle,
    character_scroll: ScrollHandle,
    character_anchor: gpui_kit::ScrollAnchor,
    data: PathBuf,
    assets: PathBuf,
    saved: Settings,
    draft: SettingsDraft,
    active_page: Page,
    active_character: Option<String>,
    status: String,
    usage_input: Entity<TextareaState>,
    quiet_start_input: Entity<InputState>,
    quiet_end_input: Entity<InputState>,
    default_voice_input: Entity<InputState>,
    api_key_input: Entity<InputState>,
    character_name_input: Entity<InputState>,
    character_video_input: Entity<InputState>,
    character_voice_input: Entity<InputState>,
    character_prompt_input: Entity<TextareaState>,
    summary_prompt_input: Entity<TextareaState>,
    volume_slider: Entity<SliderState>,
    silent_sound_slider: Entity<SliderState>,
    output_select: Entity<ChoiceSelect>,
    model_select: Entity<ChoiceSelect>,
    body_font_select: Entity<ChoiceSelect>,
    body_size_select: Entity<ChoiceSelect>,
    title_font_select: Entity<ChoiceSelect>,
    title_size_select: Entity<ChoiceSelect>,
    devices: Vec<crate::audio::OutputDevice>,
    voice_usage: VoiceUsageState,
    usage_generation: u64,
    offline_installing: bool,
    #[cfg(target_os = "windows")]
    offline_error: Option<String>,
    preview: Option<Preview>,
    voice_preview: Option<Preview>,
    subscriptions: Vec<Subscription>,
    theme_monitor: theme::ThemeMonitor,
    instance: Rc<instance::InstanceGuard>,
}

impl SettingsView {
    fn new(data: PathBuf, assets: PathBuf, settings: Settings, instance: Rc<instance::InstanceGuard>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, app| {
            weak.update(app, |view, cx| view.should_close(window, cx)).unwrap_or(true)
        });

        let draft = SettingsDraft::new(settings.clone());
        let (appearance, appearance_warning) = Appearance::load_or_default(&data);
        let active_character = draft
            .selected_character
            .clone()
            .filter(|id| draft.characters.contains_key(id));
        let active = active_character
            .as_ref()
            .and_then(|id| draft.characters.get(id))
            .cloned()
            .unwrap_or_default();
        let devices = crate::audio::output_devices();
        let font_families = crate::fonts::available_families().unwrap_or_default();
        let output_choices = output_choices(&devices, draft.settings.output_device.as_deref());
        let model_choices = SpeechModel::ALL.iter().map(|model| Choice::new(model.id(), model.label())).collect::<Vec<_>>();
        let body_font_choices = font_choices(&font_families, &draft.settings.announcement_body_font.family);
        let title_font_choices = font_choices(&font_families, &draft.settings.announcement_title_font.family);
        let size_choices = font_size_choices();
        let status = appearance_warning.unwrap_or_else(|| "Apply saves changes. Close discards unsaved edits.".to_owned());
        let character_scroll = ScrollHandle::new();
        let character_anchor = gpui_kit::ScrollAnchor::for_handle(character_scroll.clone());
        let mut view = Self {
            initial_focus_pending: true,
            navigation_focus: std::array::from_fn(|_| cx.focus_handle()),
            character_list_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            volume_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            silent_sound_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            page_scroll: ScrollHandle::new(),
            navigation_scroll: ScrollHandle::new(),
            character_scroll,
            character_anchor,
            instance,
            theme_monitor: theme::sync(Some(window), cx, appearance.theme),
            data: data.clone(),
            assets,
            saved: settings.clone(),
            draft,
            active_page: Page::Audio,
            active_character,
            status: status.clone(),
            usage_input: cx.new(|cx| TextareaState::new(window, cx).default_value("Enter an ElevenLabs key to load voice usage.")),
            quiet_start_input: cx.new(|cx| InputState::new(window, cx).default_value(format_time(settings.quiet_start))),
            quiet_end_input: cx.new(|cx| InputState::new(window, cx).default_value(format_time(settings.quiet_end))),
            default_voice_input: cx.new(|cx| InputState::new(window, cx).default_value(settings.default_voice_id.clone())),
            api_key_input: cx.new(|cx| InputState::new(window, cx).default_value(settings.elevenlabs_api_key.clone().unwrap_or_default()).masked(true)),
            character_name_input: cx.new(|cx| InputState::new(window, cx).default_value(active.name.clone())),
            character_video_input: cx.new(|cx| InputState::new(window, cx).default_value("Choose video…")),
            character_voice_input: cx.new(|cx| InputState::new(window, cx).default_value(voice_id_text(&active.voice))),
            character_prompt_input: cx.new(|cx| TextareaState::new(window, cx).default_value(active.summary_prompt.clone())),
            summary_prompt_input: cx.new(|cx| TextareaState::new(window, cx).default_value(settings.summary_prompt.clone())),
            volume_slider: cx.new(|_| SliderState::new().min(0.).max(100.).step(1.).default_value(settings.volume as f32)),
            silent_sound_slider: cx.new(|_| SliderState::new().min(0.).max(10.).step(1.).default_value(settings.silent_sound_seconds as f32)),
            output_select: cx.new(|cx| {
                SelectState::new(
                    output_choices.clone(),
                    Some(IndexPath::new(choice_index(&output_choices, output_value(settings.output_device.as_deref())))),
                    window,
                    cx,
                )
            }),
            model_select: cx.new(|cx| {
                SelectState::new(
                    model_choices.clone(),
                    Some(IndexPath::new(choice_index(&model_choices, settings.speech_model.id()))),
                    window,
                    cx,
                )
            }),
            body_font_select: cx.new(|cx| {
                SelectState::new(
                    body_font_choices.clone(),
                    Some(IndexPath::new(choice_index(&body_font_choices, &settings.announcement_body_font.family))),
                    window,
                    cx,
                )
            }),
            body_size_select: cx.new(|cx| {
                SelectState::new(
                    size_choices.clone(),
                    Some(IndexPath::new(choice_index(&size_choices, &settings.announcement_body_font.size.to_string()))),
                    window,
                    cx,
                )
            }),
            title_font_select: cx.new(|cx| {
                SelectState::new(
                    title_font_choices.clone(),
                    Some(IndexPath::new(choice_index(&title_font_choices, &settings.announcement_title_font.family))),
                    window,
                    cx,
                )
            }),
            title_size_select: cx.new(|cx| {
                SelectState::new(
                    size_choices.clone(),
                    Some(IndexPath::new(choice_index(&size_choices, &settings.announcement_title_font.size.to_string()))),
                    window,
                    cx,
                )
            }),
            devices,
            voice_usage: initial_voice_usage_state(settings.elevenlabs_api_key.as_deref()),
            usage_generation: 0,
            offline_installing: false,
            #[cfg(target_os = "windows")]
            offline_error: None,
            preview: None,
            voice_preview: None,
            subscriptions: Vec::new(),
        };
        view.sync_character_inputs(window, cx);
        view.install_subscriptions(cx);
        let handle = window.window_handle();
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                if handle.update(cx, |_, window, app| {
                    weak.update(app, |view, cx| {
                        view.instance.activate_pending(window);
                        if view.theme_monitor.refresh(Some(window), cx) { cx.notify(); }
                    })
                }).is_err() { break; }
            }
        }).detach();
        view
    }

    fn install_subscriptions(&mut self, cx: &mut Context<Self>) {
        for state in [&self.character_prompt_input, &self.summary_prompt_input, &self.usage_input] {
            self.subscriptions.push(cx.observe(state, |_, _, cx| cx.notify()));
        }
        let state = self.quiet_start_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.draft.quiet_start_text = state.read(cx).value().to_string();
                cx.notify();
            }
        }));
        let state = self.quiet_end_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.draft.quiet_end_text = state.read(cx).value().to_string();
                cx.notify();
            }
        }));
        let state = self.default_voice_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.draft.settings.default_voice_id = state.read(cx).value().to_string();
                cx.notify();
            }
        }));
        let state = self.api_key_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let key = state.read(cx).value().trim().to_owned();
                view.draft.settings.elevenlabs_api_key = (!key.is_empty()).then_some(key);
                view.usage_generation += 1;
                view.voice_usage = initial_voice_usage_state(view.draft.settings.elevenlabs_api_key.as_deref());
                cx.notify();
            }
        }));
        let state = self.character_name_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(id) = view.active_character.clone() {
                    view.draft.character(&id).name = state.read(cx).value().to_string();
                    cx.notify();
                }
            }
        }));
        let state = self.character_video_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(id) = view.active_character.clone() {
                    let video = state.read(cx).value().to_string();
                    let assets = view.assets.clone();
                    capture_draft_video(&id, view.draft.character(&id), &video, &assets);
                    cx.notify();
                }
            }
        }));
        let state = self.character_voice_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(id) = view.active_character.clone() {
                    let value = state.read(cx).value().to_string();
                    capture_draft_voice(view.draft.character(&id), &value);
                    cx.notify();
                }
            }
        }));
        let state = self.character_prompt_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(id) = view.active_character.clone() {
                    view.draft.character(&id).summary_prompt = state.read(cx).value().to_string();
                    cx.notify();
                }
            }
        }));
        let state = self.summary_prompt_input.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.draft.settings.summary_prompt = state.read(cx).value().to_string();
                cx.notify();
            }
        }));
        let state = self.volume_slider.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SliderEvent, cx| {
            let value = match event {
                SliderEvent::Change(value) | SliderEvent::Release(value) => value.start(),
            };
            view.draft.settings.volume = value.round().clamp(0., 100.) as u16;
            cx.notify();
        }));
        let state = self.silent_sound_slider.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SliderEvent, cx| {
            let value = match event {
                SliderEvent::Change(value) | SliderEvent::Release(value) => value.start(),
            };
            view.draft.settings.silent_sound_seconds = value.round().clamp(0., 10.) as u16;
            cx.notify();
        }));
        self.subscribe_output_select(cx);
        let state = self.model_select.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SelectEvent<Vec<Choice>>, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                if let Some(model) = SpeechModel::ALL.iter().find(|model| model.id() == value) {
                    view.draft.settings.speech_model = *model;
                    cx.notify();
                }
            }
        }));
        let state = self.body_font_select.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SelectEvent<Vec<Choice>>, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                view.draft.settings.announcement_body_font.family = value.clone();
                cx.notify();
            }
        }));
        let state = self.body_size_select.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SelectEvent<Vec<Choice>>, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                if let Ok(size) = value.parse() {
                    view.draft.settings.announcement_body_font.size = size;
                    cx.notify();
                }
            }
        }));
        let state = self.title_font_select.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SelectEvent<Vec<Choice>>, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                view.draft.settings.announcement_title_font.family = value.clone();
                cx.notify();
            }
        }));
        let state = self.title_size_select.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SelectEvent<Vec<Choice>>, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                if let Ok(size) = value.parse() {
                    view.draft.settings.announcement_title_font.size = size;
                    cx.notify();
                }
            }
        }));
    }

    fn select_theme(&mut self, preference: AppearancePreference, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = (Appearance { theme: preference }).save(&self.data) {
            self.status = format!("Could not save the theme: {error}");
            cx.notify();
            return;
        }
        self.theme_monitor.set_preference(preference, Some(window), cx);
        self.status = "Theme saved immediately. Other settings still need Apply.".into();
        cx.notify();
    }

    fn input_border_color(&self, state: &Entity<InputState>, window: &Window, cx: &mut Context<Self>) -> gpui_kit::Hsla {
        if state.read(cx).focus_handle(cx).is_focused(window) { cx.theme().ring } else { cx.theme().input }
    }

    fn subscribe_output_select(&mut self, cx: &mut Context<Self>) {
        let state = self.output_select.clone();
        self.subscriptions.push(cx.subscribe(&state, |view, _, event: &SelectEvent<Vec<Choice>>, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                view.draft.settings.output_device = (!value.is_empty()).then_some(value.clone());
                cx.notify();
            }
        }));
    }

    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.offline_installing {
            self.status = "Wait for offline voice installation to finish before closing.".into();
            cx.notify();
            return false;
        }
        self.stop_previews();
        let _ = window;
        true
    }

    fn capture_inputs(&mut self, cx: &mut Context<Self>) {
        self.draft.quiet_start_text = self.quiet_start_input.read(cx).value().to_string();
        self.draft.quiet_end_text = self.quiet_end_input.read(cx).value().to_string();
        self.draft.settings.default_voice_id = self.default_voice_input.read(cx).value().trim().to_owned();
        let key = self.api_key_input.read(cx).value().trim().to_owned();
        self.draft.settings.elevenlabs_api_key = (!key.is_empty()).then_some(key);
        self.draft.settings.volume = slider_u16(&self.volume_slider.read(cx).value(), 0, 100);
        self.draft.settings.silent_sound_seconds = slider_u16(&self.silent_sound_slider.read(cx).value(), 0, 10);
        if let Some(id) = self.active_character.clone() {
            let name = self.character_name_input.read(cx).value().to_string();
            let video = self.character_video_input.read(cx).value().to_string();
            let voice = self.character_voice_input.read(cx).value().to_string();
            let prompt = self.character_prompt_input.read(cx).value().to_string();
            let character = self.draft.character(&id);
            character.name = name;
            character.summary_prompt = prompt;
            capture_draft_voice(character, &voice);
            capture_draft_video(&id, character, &video, &self.assets);
        }
    }

    fn sync_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = voice_usage_text(&self.voice_usage);
        if self.usage_input.read(cx).value().as_ref() != text {
            self.usage_input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    fn sync_character_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, video, voice, prompt) = self
            .active_character
            .as_ref()
            .and_then(|id| self.draft.characters.get(id).map(|character| {
                (
                    character.name.clone(),
                    character_video(id, character, &self.assets),
                    voice_id_text(&character.voice).to_owned(),
                    character.summary_prompt.clone(),
                )
            }))
            .unwrap_or_else(|| (String::new(), String::new(), String::new(), String::new()));
        self.character_name_input.update(cx, |state, cx| state.set_value(name, window, cx));
        self.character_video_input.update(cx, |state, cx| state.set_value(video, window, cx));
        self.character_voice_input.update(cx, |state, cx| state.set_value(voice, window, cx));
        self.character_prompt_input.update(cx, |state, cx| state.set_value(prompt, window, cx));
    }

    fn change_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_page == page {
            return;
        }
        self.capture_inputs(cx);
        self.stop_previews();
        self.active_page = page;
        self.page_scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        if page == Page::SpeechService && matches!(self.voice_usage, VoiceUsageState::NotLoaded) {
            self.refresh_usage(cx);
        }
        cx.notify();
        let _ = window;
    }

    fn navigate_page(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.change_page(PAGES[index].page, window, cx);
        self.navigation_focus[index].focus(window, cx);
    }

    fn step_page(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let current = PAGES.iter().position(|spec| spec.page == self.active_page).unwrap();
        self.navigate_page(if forward { (current + 1) % PAGES.len() } else { (current + PAGES.len() - 1) % PAGES.len() }, window, cx);
    }

    fn navigate_character(&mut self, direction: i32, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.draft.characters.keys().filter(|id| !self.draft.removed_characters.contains(*id)).cloned().collect::<Vec<_>>();
        if ids.is_empty() { return; }
        let current = ids.iter().position(|id| Some(id) == self.active_character.as_ref());
        let index = match direction {
            i32::MIN => 0,
            i32::MAX => ids.len() - 1,
            -1 => current.map(|index| index.saturating_sub(1)).unwrap_or(ids.len() - 1),
            _ => current.map(|index| (index + 1).min(ids.len() - 1)).unwrap_or(0),
        };
        self.select_character(ids[index].clone(), window, cx);
        self.character_anchor.scroll_to(window, cx);
    }

    fn choice_boundary(&self, last: bool, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::Focusable;
        for state in [&self.output_select, &self.model_select, &self.body_font_select, &self.body_size_select, &self.title_font_select, &self.title_size_select] {
            let focus = state.read(cx).focus_handle(cx);
            if focus.contains_focused(window, cx) {
                let row = state.read(cx).selected_index(cx).map(|index| index.row).unwrap_or(0);
                // These selects use a single, unfiltered Vec. List's Up action wraps
                // from its first row to its last and retains the pending selection.
                for _ in 0..row + usize::from(last) {
                    focus.dispatch_action(&gpui_kit::base::actions::SelectUp, window, cx);
                }
                return;
            }
        }
    }

    fn select_character(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_inputs(cx);
        self.stop_previews();
        self.active_character = Some(id.clone());
        self.draft.selected_character = Some(id);
        self.sync_character_inputs(window, cx);
        self.status = "Character draft retained. Apply saves changes; Close discards them.".into();
        cx.notify();
    }

    fn add_character(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_inputs(cx);
        self.stop_previews();
        let id = new_character_id(&self.draft.characters);
        self.draft.characters.insert(id.clone(), Character::default());
        self.active_character = Some(id.clone());
        self.draft.selected_character = Some(id);
        self.sync_character_inputs(window, cx);
        self.status = "New character draft. Apply saves it; Close discards it.".into();
        cx.notify();
    }

    fn delete_character(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active_character.clone() else { return; };
        let name = self.draft.characters.get(&id).map(|character| character.name.as_str()).unwrap_or("this character");
        let title = format!("Delete {name}?");
        let view = cx.weak_entity();
        let after_delete_focus = self.character_list_focus.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let id = id.clone();
            let view = view.clone();
            let after_delete_focus = after_delete_focus.clone();
            dialog.title(title.clone()).close_button(false)
                .description("Apply saves the removal. Cloud voices are unchanged.")
                .button_props(gpui_kit::component::dialog::DialogButtonProps::default().show_cancel(true).ok_text("Delete").cancel_text("Cancel"))
                .on_ok(move |_, window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.stop_previews();
                        view.draft.remove_character(&id);
                        if view.active_character.as_ref() == Some(&id) { view.active_character = None; }
                        view.sync_character_inputs(window, cx);
                        view.status = "Character marked for removal. Apply saves it; cloud voices are unchanged.".into();
                        cx.notify();
                    });
                    let focus = after_delete_focus.clone();
                    window.on_next_frame(move |window, cx| focus.focus(window, cx));
                    true
                })
        });
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        self.capture_inputs(cx);
        let candidate = match self.draft.candidate() {
            Ok(settings) => settings,
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
        };
        if let Err(error) = candidate.save(&self.data) {
            self.status = error;
            cx.notify();
            return;
        }
        self.saved = candidate.clone();
        self.draft.commit(candidate);
        self.status = "Saved. Changes apply to the next announcement.".into();
        cx.notify();
    }

    fn preview_settings(&mut self, cx: &mut Context<Self>) -> Result<Settings, String> {
        self.capture_inputs(cx);
        self.draft.candidate()
    }

    fn start_audio_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview.take().is_some() {
            self.status = "Preview stopped.".into();
            cx.notify();
            return;
        }
        self.voice_preview = None;
        let settings = match self.preview_settings(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
        };
        if settings.volume == 0 {
            self.status = "Preview is silent at 0% volume.".into();
            cx.notify();
            return;
        }
        let now = chrono::Local::now();
        if settings.quiet_at(now.hour() * 60 + now.minute()) {
            self.status = "Preview is silent during quiet hours.".into();
            cx.notify();
            return;
        }
        self.preview = Some(Preview::start(self.data.clone(), settings, self.assets.clone()));
        self.status = "Playing static, then: This is an announcement".into();
        cx.notify();
        self.poll_previews(cx);
    }

    fn start_voice_preview(&mut self, cx: &mut Context<Self>) {
        if self.voice_preview.take().is_some() {
            self.status = "Voice preview stopped.".into();
            cx.notify();
            return;
        }
        self.preview = None;
        let Some(id) = self.active_character.clone() else { return; };
        let mut settings = match self.preview_settings(cx) {
            Ok(settings) => settings,
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
        };
        if settings.volume == 0 {
            self.status = "Voice preview is silent at 0% volume.".into();
            cx.notify();
            return;
        }
        let now = chrono::Local::now();
        if settings.quiet_at(now.hour() * 60 + now.minute()) {
            self.status = "Preview is silent during quiet hours.".into();
            cx.notify();
            return;
        }
        let Some(character) = self.draft.characters.get(&id).cloned() else { return; };
        if let CharacterVoice::ElevenLabs { voice_id } = &character.voice {
            if let Err(error) = crate::characters::validate_voice_id(&voice_id) {
                self.status = error;
                cx.notify();
                return;
            }
        }
        settings.characters.insert(id.clone(), character);
        settings.selected_character = Some(id);
        self.voice_preview = Some(Preview::voice(settings, "This is a character voice example".into(), self.assets.clone()));
        self.status = "Playing voice example.".into();
        cx.notify();
        self.poll_previews(cx);
    }

    fn stop_previews(&mut self) {
        self.preview = None;
        self.voice_preview = None;
    }

    fn poll_previews(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let done = weak
                    .update(cx, |view, cx| {
                        let mut done = view.preview.is_none() && view.voice_preview.is_none();
                        if let Some(preview) = view.preview.as_mut() {
                            if let Some(result) = preview.finished() {
                                view.preview = None;
                                view.status = match result {
                                    Ok(()) => "Preview finished.".into(),
                                    Err(error) => format!("Preview failed: {error}"),
                                };
                                cx.notify();
                            }
                        }
                        if let Some(preview) = view.voice_preview.as_mut() {
                            if let Some(result) = preview.finished() {
                                view.voice_preview = None;
                                view.status = match result {
                                    Ok(()) => "Voice preview finished.".into(),
                                    Err(error) => format!("Voice preview failed: {error}"),
                                };
                                cx.notify();
                            }
                        }
                        done = done || (view.preview.is_none() && view.voice_preview.is_none());
                        done
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    fn refresh_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_inputs(cx);
        self.devices = crate::audio::output_devices();
        let choices = output_choices(&self.devices, self.draft.settings.output_device.as_deref());
        let selected = output_value(self.draft.settings.output_device.as_deref()).to_owned();
        self.output_select.update(cx, |state, cx| {
            state.set_items(choices, window, cx);
            state.set_selected_value(&selected, window, cx);
        });
        self.status = "Output devices refreshed.".into();
        cx.notify();
    }

    fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        self.capture_inputs(cx);
        self.usage_generation += 1;
        let generation = self.usage_generation;
        let key = self
            .draft
            .settings
            .elevenlabs_api_key
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if key.is_empty() {
            self.voice_usage = VoiceUsageState::NoKey;
            cx.notify();
            return;
        }
        if let Err(error) = crate::elevenlabs::validate_api_key(&key) {
            self.voice_usage = VoiceUsageState::Failed(error);
            cx.notify();
            return;
        }
        let mut settings = self.draft.settings.clone();
        settings.elevenlabs_api_key = Some(key.clone());
        self.voice_usage = VoiceUsageState::Loading { key: key.clone() };
        let weak = cx.weak_entity();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let client = Client::from_settings(&settings)?.ok_or_else(|| "Enter an ElevenLabs key to load voice usage.".to_string())?;
                client.voice_usage()
            })();
            let _ = sender.send(result);
        });
        cx.spawn(async move |_, cx| {
            loop {
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = weak.update(cx, |view, cx| {
                            if view.usage_generation == generation && matches!(&view.voice_usage, VoiceUsageState::Loading { key: current } if current == &key) {
                                view.voice_usage = match result {
                                    Ok(usage) => VoiceUsageState::Ready(usage),
                                    Err(error) => VoiceUsageState::Failed(error),
                                };
                                cx.notify();
                            }
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = weak.update(cx, |view, cx| {
                            if view.usage_generation == generation && matches!(&view.voice_usage, VoiceUsageState::Loading { key: current } if current == &key) {
                                view.voice_usage = VoiceUsageState::Failed("Voice usage request stopped unexpectedly.".into());
                                cx.notify();
                            }
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                    }
                }
            }
        })
        .detach();
        cx.notify();
    }

    #[cfg(target_os = "windows")]
    fn install_offline_voice(&mut self, cx: &mut Context<Self>) {
        if self.offline_installing || crate::tts::installed() {
            return;
        }
        self.offline_installing = true;
        self.offline_error = None;
        self.status = "Installing offline voice…".into();
        let weak = cx.weak_entity();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(crate::tts::install());
        });
        cx.spawn(async move |_, cx| {
            loop {
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = weak.update(cx, |view, cx| {
                            view.offline_installing = false;
                            match result {
                                Ok(()) => {
                                    view.offline_error = None;
                                    view.status = "Offline voice installed. Changes apply immediately.".into();
                                }
                                Err(error) => {
                                    view.offline_error = Some(error.clone());
                                    view.status = format!("Offline voice installation failed: {error}");
                                }
                            }
                            cx.notify();
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = weak.update(cx, |view, cx| {
                            view.offline_installing = false;
                            view.offline_error = Some("Offline voice installation stopped unexpectedly.".into());
                            view.status = "Offline voice installation stopped unexpectedly.".into();
                            cx.notify();
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                    }
                }
            }
        })
        .detach();
        cx.notify();
    }

    #[cfg(target_os = "windows")]
    fn browse_video(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active_character.clone() else { return; };
        let current = self.character_video_input.read(cx).value().to_string();
        let path = video_picker_path(&current, &self.assets);
        let library = video_picker_path("", &self.assets);
        let focus = window.focused(cx);
        let result = dialogs::pick_video(window, &path, &library);
        if let Some(focus) = focus { focus.focus(window, cx); }
        let selected = match result {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => {
                self.status = format!("Could not open the video picker: {error}");
                cx.notify();
                return;
            }
        };
        if !selected.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")) {
            self.status = "Choose an MP4 animation file.".into();
            cx.notify();
            return;
        }
        let existing = self.draft.characters.get(&id).and_then(|character| character.animation_path.clone());
        let stored = existing.filter(|existing| crate::characters::animation_path(&id, existing, &self.assets) == selected).unwrap_or(selected);
        self.character_video_input.update(cx, |state, cx| state.set_value(stored.to_string_lossy().into_owned(), window, cx));
        self.draft.character(&id).animation_path = Some(stored);
        self.status = "Character video selected. Apply saves the path.".into();
        cx.notify();
    }

    #[cfg(target_os = "windows")]
    fn open_my_voices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = dialogs::open_my_voices(window) {
            self.status = error;
            cx.notify();
        }
    }

    fn reset_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.settings.announcement_body_font = FontPreference::new("Century Gothic", 18);
        self.draft.settings.announcement_title_font = FontPreference::new("Century Gothic", 14);
        self.draft.settings.summary_prompt = DEFAULT_SUMMARY_PROMPT.into();
        let body_family = "Century Gothic".to_owned();
        let title_family = "Century Gothic".to_owned();
        let body_size = "18".to_owned();
        let title_size = "14".to_owned();
        self.body_font_select.update(cx, |state, cx| state.set_selected_value(&body_family, window, cx));
        self.title_font_select.update(cx, |state, cx| state.set_selected_value(&title_family, window, cx));
        self.body_size_select.update(cx, |state, cx| state.set_selected_value(&body_size, window, cx));
        self.title_size_select.update(cx, |state, cx| state.set_selected_value(&title_size, window, cx));
        self.summary_prompt_input.update(cx, |state, cx| state.set_value(DEFAULT_SUMMARY_PROMPT, window, cx));
        self.status = "Announcement defaults restored. Apply saves them; Close discards them.".into();
        cx.notify();
    }

    #[cfg(target_os = "linux")]
    fn browse_video(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active_character.clone() else { return; };
        let focus = window.focused(cx);
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true, directories: false, multiple: false,
            prompt: Some("Choose character animation (MP4)".into()),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = paths.await;
            let _ = view.update_in(cx, |view, window, cx| {
                if let Some(focus) = focus { focus.focus(window, cx); }
                match result {
                    Ok(Ok(Some(paths))) => {
                        let Some(selected) = paths.into_iter().next() else { return; };
                        if !selected.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")) {
                            view.status = "Choose an MP4 animation file.".into();
                        } else if view.draft.characters.contains_key(&id) {
                            let existing = view.draft.characters.get(&id).and_then(|character| character.animation_path.clone());
                            let stored = existing.filter(|existing| crate::characters::animation_path(&id, existing, &view.assets) == selected).unwrap_or(selected);
                            view.draft.character(&id).animation_path = Some(stored.clone());
                            if view.active_character.as_ref() == Some(&id) {
                                view.character_video_input.update(cx, |state, cx| state.set_value(stored.to_string_lossy().into_owned(), window, cx));
                            }
                            view.status = "Character video selected. Apply saves the path.".into();
                        }
                    }
                    Ok(Ok(None)) => return,
                    Ok(Err(error)) => view.status = format!("Could not open the video picker: {error}"),
                    Err(error) => view.status = format!("Video picker stopped: {error}"),
                }
                cx.notify();
            });
        }).detach();
    }

    #[cfg(target_os = "linux")]
    fn open_my_voices(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.open_url("https://elevenlabs.io/app/voice-lab");
    }

    #[cfg(target_os = "linux")]
    fn render_offline_voice(&self, _: &mut Context<Self>) -> AnyElement {
        let status = if crate::platform::espeak_program().is_some() {
            "eSpeak is available. Linux offline speech uses your system eSpeak voices."
        } else {
            "eSpeak is unavailable. Install espeak-ng with your Linux package manager to enable offline speech."
        };
        v_flex().w_full().gap_4()
            .child(div().id("offline-status").role(gpui_kit::Role::Status).aria_label(status)
                .a11y_synthetic_children(|builder| builder.parent_node().set_live(gpui_kit::accesskit::Live::Polite))
                .text_base().child(status))
            .child(div().child("Playback uses PulseAudio or PipeWire's PulseAudio compatibility service. Kitten voice downloads are Windows-only."))
            .into_any_element()
    }

    fn render_sidebar(&self, layout: Layout, cx: &mut Context<Self>) -> impl IntoElement {
        let mut navigation = v_flex()
            .id("settings-navigation")
            .role(gpui_kit::Role::TabList)
            .aria_label("Settings pages")
            .aria_orientation(if layout.compact { gpui_kit::Orientation::Horizontal } else { gpui_kit::Orientation::Vertical })
            .key_context(if layout.compact { "HeraldNavigation HeraldNavHorizontal" } else { "HeraldNavigation HeraldNavVertical" })
            .on_action(cx.listener(|view, _: &keyboard::NextItem, window, cx| view.step_page(true, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::PreviousItem, window, cx| view.step_page(false, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::FirstItem, window, cx| view.navigate_page(0, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::LastItem, window, cx| view.navigate_page(PAGES.len() - 1, window, cx)))
            .on_action(cx.listener(|_, _: &keyboard::Activate, _, _| {}))
            .when(layout.compact, |nav| nav.flex_row().flex_wrap().w_full())
            .when(!layout.compact, |nav| nav.size_full().min_h_0().track_scroll(&self.navigation_scroll).overflow_y_scroll())
            .gap_2();
        for (index, spec) in PAGES.iter().enumerate() {
            let selected = self.active_page == spec.page;
            let page = spec.page;
            let focus = self.navigation_focus[index].clone().tab_index(0).tab_stop(selected);
            let button = div().id(spec.key)
                .role(gpui_kit::Role::Tab)
                .accessibility_id(spec.key)
                .aria_label(spec.label)
                .aria_selected(selected)
                .track_focus(&focus)
                .tab_stop(selected)
                .px_3().py_2().rounded_md().cursor_pointer()
                .border_2().border_color(cx.theme().transparent)
                .focus_visible(|style| style.border_color(cx.theme().ring))
                .when(selected, |button| button.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                .when(!layout.compact, |button| button.w_full())
                .flex_shrink_0()
                .child(spec.label)
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.change_page(page, window, cx);
                    view.navigation_focus[index].focus(window, cx);
                }));
            let button = button.into_any_element();
            let button = if layout.compact { button } else { button.reveal(spec.key, &self.navigation_scroll).flex_shrink_0().into_any_element() };
            navigation = navigation.child(button);
        }
        let navigation = if layout.compact {
            navigation.into_any_element()
        } else {
            div().relative().min_h_0().flex_1().w_full().child(navigation).child(Scrollbar::vertical(&self.navigation_scroll)).into_any_element()
        };
        let preference = self.theme_monitor.preference();
        let themes = h_flex()
            .id("theme-preference-buttons")
            .flex_wrap()
            .gap_1()
            .child(
                Button::new("theme-dark")
                    .label("Dark")
                    .accessibility_id("theme-dark")
                    .toggled(preference == AppearancePreference::Dark)
                    .when(preference == AppearancePreference::Dark, |button| button.primary())
                    .on_click(cx.listener(|view, _, window, cx| view.select_theme(AppearancePreference::Dark, window, cx))),
            )
            .child(
                Button::new("theme-light")
                    .label("Light")
                    .accessibility_id("theme-light")
                    .toggled(preference == AppearancePreference::Light)
                    .when(preference == AppearancePreference::Light, |button| button.primary())
                    .on_click(cx.listener(|view, _, window, cx| view.select_theme(AppearancePreference::Light, window, cx))),
            )
            .child(
                Button::new("theme-system")
                    .label("System")
                    .accessibility_id("theme-system")
                    .toggled(preference == AppearancePreference::System)
                    .when(preference == AppearancePreference::System, |button| button.primary())
                    .on_click(cx.listener(|view, _, window, cx| view.select_theme(AppearancePreference::System, window, cx))),
            );
        let theme_group = v_flex()
            .id("theme-preferences")
            .gap_1()
            .when(layout.compact, |group| group.w_full().pt_2().border_t_1())
            .when(!layout.compact, |group| group.mt_4().pt_4().border_t_1())
            .border_color(cx.theme().sidebar_border)
            .child(div().text_sm().child("Theme"))
            .child(themes);
        v_flex()
            .id("settings-sidebar")
            .accessibility_id("settings-sidebar")
            .role(gpui_kit::Role::Group)
            .aria_label("Settings sidebar")
            .when(layout.compact, |sidebar| sidebar.flex_row().flex_wrap().w_full().p_2().border_b_1())
            .when(!layout.compact, |sidebar| sidebar.h_full().w(px(220.)).p_4().border_r_1())
            .flex_shrink_0()
            .gap_2()
            .bg(cx.theme().sidebar)
            .border_color(cx.theme().sidebar_border)
            .child(navigation)
            .child(theme_group)
    }

    fn render_slider(&self, id: &'static str, label: &'static str, state: &Entity<SliderState>, focus: &FocusHandle, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::base::{SliderTrack, SliderIndicator, SliderThumb};
        let value = state.read(cx);
        let min = value.min_value();
        let max = value.max_value();
        let current = value.value().start();
        let step = value.step_value();
        let percentage = value.percentage().end;
        let update = |amount: f32| {
            let state = state.clone();
            move |window: &mut Window, cx: &mut App| {
                state.update(cx, |state, cx| {
                    let value = (state.value().start() + amount).clamp(state.min_value(), state.max_value());
                    state.set_value(value, window, cx);
                    cx.emit(SliderEvent::Change(state.value()));
                });
            }
        };
        let increase = update(step);
        let decrease = update(-step);
        let minimum = state.clone();
        let maximum = state.clone();
        let increment_a11y = update(step);
        let decrement_a11y = update(-step);
        let release = state.clone();
        let mouse_focus = focus.clone();
        div().id(id).role(gpui_kit::Role::Slider).accessibility_id(id).aria_label(label)
            .aria_numeric_value(current as f64).aria_min_numeric_value(min as f64).aria_max_numeric_value(max as f64)
            .aria_numeric_value_step(step as f64).aria_orientation(gpui_kit::Orientation::Horizontal)
            .track_focus(focus).tab_index(0).key_context("HeraldSlider")
            .w_full().h_8().px_2().border_2().border_color(cx.theme().transparent).rounded_md()
            .focus_visible(|style| style.border_color(cx.theme().ring))
            .on_action(move |_: &keyboard::Increase, window, cx| increase(window, cx))
            .on_action(move |_: &keyboard::Decrease, window, cx| decrease(window, cx))
            .on_action(move |_: &keyboard::Minimum, window, cx| minimum.update(cx, |state, cx| {
                state.set_value(min, window, cx);
                cx.emit(SliderEvent::Change(state.value()));
            }))
            .on_action(move |_: &keyboard::Maximum, window, cx| maximum.update(cx, |state, cx| {
                state.set_value(max, window, cx);
                cx.emit(SliderEvent::Change(state.value()));
            }))
            .on_a11y_action(gpui_kit::AccessibleAction::Increment, move |_, window, cx| increment_a11y(window, cx))
            .on_a11y_action(gpui_kit::AccessibleAction::Decrement, move |_, window, cx| decrement_a11y(window, cx))
            .capture_any_mouse_down(move |_, window, cx| mouse_focus.focus(window, cx))
            .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| release.update(cx, |state, cx| state.handle_release(cx)))
            .child(SliderTrack::new(state).axis(gpui_kit::Axis::Horizontal).w_full().h_6().flex().items_center()
                .child(SliderIndicator::new(state).relative().w_full().h(px(6.)).rounded_full().bg(cx.theme().muted)
                    .child(div().absolute().h_full().left_0().w(gpui_kit::relative(percentage)).rounded_full().bg(cx.theme().primary))
                    .child(SliderThumb::new(state).absolute().top(px(-5.)).left(gpui_kit::relative(percentage)).ml(px(-8.)).size_4().rounded_full().bg(cx.theme().primary))))
    }

    fn render_audio(&self, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let volume = v_flex()
            .gap_1()
            .child(div().text_sm().child(format!("Volume: {}%", self.draft.settings.volume)))
            .child(self.render_slider("volume", "Announcer volume", &self.volume_slider, &self.volume_focus, cx).reveal("reveal-volume", &self.page_scroll))
            .into_any_element();
        let output = h_flex()
            .w_full()
            .when(layout.compact, |row| row.flex_col().items_start())
            .gap_2()
            .child(
                Select::new(&self.output_select)
                    .id("output-device")
                    .accessibility_label("Output device")
                    .focus_ring(false)
                    .w_full().reveal("reveal-output", &self.page_scroll).flex_1().when(layout.compact, |field| field.w_full()),
            )
            .child(
                Button::new("refresh-devices")
                    .label("Refresh")
                    .accessibility_id("refresh-devices")
                    .on_click(cx.listener(|view, _, window, cx| view.refresh_devices(window, cx))).reveal("reveal-refresh-devices", &self.page_scroll),
            )
            .into_any_element();
        let preview = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .when(layout.compact, |row| row.flex_col().items_start())
            .child(
                Button::new("preview")
                    .label(if self.preview.is_some() { "Stop example" } else { "Play example" })
                    .accessibility_id("preview")
                    .on_click(cx.listener(|view, _, _, cx| view.start_audio_preview(cx))).reveal("reveal-preview", &self.page_scroll),
            )
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Previews your selected settings without saving."))
            .into_any_element();
        let silent = v_flex()
            .gap_1()
            .child(div().text_sm().child(format!("Silent sound: {} seconds", self.draft.settings.silent_sound_seconds)))
            .child(self.render_slider("silent-sound", "Silent sound", &self.silent_sound_slider, &self.silent_sound_focus, cx).reveal("reveal-silent-sound", &self.page_scroll))
            .into_any_element();
        v_flex().w_full().gap_4().child(volume).child(div().text_sm().child("Output device")).child(output).child(preview).child(silent).into_any_element()
    }

    fn render_time_input(&self, state: &Entity<InputState>, id: &'static str, label: &'static str, enabled: bool, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        div().min_w_0().child(if enabled {
            keyboard::input(state, id, label, window, cx).into_any_element()
        } else {
            div().id(id).role(gpui_kit::Role::TextInput).accessibility_id(id).aria_label(label)
                .a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
                .aria_value(state.read(cx).value().clone()).h_8().px_3().py_1().border_1().rounded(cx.theme().radius)
                .border_color(cx.theme().border).bg(cx.theme().input.opacity(0.5)).text_color(cx.theme().muted_foreground)
                .child(state.read(cx).value().clone()).into_any_element()
        })
    }

    fn render_quiet_hours(&self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let schedule = self.draft.settings.schedule_enabled;
        v_flex()
            .w_full()
            .gap_4()
            .child(
                Checkbox::new("quiet-mode")
                    .label("Quiet mode")
                    .accessibility_label("Quiet mode")
                    .checked(self.draft.settings.quiet_mode)
                    .on_change(cx.listener(|view, value, _, cx| {
                        view.draft.settings.quiet_mode = *value;
                        cx.notify();
                    })).reveal("reveal-quiet-mode", &self.page_scroll),
            )
            .child(
                Checkbox::new("schedule")
                    .label("Daily schedule")
                    .accessibility_label("Daily schedule")
                    .checked(schedule)
                    .on_change(cx.listener(|view, value, _, cx| {
                        view.draft.settings.schedule_enabled = *value;
                        cx.notify();
                    })).reveal("reveal-schedule", &self.page_scroll),
            )
            .child(
                h_flex()
                    .w_full().flex_wrap()
                    .gap_2()
                    .items_center()
                    .when(layout.compact, |row| row.flex_col().items_start())
                    .child(div().text_sm().child("From"))
                    .child(self.render_time_input(&self.quiet_start_input, "quiet-start", "From", schedule, window, cx).w(px(110.)).when(layout.compact, |input| input.w_full()).reveal("reveal-quiet-start", &self.page_scroll).when(layout.compact, |field| field.w_full()))
                    .child(div().text_sm().child("To"))
                    .child(self.render_time_input(&self.quiet_end_input, "quiet-end", "To", schedule, window, cx).w(px(110.)).when(layout.compact, |input| input.w_full()).reveal("reveal-quiet-end", &self.page_scroll).when(layout.compact, |field| field.w_full()))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Use HH:MM. 24:00 is allowed for the end.")),
            )
            .into_any_element()
    }

    fn render_speech_service(&mut self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_usage(window, cx);
        let key_status = api_key_status(self.draft.settings.elevenlabs_api_key.as_deref());
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .when(layout.compact, |row| row.flex_col())
                    .gap_4()
                    .child(v_flex().gap_1().min_w_0().flex_1().when(layout.compact, |field| field.w_full()).child(div().text_sm().child("Speech model")).child(Select::new(&self.model_select).id("speech-model").accessibility_label("Speech model").focus_ring(false).w_full().reveal("reveal-model", &self.page_scroll).w_full()))
                    .child(v_flex().gap_1().min_w_0().flex_1().when(layout.compact, |field| field.w_full()).child(div().text_sm().child("Default voice ID")).child(Input::new(&self.default_voice_input).accessibility_id("default-voice-id").aria_label("Default voice ID").focus_ring(false).border_color(self.input_border_color(&self.default_voice_input, window, cx)).w_full().reveal("reveal-default-voice", &self.page_scroll).w_full())),
            )
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child("Characters without a custom ElevenLabs voice use this ID."))
            .child(v_flex().gap_1().child(div().text_sm().child("ElevenLabs key")).child(Input::new(&self.api_key_input).accessibility_id("api-key").aria_label("ElevenLabs key").content_type(InputContentType::Password).mask_toggle().focus_ring(false).border_color(self.input_border_color(&self.api_key_input, window, cx)).w_full().reveal("reveal-api-key", &self.page_scroll).w_full()).child(div().id("api-key-status").role(gpui_kit::Role::Label).aria_label(key_status.clone()).text_sm().text_color(cx.theme().muted_foreground).child(key_status)))
            .child(keyboard::textarea(&self.usage_input, "voice-usage", "Voice usage", 96., true, window, cx).reveal("reveal-voice-usage", &self.page_scroll).w_full())
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(Button::new("refresh-usage").label("Refresh").accessibility_id("refresh-usage").disabled(matches!(self.voice_usage, VoiceUsageState::Loading { .. })).on_click(cx.listener(|view, _, _, cx| view.refresh_usage(cx))).reveal("reveal-refresh-usage", &self.page_scroll))
                    .child(Button::new("my-voices").label("Open My Voices").on_click(cx.listener(|view, _, window, cx| view.open_my_voices(window, cx))).reveal("reveal-my-voices", &self.page_scroll)),
            )
            .into_any_element()
    }

    #[cfg(target_os = "windows")]
    fn render_offline_voice(&self, cx: &mut Context<Self>) -> AnyElement {
        let installed = crate::tts::installed();
        let status = if self.offline_installing {
            "Installing… Downloading and checking the voice engine and model.".to_owned()
        } else if installed {
            "Installed. Kitten CPU speech is available offline.".to_owned()
        } else if let Some(error) = &self.offline_error {
            format!("Installation failed: {error}")
        } else {
            "Not installed. Download the voice engine and model to enable offline speech.".to_owned()
        };
        let label = if installed { "Installed" } else if self.offline_error.is_some() { "Retry install" } else { "Install" };
        v_flex()
            .w_full()
            .gap_4()
            .child(div().id("offline-status").role(gpui_kit::Role::Status).aria_label(status.clone())
                .a11y_synthetic_children(|builder| builder.parent_node().set_live(gpui_kit::accesskit::Live::Polite))
                .text_base().child(status))
            .child(Button::new("install-voice").label(label).accessibility_id("install-voice").disabled(self.offline_installing || installed).on_click(cx.listener(|view, _, _, cx| view.install_offline_voice(cx))).reveal("reveal-install-voice", &self.page_scroll))
            .into_any_element()
    }

    fn render_characters(&self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self
            .draft
            .characters
            .iter()
            .filter(|(id, _)| !self.draft.removed_characters.contains(*id))
            .map(|(id, character)| (id.clone(), if character.name.trim().is_empty() { "New character".to_owned() } else { character.name.clone() }, self.active_character.as_deref() == Some(id.as_str())))
            .collect::<Vec<_>>();
        let mut list = v_flex().id("character-list").role(gpui_kit::Role::ListBox).aria_label("Characters")
            .track_focus(&self.character_list_focus.clone().tab_stop(!rows.is_empty())).key_context("HeraldCharacters")
            .border_2().border_color(cx.theme().transparent).focus_visible(|style| style.border_color(cx.theme().ring))
            .on_action(cx.listener(|view, _: &keyboard::NextItem, window, cx| view.navigate_character(1, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::PreviousItem, window, cx| view.navigate_character(-1, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::FirstItem, window, cx| view.navigate_character(i32::MIN, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::LastItem, window, cx| view.navigate_character(i32::MAX, window, cx)))
            .w_full().gap_1();
        for (id, name, selected) in rows {
            let row = div().id(format!("character-{id}")).role(gpui_kit::Role::ListBoxOption).aria_label(name.clone())
                .aria_selected(selected).when(selected, |row| row.aria_active_descendant().anchor_scroll(Some(self.character_anchor.clone())).bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                .w_full().p_2().rounded_md().cursor_pointer().child(name)
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.select_character(id.clone(), window, cx);
                    view.character_list_focus.focus(window, cx);
                }));
            list = list.child(row);
        }
        let list = v_flex().w(px(230.)).when(layout.compact, |list| list.w_full()).flex_shrink_0().min_w_0().gap_2()
            .child(div().relative().w_full().h(px(layout.character_list_height)).overflow_hidden()
                .child(list.size_full().track_scroll(&self.character_scroll).overflow_y_scroll())
                .child(div().absolute().inset_0().child(Scrollbar::vertical(&self.character_scroll).viewport_from_layout()))
                .reveal("reveal-character-list", &self.page_scroll).w_full())
            .child(Button::new("new-character").label("New").accessibility_id("new-character").secondary().on_click(cx.listener(|view, _, window, cx| view.add_character(window, cx))).reveal("reveal-new-character", &self.page_scroll));
        let editor = if self.active_character.is_some() {
            let selected = self
                .active_character
                .as_ref()
                .and_then(|id| self.draft.characters.get(id))
                .is_some_and(|character| character.selected);
            v_flex()
                .min_w_0()
                .flex_1()
                .when(layout.compact, |editor| editor.w_full())
                .gap_2()
                .child(div().text_sm().child("Character name"))
                .child(Input::new(&self.character_name_input).accessibility_id("character-name").aria_label("Character name").focus_ring(false).border_color(self.input_border_color(&self.character_name_input, window, cx)).w_full().reveal("reveal-character-name", &self.page_scroll).w_full())
                .child(div().text_sm().child("Character video"))
                .child(h_flex().w_full().gap_2().when(layout.compact, |row| row.flex_col().items_start()).child(Input::new(&self.character_video_input).accessibility_id("character-video").aria_label("Character video").readonly(true).focus_ring(false).border_color(self.input_border_color(&self.character_video_input, window, cx)).w_full().reveal("reveal-character-video", &self.page_scroll).flex_1().when(layout.compact, |field| field.w_full())).child(Button::new("browse-video").label("Browse").on_click(cx.listener(|view, _, window, cx| view.browse_video(window, cx))).reveal("reveal-browse-video", &self.page_scroll)))
                .child(div().text_sm().child("ElevenLabs voice ID"))
                .child(Input::new(&self.character_voice_input).accessibility_id("character-voice-id").aria_label("ElevenLabs voice ID").focus_ring(false).border_color(self.input_border_color(&self.character_voice_input, window, cx)).w_full().reveal("reveal-character-voice", &self.page_scroll).w_full())
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(Button::new("character-preview").label(if self.voice_preview.is_some() { "Stop example" } else { "Play voice example" }).accessibility_id("character-preview").on_click(cx.listener(|view, _, _, cx| view.start_voice_preview(cx))).reveal("reveal-character-preview", &self.page_scroll))
                        .child(Button::new("delete-character").label("Delete").accessibility_id("delete-character").danger().on_click(cx.listener(|view, _, window, cx| view.delete_character(window, cx))).reveal("reveal-delete-character", &self.page_scroll)),
                )
                .child(Checkbox::new("character-selected").label("Selected").accessibility_label("Selected").checked(selected).on_change(cx.listener(|view, value, _, cx| {
                    if let Some(id) = view.active_character.clone() {
                        view.draft.character(&id).selected = *value;
                        cx.notify();
                    }
                })).reveal("reveal-character-selected", &self.page_scroll))
                .child(div().text_sm().child("Summary prompt (blank uses the default prompt)"))
                .child(keyboard::textarea(&self.character_prompt_input, "character-prompt", "Character summary prompt", 180., false, window, cx).reveal("reveal-character-prompt", &self.page_scroll).w_full())
                .into_any_element()
        } else {
            v_flex().flex_1().min_w_0().child(div().text_base().child("Choose a character or create a new one." )).into_any_element()
        };
        h_flex().w_full().items_start().when(layout.compact, |row| row.flex_col()).gap_4().child(list).child(editor).into_any_element()
    }

    fn render_announcements(&mut self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let font_row = |label: &'static str, font: &Entity<ChoiceSelect>, size_state: &Entity<ChoiceSelect>, font_id: &'static str, size_id: &'static str| {
            h_flex()
                .w_full()
                .when(layout.compact, |row| row.flex_col())
                .gap_2()
                .child(v_flex().gap_1().min_w_0().flex_1().when(layout.compact, |field| field.w_full()).child(div().text_sm().child(label)).child(Select::new(font).id(font_id).accessibility_label(label).focus_ring(false).w_full().reveal(font_id, &self.page_scroll).w_full()))
                .child(v_flex().gap_1().w(px(130.)).when(layout.compact, |field| field.w_full()).child(div().text_sm().child(format!("{label} size"))).child(Select::new(size_state).id(size_id).accessibility_label(format!("{label} size")).focus_ring(false).w_full().reveal(size_id, &self.page_scroll).w_full()))
        };
        v_flex()
            .w_full()
            .gap_4()
            .child(font_row("Body font", &self.body_font_select, &self.body_size_select, "body-font", "body-size"))
            .child(font_row("Title font", &self.title_font_select, &self.title_size_select, "title-font", "title-size"))
            .child(div().text_sm().child("Summary prompt"))
            .child(keyboard::textarea(&self.summary_prompt_input, "summary-prompt", "Summary prompt", 220., false, window, cx).reveal("reveal-summary-prompt", &self.page_scroll).w_full())
            .child(Button::new("reset-defaults").label("Reset defaults").accessibility_id("reset-defaults").secondary().on_click(cx.listener(|view, _, window, cx| view.reset_defaults(window, cx))).reveal("reveal-reset-defaults", &self.page_scroll))
            .into_any_element()
    }

    fn render_page(&mut self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.active_page {
            Page::Characters => self.render_characters(layout, window, cx),
            Page::Audio => self.render_audio(layout, cx),
            Page::QuietHours => self.render_quiet_hours(layout, window, cx),
            Page::SpeechService => self.render_speech_service(layout, window, cx),
            Page::OfflineVoice => self.render_offline_voice(cx),
            Page::Announcements => self.render_announcements(layout, window, cx),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.initial_focus_pending {
            self.initial_focus_pending = false;
            let focus = self.navigation_focus[1].clone();
            window.on_next_frame(move |window, cx| focus.focus(window, cx));
        }
        let layout = Layout::for_window(window);
        let page = self.active_page.spec();
        let page_content = self.render_page(layout, window, cx);
        let footer = h_flex()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .pt_3()
            .child(div().id("status").role(gpui_kit::Role::Status).accessibility_id("status")
                .aria_label(format!("Settings status: {}", self.status))
                .a11y_synthetic_children(|builder| builder.parent_node().set_live(gpui_kit::accesskit::Live::Polite))
                .min_h(px(if layout.compact { 48. } else { 72. })).min_w_0().flex_1().text_sm().child(self.status.clone()))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .child(Button::new("apply").label("Apply").accessibility_id("apply").primary().on_click(cx.listener(|view, _, _, cx| view.apply(cx))))
                    .child(Button::new("close").label("Close").accessibility_id("close").disabled(self.offline_installing).secondary().on_click(cx.listener(|view, _, window, cx| {
                        if view.should_close(window, cx) {
                            window.remove_window();
                        }
                    }))),
            );
        gpui_kit::div()
            .id("settings-view")
            .key_context("HeraldSettings")
            .on_action(cx.listener(|view, _: &keyboard::Apply, _, cx| view.apply(cx)))
            .on_action(cx.listener(|view, _: &keyboard::NextPage, window, cx| view.step_page(true, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::PreviousPage, window, cx| view.step_page(false, window, cx)))
            .on_action(cx.listener(|_, _: &keyboard::NextField, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &keyboard::PreviousField, window, cx| window.focus_prev(cx)))
            .on_action(cx.listener(|view, _: &keyboard::FirstChoice, window, cx| view.choice_boundary(false, window, cx)))
            .on_action(cx.listener(|view, _: &keyboard::LastChoice, window, cx| view.choice_boundary(true, window, cx)))
            .size_full()
            .flex()
            .when(layout.compact, |root| root.flex_col())
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(layout, cx))
            .child(
                v_flex()
                    .min_h_0()
                    .min_w_0()
                    .flex_1()
                    .when(layout.compact, |body| body.w_full().p_3())
                    .when(!layout.compact, |body| body.h_full().p_8())
                    .gap_3()
                    .child(div().id("page-scroll-wrapper").relative().min_h_0().flex_1().w_full().overflow_hidden()
                        .child(v_flex().id("page-scroll").size_full().track_scroll(&self.page_scroll).overflow_y_scroll().child(
                         v_flex().w_full().min_w_0().gap_3().flex_none()
                            .child(div().id("page-title").role(gpui_kit::Role::Heading).accessibility_id("page-title").aria_label(page.label).text_2xl().font_weight(FontWeight::SEMIBOLD).child(page.label))
                            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(page.description))
                            .child(page_content)
                        ))
                        .child(div().absolute().inset_0().child(Scrollbar::vertical(&self.page_scroll).viewport_from_layout())))
                    .child(footer),
            )
    }
}

fn initial_voice_usage_state(api_key: Option<&str>) -> VoiceUsageState {
    if api_key.is_some_and(|key| !key.trim().is_empty()) { VoiceUsageState::NotLoaded } else { VoiceUsageState::NoKey }
}

fn api_key_status(api_key: Option<&str>) -> String {
    match api_key.filter(|key| !key.trim().is_empty()).map(crate::elevenlabs::validate_api_key) {
        Some(Ok(())) => "Using the key entered in Settings. Install Offline voice for local fallback.".into(),
        None => "Enter an ElevenLabs key, or install local speech in Offline voice.".into(),
        Some(Err(error)) => format!("ElevenLabs key unavailable: {error}"),
    }
}

fn voice_usage_text(state: &VoiceUsageState) -> String {
    match state {
        VoiceUsageState::NoKey => "Enter an ElevenLabs key to load voice usage.".into(),
        VoiceUsageState::NotLoaded => "Refresh to load ElevenLabs voice usage.".into(),
        VoiceUsageState::Loading { .. } => "Loading ElevenLabs voice usage…".into(),
        VoiceUsageState::Failed(error) => format!("Voice usage unavailable: {error}"),
        VoiceUsageState::Ready(usage) => {
            let slots = format!("Voice slots: {} of {} used; {} remaining.", usage.voice_slots_used, usage.voice_limit, usage.remaining_voice_slots());
            let edits = match usage.voice_add_edit_allowance() {
                Some((limit, remaining)) => format!("Voice additions and edits: {} of {} used; {} remaining this billing period.", usage.voice_add_edit_counter, limit, remaining),
                None => format!("Voice additions and edits: {} used; limit unknown; remaining unknown.", usage.voice_add_edit_counter),
            };
            format!("{slots}\n{edits}")
        }
    }
}

fn slider_u16(value: &SliderValue, min: u16, max: u16) -> u16 {
    value.start().round().clamp(f32::from(min), f32::from(max)) as u16
}

fn output_value(selected: Option<&str>) -> &str {
    selected.unwrap_or("")
}

fn output_choices(devices: &[crate::audio::OutputDevice], selected: Option<&str>) -> Vec<Choice> {
    let mut choices = vec![Choice::new("", "System default")];
    choices.extend(devices.iter().map(|device| Choice::new(device.id.clone(), device.name.clone())));
    if let Some(selected) = selected {
        if !devices.iter().any(|device| device.id == selected) {
            choices.push(Choice::new(selected, "Selected device unavailable (using system default)"));
        }
    }
    choices
}

fn font_choices(families: &[String], selected: &str) -> Vec<Choice> {
    let mut choices = families.iter().map(|family| Choice::new(family, family)).collect::<Vec<_>>();
    if !choices.iter().any(|choice| choice.value == selected) {
        choices.insert(0, Choice::new(selected, selected));
    }
    if choices.is_empty() {
        choices.push(Choice::new(selected, selected));
    }
    choices
}

fn font_size_choices() -> Vec<Choice> {
    (MIN_FONT_SIZE..=MAX_FONT_SIZE).map(|size| Choice::new(size.to_string(), size.to_string())).collect()
}

fn choice_index(choices: &[Choice], value: &str) -> usize {
    choices.iter().position(|choice| choice.value == value).unwrap_or(0)
}

pub fn run(data: &Path, assets: &Path) -> Result<(), String> {
    let settings = Settings::load(data)?;
    let Some(instance) = instance::InstanceGuard::acquire(data)? else { return Ok(()); };
    let instance_guard = Rc::new(instance);
    let instance = instance_guard.clone();
    let data = data.to_path_buf();
    let assets = assets.to_path_buf();
    let launch_error = Rc::new(RefCell::new(None));
    let launch_error_for_app = launch_error.clone();
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        keyboard::init(cx);
        let options = WindowOptions {
            app_id: Some("herald-settings".into()),
            window_bounds: Some(WindowBounds::centered(size(px(960.), px(720.)), cx)),
            titlebar: Some(gpui_kit::TitlebarOptions { title: Some("Herald settings".into()), ..Default::default() }),
            ..WindowOptions::default()
        };
        match gpui_kit::open_window(options, cx, |window, cx| {
            if let Err(error) = instance.register(window) {
                *launch_error_for_app.borrow_mut() = Some(error);
            }
            cx.new(|cx| SettingsView::new(data, assets, settings, instance.clone(), window, cx))
        }) {
            Ok((handle, _view)) => {
                let _ = handle.downcast::<Root>();
            }
            Err(error) => {
                *launch_error_for_app.borrow_mut() = Some(error.to_string());
                cx.quit();
            }
        }
    });
    let launch_error = launch_error.borrow_mut().take();
    instance_guard.keep_alive();
    launch_error.map_or(Ok(()), Err)
}
