#[cfg(not(target_os = "windows"))]
pub fn run(_data: &std::path::Path, _assets: &std::path::Path) -> Result<(), String> {
    Err("The settings app is currently available on Windows.".into())
}

#[cfg(target_os = "windows")]
mod native {
    use crate::audio::OutputDevice;
    use crate::characters::{validate_registry, Character, CharacterVoice};
    use crate::elevenlabs::{Client, SpeechModel, VoiceUsage};
    use crate::settings::{format_time, parse_time, Settings};
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, Receiver};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::{Controls::*, WindowsAndMessaging::*};
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;

    const TBM_GETPOS: u32 = WM_USER;

    const QUIET: i32 = 101;
    const SCHEDULE: i32 = 102;
    const START: i32 = 103;
    const END: i32 = 104;
    const VOLUME: i32 = 105;
    const OUTPUT: i32 = 106;
    const APPLY: i32 = 107;
    const CLOSE: i32 = 108;
    const STATUS: i32 = 109;
    const VOLUME_LABEL: i32 = 110;
    const REFRESH: i32 = 111;
    const PREVIEW: i32 = 112;
    const MODEL: i32 = 113;
    const API_KEY: i32 = 114;
    const API_KEY_LABEL: i32 = 116;
    const API_KEY_HINT: i32 = 117;
    const VOICE_USAGE: i32 = 118;
    const USAGE_REFRESH: i32 = 119;
    const MY_VOICES: i32 = 120;
    const DEFAULT_VOICE_ID: i32 = 121;
    const DEFAULT_VOICE_ID_LABEL: i32 = 122;
    const DEFAULT_VOICE_ID_HINT: i32 = 123;
    const OFFLINE_STATUS: i32 = 124;
    const OFFLINE_INSTALL: i32 = 125;
    const SIDEBAR: i32 = 200;
    const CHARACTER_LIST: i32 = 201;
    const NEW_CHARACTER: i32 = 202;
    const CHARACTER_NAME: i32 = 203;
    const VIDEO_PATH: i32 = 206;
    const VOICE_ID: i32 = 216;
    const VOICE_ID_LABEL: i32 = 217;
    const PLAY_VOICE: i32 = 210;
    const REMOVE_CHARACTER: i32 = 214;
    const CHARACTER_EMPTY: i32 = 402;
    const QUIET_HINT: i32 = 301;
    const START_LABEL: i32 = 303;
    const END_LABEL: i32 = 304;
    const TIME_HINT: i32 = 305;
    const OUTPUT_LABEL: i32 = 306;
    const MODEL_LABEL: i32 = 307;
    const PREVIEW_HINT: i32 = 308;
    const NAME_LABEL: i32 = 310;
    const VIDEO_LABEL: i32 = 313;
    const PAGE_TITLE: i32 = 400;
    const PAGE_HINT: i32 = 401;
    const SHOW_ON_DESKTOP: usize = 0x43415354;
    const VOICE_EXAMPLE: &str = "I bring news for your attention. Listen as I deliver this announcement. Your work is ready, and every check has passed.";

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Page {
        Characters,
        Audio,
        QuietHours,
        SpeechService,
        OfflineVoice,
    }

    impl Page {
        fn index(self) -> usize {
            PAGE_SPECS.iter().position(|spec| spec.page == self).unwrap()
        }
    }

    struct PageSpec {
        page: Page,
        label: &'static str,
        hint: &'static str,
        controls: &'static [i32],
    }

    const CHARACTER_PAGE_CONTROLS: &[i32] = &[
        CHARACTER_LIST,
        NEW_CHARACTER,
        CHARACTER_EMPTY,
        NAME_LABEL,
        CHARACTER_NAME,
        VOICE_ID_LABEL,
        VOICE_ID,
        VIDEO_LABEL,
        VIDEO_PATH,
        PLAY_VOICE,
        REMOVE_CHARACTER,
    ];
    const AUDIO_PAGE_CONTROLS: &[i32] = &[
        VOLUME_LABEL,
        VOLUME,
        OUTPUT_LABEL,
        OUTPUT,
        REFRESH,
        PREVIEW,
        PREVIEW_HINT,
    ];
    const QUIET_HOURS_PAGE_CONTROLS: &[i32] = &[
        QUIET,
        QUIET_HINT,
        SCHEDULE,
        START_LABEL,
        START,
        END_LABEL,
        END,
        TIME_HINT,
    ];
    const SPEECH_SERVICE_PAGE_CONTROLS: &[i32] = &[
        MODEL_LABEL,
        MODEL,
        DEFAULT_VOICE_ID_LABEL,
        DEFAULT_VOICE_ID,
        DEFAULT_VOICE_ID_HINT,
        API_KEY_LABEL,
        API_KEY,
        API_KEY_HINT,
        VOICE_USAGE,
        USAGE_REFRESH,
        MY_VOICES,
    ];

    static PAGE_SPECS: [PageSpec; 5] = [
        PageSpec {
            page: Page::Characters,
            label: "Characters",
            hint: "Edit animation and voice details, or create a new character.",
            controls: CHARACTER_PAGE_CONTROLS,
        },
        PageSpec {
            page: Page::Audio,
            label: "Audio",
            hint: "Set announcement volume, output device, and audio preview.",
            controls: AUDIO_PAGE_CONTROLS,
        },
        PageSpec {
            page: Page::QuietHours,
            label: "Quiet hours",
            hint: "Mute speech or schedule quiet hours in local time.",
            controls: QUIET_HOURS_PAGE_CONTROLS,
        },
        PageSpec {
            page: Page::SpeechService,
            label: "Speech service",
            hint: "Choose a model and optionally configure ElevenLabs.",
            controls: SPEECH_SERVICE_PAGE_CONTROLS,
        },
        PageSpec {
            page: Page::OfflineVoice,
            label: "Offline voice",
            hint: "Install Kitten CPU speech for use without ElevenLabs or an internet connection.",
            controls: &[OFFLINE_STATUS, OFFLINE_INSTALL],
        },
    ];

    fn page_spec(page: Page) -> &'static PageSpec {
        PAGE_SPECS.iter().find(|spec| spec.page == page).unwrap()
    }

    fn page_from_index(index: isize) -> Option<Page> {
        if index < 0 { return None; }
        PAGE_SPECS.get(index as usize).map(|spec| spec.page)
    }

    #[repr(C)]
    struct DesktopRequest {
        kind: usize,
        size: u32,
        desktop: *const windows::core::GUID,
    }

    enum VoiceUsageState {
        NoKey,
        NotLoaded,
        Loading(Receiver<Result<VoiceUsage, String>>),
        Ready(VoiceUsage),
        Failed(String),
    }

    struct UiResources {
        body: HFONT,
        heading: HFONT,
        muted: HFONT,
        main_brush: HBRUSH,
        sidebar_brush: HBRUSH,
    }

    impl UiResources {
        unsafe fn new() -> Result<Self, String> {
            let face = wide("Segoe UI");
            let mut ui = Self {
                body: std::ptr::null_mut(),
                heading: std::ptr::null_mut(),
                muted: std::ptr::null_mut(),
                main_brush: std::ptr::null_mut(),
                sidebar_brush: std::ptr::null_mut(),
            };
            let font = |height, weight| {
                let handle = CreateFontW(height, 0, 0, 0, weight, 0, 0, 0, 0, 0, 0, 0, 0, face.as_ptr());
                if handle.is_null() { Err(std::io::Error::last_os_error().to_string()) } else { Ok(handle) }
            };
            ui.body = font(-16, 400)?;
            ui.heading = font(-26, 600)?;
            ui.muted = font(-15, 400)?;
            ui.main_brush = CreateSolidBrush(color(255, 255, 255));
            if ui.main_brush.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
            ui.sidebar_brush = CreateSolidBrush(color(246, 248, 251));
            if ui.sidebar_brush.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
            Ok(ui)
        }
    }

    impl Drop for UiResources {
        fn drop(&mut self) {
            unsafe {
                if !self.body.is_null() { DeleteObject(self.body as HGDIOBJ); }
                if !self.heading.is_null() { DeleteObject(self.heading as HGDIOBJ); }
                if !self.muted.is_null() { DeleteObject(self.muted as HGDIOBJ); }
                if !self.main_brush.is_null() { DeleteObject(self.main_brush as HGDIOBJ); }
                if !self.sidebar_brush.is_null() { DeleteObject(self.sidebar_brush as HGDIOBJ); }
            }
        }
    }

    struct Form {
        data: PathBuf,
        assets: PathBuf,
        settings: Settings,
        devices: Vec<OutputDevice>,
        missing: Option<String>,
        preview: Option<crate::platform::Preview>,
        voice_preview: Option<crate::platform::Preview>,
        drafts: BTreeMap<String, Character>,
        active_draft: Option<String>,
        selected_character: Option<String>,
        character_ids: Vec<String>,
        removed_characters: std::collections::BTreeSet<String>,
        voice_usage: VoiceUsageState,
        offline_install: Option<Receiver<Result<(), String>>>,
        updating: bool,
        active_page: Page,
        ui: UiResources,
    }

    fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }

    fn color(red: u8, green: u8, blue: u8) -> u32 {
        red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
    }

    fn volume_label(volume: u16) -> String {
        if volume == 0 { "Announcer volume: muted".into() }
        else { format!("Announcer volume: {volume}% ({:.1} dB)", -60.0 + 0.6 * f64::from(volume)) }
    }

    unsafe fn text(window: HWND, id: i32) -> String {
        let control = GetDlgItem(window, id);
        let mut buffer = vec![0u16; GetWindowTextLengthW(control) as usize + 1];
        let length = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..length as usize])
    }

    unsafe fn label(window: HWND, id: i32, value: &str) {
        SetWindowTextW(GetDlgItem(window, id), wide(value).as_ptr());
    }

    unsafe fn checked(window: HWND, id: i32) -> bool {
        SendMessageW(GetDlgItem(window, id), BM_GETCHECK, 0, 0) == BST_CHECKED as isize
    }

    unsafe fn set_checked(window: HWND, id: i32, value: bool) {
        SendMessageW(GetDlgItem(window, id), BM_SETCHECK, if value { BST_CHECKED } else { BST_UNCHECKED } as usize, 0);
    }

    unsafe fn control(window: HWND, class: &str, title: &str, id: i32, style: u32, bounds: (i32, i32, i32, i32)) -> Result<(), String> {
        let (x, y, width, height) = bounds;
        let control = CreateWindowExW(0, wide(class).as_ptr(), wide(title).as_ptr(), WS_CHILD | WS_VISIBLE | style,
            x, y, width, height, window, id as usize as HMENU, GetModuleHandleW(std::ptr::null()), std::ptr::null());
        if control.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
        SendMessageW(control, WM_SETFONT, GetStockObject(DEFAULT_GUI_FONT) as usize, 1);
        Ok(())
    }

    unsafe fn set_font(window: HWND, id: i32, font: HFONT) {
        let control = GetDlgItem(window, id);
        if !control.is_null() { SendMessageW(control, WM_SETFONT, font as usize, 1); }
    }

    unsafe fn apply_fonts(window: HWND, form: &Form) {
        set_font(window, SIDEBAR, form.ui.body);
        for spec in &PAGE_SPECS {
            for id in spec.controls { set_font(window, *id, form.ui.body); }
        }
        for id in [APPLY, CLOSE, STATUS] { set_font(window, id, form.ui.body); }
        set_font(window, PAGE_TITLE, form.ui.heading);
        set_font(window, PAGE_HINT, form.ui.muted);
        for id in [QUIET_HINT, TIME_HINT, PREVIEW_HINT, API_KEY_HINT, DEFAULT_VOICE_ID_HINT, VOICE_USAGE, CHARACTER_EMPTY, STATUS] {
            set_font(window, id, form.ui.muted);
        }
    }

    fn empty_draft() -> Character { Character::default() }

    fn voice_id_text(voice: &CharacterVoice) -> &str {
        match voice {
            CharacterVoice::Local { .. } => "",
            CharacterVoice::ElevenLabs { voice_id } => voice_id,
        }
    }

    fn capture_draft_voice(draft: &mut Character, entered_id: &str) {
        let voice_id = entered_id.trim();
        if !voice_id.is_empty() {
            draft.voice = CharacterVoice::ElevenLabs { voice_id: voice_id.to_owned() };
        } else if matches!(draft.voice, CharacterVoice::ElevenLabs { .. }) {
            draft.voice = CharacterVoice::default();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn entered_voice_id_is_trimmed_and_clearing_restores_local() {
            let mut draft = empty_draft();
            capture_draft_voice(&mut draft, "  own-voice_123  ");
            assert_eq!(draft.voice, CharacterVoice::ElevenLabs { voice_id: "own-voice_123".into() });
            capture_draft_voice(&mut draft, "  ");
            assert_eq!(draft.voice, CharacterVoice::Local { speaker: None });
        }

        #[test]
        fn local_speaker_is_preserved_until_an_id_is_entered() {
            let mut draft = empty_draft();
            draft.voice = CharacterVoice::Local { speaker: Some("Luna".into()) };
            capture_draft_voice(&mut draft, "");
            assert_eq!(draft.voice, CharacterVoice::Local { speaker: Some("Luna".into()) });
            capture_draft_voice(&mut draft, "own-voice");
            assert_eq!(draft.voice, CharacterVoice::ElevenLabs { voice_id: "own-voice".into() });
        }

        #[test]
        fn name_video_edits_preserve_entered_voice_id() {
            let mut draft = empty_draft();
            capture_draft_voice(&mut draft, "own-voice");
            assert_eq!(draft.voice, CharacterVoice::ElevenLabs { voice_id: "own-voice".into() });
            draft.name = "Renamed character".into();
            draft.animation_path = Some(PathBuf::from("changed.mp4"));
            capture_draft_voice(&mut draft, "own-voice");
            assert_eq!(draft.voice, CharacterVoice::ElevenLabs { voice_id: "own-voice".into() });
        }
    }

    unsafe fn draft_for<'a>(form: &'a mut Form, id: &str) -> &'a mut Character {
        if !form.drafts.contains_key(id) {
            let draft = form.settings.characters.get(id).cloned().unwrap_or_else(empty_draft);
            form.drafts.insert(id.to_owned(), draft);
        }
        form.drafts.get_mut(id).unwrap()
    }

    unsafe fn capture_current_draft(window: HWND, form: &mut Form) {
        if form.updating { return; }
        let Some(id) = form.active_draft.clone() else { return; };
        let name = text(window, CHARACTER_NAME);
        let entered_id = text(window, VOICE_ID);
        let video = text(window, VIDEO_PATH);
        let assets = form.assets.clone();
        let draft = draft_for(form, &id);
        draft.name = name;
        capture_draft_voice(draft, &entered_id);
        let video = PathBuf::from(video);
        draft.animation_path = if video.as_os_str().is_empty() || video == PathBuf::from("Choose video…") {
            None
        } else if draft.animation_path.as_ref().is_some_and(|path| crate::characters::animation_path(&id, path, &assets) == video) {
            draft.animation_path.clone()
        } else {
            Some(video)
        };
        let name = if draft.name.trim().is_empty() { "New character".to_owned() } else { draft.name.clone() };
        let index = form.character_ids.iter().position(|candidate| candidate == &id);
        if let Some(index) = index {
            let list = GetDlgItem(window, CHARACTER_LIST);
            SendMessageW(list, LB_DELETESTRING, index, 0);
            SendMessageW(list, LB_INSERTSTRING, index, wide(&name).as_ptr() as isize);
            SendMessageW(list, LB_SETCURSEL, index, 0);
        }
    }

    unsafe fn refresh_page_visibility(window: HWND, form: &Form) {
        let spec = page_spec(form.active_page);
        label(window, PAGE_TITLE, spec.label);
        label(window, PAGE_HINT, spec.hint);
        for page in &PAGE_SPECS {
            let visibility = if page.page == form.active_page { SW_SHOW } else { SW_HIDE };
            for id in page.controls { ShowWindow(GetDlgItem(window, *id), visibility); }
        }
        let show_editor = form.active_page == Page::Characters && form.active_draft.is_some();
        for id in [NAME_LABEL, CHARACTER_NAME, VOICE_ID_LABEL, VOICE_ID, VIDEO_LABEL, VIDEO_PATH, PLAY_VOICE, REMOVE_CHARACTER] {
            ShowWindow(GetDlgItem(window, id), if show_editor { SW_SHOW } else { SW_HIDE });
        }
        ShowWindow(GetDlgItem(window, CHARACTER_EMPTY), if form.active_page == Page::Characters && !show_editor { SW_SHOW } else { SW_HIDE });
    }

    unsafe fn set_draft_controls(window: HWND, form: &mut Form) {
        form.updating = true;
        if let Some(id) = form.active_draft.clone() {
            let assets = form.assets.clone();
            let draft = draft_for(form, &id);
            label(window, CHARACTER_NAME, &draft.name);
            label(window, VOICE_ID, voice_id_text(&draft.voice));
            label(window, VIDEO_PATH, &draft.animation_path.as_ref().map_or_else(|| "Choose video…".into(), |path| crate::characters::animation_path(&id, path, &assets).to_string_lossy().into_owned()));
        } else {
            for id in [CHARACTER_NAME, VOICE_ID, VIDEO_PATH] { label(window, id, ""); }
        }
        form.updating = false;
        refresh_page_visibility(window, form);
    }

    unsafe fn refresh_characters(window: HWND, form: &mut Form) {
        let list = GetDlgItem(window, CHARACTER_LIST);
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        form.character_ids.clear();
        for id in form.settings.characters.keys() {
            if !form.removed_characters.contains(id) { form.character_ids.push(id.clone()); }
        }
        for id in form.drafts.keys() {
            if !form.character_ids.contains(id) { form.character_ids.push(id.clone()); }
        }
        form.character_ids.sort();
        for id in &form.character_ids {
            let name = form.drafts.get(id).map(|draft| draft.name.clone()).or_else(|| form.settings.characters.get(id).map(|character| character.name.clone())).unwrap_or_default();
            let label = if name.trim().is_empty() { "New character".to_owned() } else { name };
            SendMessageW(list, LB_ADDSTRING, 0, wide(&label).as_ptr() as isize);
        }
        if let Some(id) = form.active_draft.as_deref().and_then(|id| form.character_ids.iter().position(|candidate| candidate == id)) {
            SendMessageW(list, LB_SETCURSEL, id, 0);
        } else if let Some(index) = form.selected_character.as_deref().and_then(|selected| form.character_ids.iter().position(|candidate| candidate == selected)) {
            form.active_draft = Some(form.character_ids[index].clone());
            SendMessageW(list, LB_SETCURSEL, index, 0);
        }
        form.selected_character = form.active_draft.clone();
        EnableWindow(list, (!form.character_ids.is_empty()) as i32);
        set_draft_controls(window, form);
    }

    unsafe fn new_character_id(form: &Form) -> String {
        let mut id = format!("character-{}", crate::state::timestamp());
        let mut suffix = 1;
        while form.settings.characters.contains_key(&id) || form.drafts.contains_key(&id) {
            id = format!("character-{}-{suffix}", crate::state::timestamp());
            suffix += 1;
        }
        id
    }

    unsafe fn select_character(window: HWND, form: &mut Form, index: usize) {
        capture_current_draft(window, form);
        stop_previews(window, form);
        let Some(id) = form.character_ids.get(index).cloned() else { return; };
        form.active_draft = Some(id.clone());
        form.selected_character = Some(id.clone());
        SendMessageW(GetDlgItem(window, CHARACTER_LIST), LB_SETCURSEL, index, 0);
        let _ = draft_for(form, &id);
        set_draft_controls(window, form);
    }

    unsafe fn select_page(window: HWND, form: &mut Form, page: Page) {
        let entering = form.active_page != page;
        if entering {
            capture_current_draft(window, form);
            stop_previews(window, form);
            form.active_page = page;
        }
        SendMessageW(GetDlgItem(window, SIDEBAR), LB_SETCURSEL, page.index(), 0);
        refresh_page_visibility(window, form);
        if entering && page == Page::SpeechService { refresh_voice_usage(window, form, false); }
        if entering && page == Page::OfflineVoice && form.offline_install.is_none() { render_offline_voice(window, form); }
    }

    unsafe fn populate_pages(window: HWND, form: &Form) {
        let sidebar = GetDlgItem(window, SIDEBAR);
        SendMessageW(sidebar, LB_RESETCONTENT, 0, 0);
        for spec in &PAGE_SPECS {
            SendMessageW(sidebar, LB_ADDSTRING, 0, wide(spec.label).as_ptr() as isize);
        }
        SendMessageW(sidebar, LB_SETITEMHEIGHT, 0, 38);
        SendMessageW(sidebar, LB_SETCURSEL, form.active_page.index(), 0);
    }

    unsafe fn populate_outputs(window: HWND, form: &mut Form, selected: Option<&str>) {
        let output = GetDlgItem(window, OUTPUT);
        SendMessageW(output, CB_RESETCONTENT, 0, 0);
        SendMessageW(output, CB_ADDSTRING, 0, wide("System default").as_ptr() as isize);
        form.devices = crate::audio::output_devices();
        let mut selection = 0;
        for (index, device) in form.devices.iter().enumerate() {
            SendMessageW(output, CB_ADDSTRING, 0, wide(&device.name).as_ptr() as isize);
            if selected == Some(device.id.as_str()) { selection = index + 1; }
        }
        form.missing = None;
        if selection == 0 && selected.is_some() {
            SendMessageW(output, CB_ADDSTRING, 0, wide("Selected device unavailable (using system default)").as_ptr() as isize);
            selection = form.devices.len() + 1;
            form.missing = selected.map(String::from);
        }
        SendMessageW(output, CB_SETCURSEL, selection, 0);
    }

    unsafe fn selected_output(window: HWND, form: &Form) -> Option<String> {
        let index = SendMessageW(GetDlgItem(window, OUTPUT), CB_GETCURSEL, 0, 0);
        if index <= 0 {
            None
        } else {
            form.devices.get(index as usize - 1).map(|device| device.id.clone()).or_else(|| form.missing.clone())
        }
    }

    unsafe fn read_speech_settings(window: HWND, form: &Form) -> Settings {
        let mut settings = form.settings.clone();
        settings.volume = SendMessageW(GetDlgItem(window, VOLUME), TBM_GETPOS, 0, 0) as u16;
        settings.output_device = selected_output(window, form);
        let index = SendMessageW(GetDlgItem(window, MODEL), CB_GETCURSEL, 0, 0);
        settings.speech_model = SpeechModel::ALL.get(index.max(0) as usize).copied().unwrap_or_default();
        settings.default_voice_id = text(window, DEFAULT_VOICE_ID).trim().to_owned();
        let key = text(window, API_KEY).trim().to_owned();
        settings.elevenlabs_api_key = if key.is_empty() { None } else { Some(key) };
        settings
    }

    unsafe fn save(window: HWND, form: &mut Form) -> Result<(), String> {
        capture_current_draft(window, form);
        let mut settings = read_speech_settings(window, form);
        settings.quiet_mode = checked(window, QUIET);
        settings.schedule_enabled = checked(window, SCHEDULE);
        settings.quiet_start = parse_time(&text(window, START), false)?;
        settings.quiet_end = parse_time(&text(window, END), true)?;
        for id in &form.removed_characters {
            settings.characters.remove(id);
            if settings.selected_character.as_deref() == Some(id.as_str()) { settings.selected_character = None; }
        }
        for (id, draft) in &form.drafts {
            let candidate = draft.clone();
            if draft.name.trim().is_empty() && !form.settings.characters.contains_key(id) && form.selected_character.as_deref() != Some(id) {
                continue;
            }
            validate_registry(&BTreeMap::from([(id.clone(), candidate.clone())]), None)?;
            settings.characters.insert(id.clone(), candidate);
        }
        settings.selected_character = form.selected_character.clone().filter(|id| settings.characters.contains_key(id));
        settings.save(&form.data)?;
        form.settings = settings;
        label(window, API_KEY_HINT, &api_key_status(form.settings.elevenlabs_api_key.as_deref()));
        form.removed_characters.clear();
        label(window, STATUS, "Saved. Changes apply to the next announcement.");
        refresh_characters(window, form);
        Ok(())
    }

    fn api_key_status(api_key: Option<&str>) -> String {
        match api_key.filter(|key| !key.trim().is_empty()).map(crate::elevenlabs::validate_api_key) {
            Some(Ok(())) => "Using the key entered in Settings. Install Offline voice for local fallback.".into(),
            None => "Enter an ElevenLabs key, or install local speech in Offline voice.".into(),
            Some(Err(error)) => format!("ElevenLabs API key unavailable: {error}"),
        }
    }

    fn initial_voice_usage_state(api_key: Option<&str>) -> VoiceUsageState {
        if api_key.is_some_and(|key| !key.trim().is_empty()) { VoiceUsageState::NotLoaded } else { VoiceUsageState::NoKey }
    }

    fn voice_usage_text(state: &VoiceUsageState) -> String {
        match state {
            VoiceUsageState::NoKey => "Enter an ElevenLabs key to load voice usage.".into(),
            VoiceUsageState::NotLoaded => "Refresh to load ElevenLabs voice usage.".into(),
            VoiceUsageState::Loading(_) => "Loading ElevenLabs voice usage…".into(),
            VoiceUsageState::Failed(error) => format!("Voice usage unavailable: {error}"),
            VoiceUsageState::Ready(usage) => {
                let slots = format!("Voice slots: {} of {} used; {} remaining.", usage.voice_slots_used, usage.voice_limit, usage.remaining_voice_slots());
                let edits = match usage.voice_add_edit_allowance() {
                    Some((limit, remaining)) => format!("Voice additions and edits: {} of {} used; {} remaining this billing period.", usage.voice_add_edit_counter, limit, remaining),
                    None => format!("Voice additions and edits: {} used; limit unknown; remaining unknown.", usage.voice_add_edit_counter),
                };
                format!("{slots}\r\n{edits}")
            }
        }
    }

    unsafe fn render_voice_usage(window: HWND, form: &Form) {
        label(window, VOICE_USAGE, &voice_usage_text(&form.voice_usage));
        let has_key = !text(window, API_KEY).trim().is_empty();
        let loading = matches!(form.voice_usage, VoiceUsageState::Loading(_));
        EnableWindow(GetDlgItem(window, USAGE_REFRESH), (has_key && !loading) as i32);
    }

    unsafe fn clear_voice_usage(window: HWND, form: &mut Form) {
        KillTimer(window, 4);
        form.voice_usage = initial_voice_usage_state(Some(text(window, API_KEY).trim()));
        render_voice_usage(window, form);
    }

    unsafe fn refresh_voice_usage(window: HWND, form: &mut Form, force: bool) {
        let key = text(window, API_KEY).trim().to_owned();
        if key.is_empty() {
            KillTimer(window, 4);
            form.voice_usage = VoiceUsageState::NoKey;
            render_voice_usage(window, form);
            return;
        }
        if !force && matches!(form.voice_usage, VoiceUsageState::Loading(_)) { return; }
        if let Err(error) = crate::elevenlabs::validate_api_key(&key) {
            KillTimer(window, 4);
            form.voice_usage = VoiceUsageState::Failed(error);
            render_voice_usage(window, form);
            return;
        }
        let mut settings = read_speech_settings(window, form);
        settings.elevenlabs_api_key = Some(key);
        let (sender, receiver) = mpsc::channel();
        form.voice_usage = VoiceUsageState::Loading(receiver);
        render_voice_usage(window, form);
        SetTimer(window, 4, 100, None);
        std::thread::spawn(move || {
            let result: Result<VoiceUsage, String> = (|| {
                let client = Client::from_settings(&settings)?.ok_or_else(|| "Enter an ElevenLabs key to load voice usage.".to_string())?;
                client.voice_usage()
            })();
            let _ = sender.send(result);
        });
    }

    unsafe fn poll_voice_usage(window: HWND, form: &mut Form) {
        let result = match &form.voice_usage {
            VoiceUsageState::Loading(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => Err("Voice usage request stopped unexpectedly.".into()),
            },
            _ => return,
        };
        KillTimer(window, 4);
        form.voice_usage = match result {
            Ok(usage) => VoiceUsageState::Ready(usage),
            Err(error) => VoiceUsageState::Failed(error),
        };
        render_voice_usage(window, form);
    }

    unsafe fn render_offline_voice(window: HWND, form: &Form) {
        let installing = form.offline_install.is_some();
        let installed = crate::tts::installed();
        label(window, OFFLINE_STATUS, if installing {
            "Installing… Downloading and checking the voice engine and model."
        } else if installed {
            "Installed. Kitten CPU speech is available offline."
        } else {
            "Not installed. Download the voice engine and model to enable offline speech."
        });
        label(window, OFFLINE_INSTALL, if installed { "Installed" } else { "Install" });
        EnableWindow(GetDlgItem(window, OFFLINE_INSTALL), (!installing && !installed) as i32);
        EnableWindow(GetDlgItem(window, CLOSE), (!installing) as i32);
    }

    unsafe fn install_offline_voice(window: HWND, form: &mut Form) {
        if form.offline_install.is_some() { return; }
        let (sender, receiver) = mpsc::channel();
        form.offline_install = Some(receiver);
        render_offline_voice(window, form);
        SetTimer(window, 5, 100, None);
        std::thread::spawn(move || { let _ = sender.send(crate::tts::install().and_then(|_| crate::tts::prepare())); });
    }

    unsafe fn poll_offline_voice(window: HWND, form: &mut Form) {
        let Some(receiver) = &form.offline_install else { return; };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Offline voice installation stopped unexpectedly.".into()),
        };
        KillTimer(window, 5);
        form.offline_install = None;
        render_offline_voice(window, form);
        if let Err(error) = result {
            label(window, OFFLINE_STATUS, &format!("Installation failed: {error}"));
            label(window, OFFLINE_INSTALL, "Retry install");
            EnableWindow(GetDlgItem(window, OFFLINE_INSTALL), 1);
        }
    }

    unsafe fn browse_video(window: HWND, form: &mut Form) {
        let mut file = vec![0u16; 32768];
        let filter = wide("MP4 video\0*.mp4\0All files\0*.*\0");
        let title = wide("Choose character animation");
        let mut dialog = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: window,
            lpstrFilter: filter.as_ptr(),
            lpstrFile: file.as_mut_ptr(),
            nMaxFile: file.len() as u32,
            lpstrTitle: title.as_ptr(),
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..OPENFILENAMEW::default()
        };
        if GetOpenFileNameW(&mut dialog) == 0 { return; }
        let length = file.iter().position(|value| *value == 0).unwrap_or(file.len());
        let path = PathBuf::from(String::from_utf16_lossy(&file[..length]));
        if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")) {
            label(window, STATUS, "Choose an MP4 animation file.");
            return;
        }
        SetWindowTextW(GetDlgItem(window, VIDEO_PATH), wide(&path.to_string_lossy()).as_ptr());
        capture_current_draft(window, form);
    }

    unsafe fn start_voice_preview(window: HWND, form: &mut Form) {
        if form.voice_preview.take().is_some() {
            label(window, PLAY_VOICE, "Play voice example");
            label(window, STATUS, "Voice preview stopped.");
            KillTimer(window, 3);
            return;
        }
        capture_current_draft(window, form);
        if form.preview.take().is_some() {
            label(window, PREVIEW, "Play example");
            KillTimer(window, 1);
        }
        let settings = read_speech_settings(window, form);
        play_character_example(window, form, settings);
    }

    unsafe fn play_character_example(window: HWND, form: &mut Form, mut settings: Settings) {
        if settings.volume == 0 { label(window, STATUS, "Voice preview is silent at 0% volume."); return; }
        let Some(id) = form.active_draft.clone() else { return; };
        let character = draft_for(form, &id).clone();
        if let CharacterVoice::ElevenLabs { voice_id } = &character.voice {
            if let Err(error) = crate::characters::validate_voice_id(voice_id) {
                label(window, STATUS, &error);
                return;
            }
        }
        settings.characters.insert(id.clone(), character);
        settings.selected_character = Some(id);
        form.voice_preview = Some(crate::platform::Preview::voice(settings, VOICE_EXAMPLE.into(), form.assets.clone()));
        label(window, PLAY_VOICE, "Stop example");
        label(window, STATUS, "Playing voice example.");
        SetTimer(window, 3, 100, None);
    }

    unsafe fn stop_previews(window: HWND, form: &mut Form) {
        form.voice_preview = None;
        form.preview = None;
        KillTimer(window, 1);
        KillTimer(window, 3);
        label(window, PLAY_VOICE, "Play voice example");
        label(window, PREVIEW, "Play example");
    }

    unsafe fn layout(window: HWND, width: i32, height: i32) {
        if width <= 0 || height <= 0 { return; }
        let sidebar_width = 220;
        let main_left = sidebar_width + 32;
        let main_right = width - 32;
        let main_width = main_right - main_left;
        let footer_y = height - 112;
        if main_width <= 0 || footer_y <= 120 { return; }
        let move_control = |id: i32, x: i32, y: i32, w: i32, h: i32| {
            SetWindowPos(GetDlgItem(window, id), std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        };
        move_control(SIDEBAR, 24, 30, sidebar_width - 24, PAGE_SPECS.len() as i32 * 38);
        move_control(PAGE_TITLE, main_left, 22, main_width, 42);
        move_control(PAGE_HINT, main_left, 70, main_width, 32);

        let character_list_y = 126;
        let character_list_height = footer_y - character_list_y - 48;
        let character_editor_x = main_left + 250;
        let character_editor_width = main_right - character_editor_x;
        move_control(CHARACTER_LIST, main_left, character_list_y, 220, character_list_height);
        move_control(NEW_CHARACTER, main_left, character_list_y + character_list_height + 12, 220, 34);
        move_control(CHARACTER_EMPTY, character_editor_x, character_list_y + 26, character_editor_width, 110);
        move_control(NAME_LABEL, character_editor_x, character_list_y, character_editor_width, 24);
        move_control(CHARACTER_NAME, character_editor_x, character_list_y + 28, character_editor_width, 34);
        move_control(VIDEO_LABEL, character_editor_x, character_list_y + 78, character_editor_width, 24);
        move_control(VIDEO_PATH, character_editor_x, character_list_y + 106, character_editor_width, 36);
        let character_actions_y = character_list_y + 236;
        move_control(VOICE_ID_LABEL, character_editor_x, character_list_y + 158, character_editor_width, 24);
        move_control(VOICE_ID, character_editor_x, character_list_y + 186, character_editor_width, 34);
        move_control(PLAY_VOICE, character_editor_x, character_actions_y, 160, 34);
        move_control(REMOVE_CHARACTER, main_right - 92, character_actions_y, 92, 34);

        move_control(VOLUME_LABEL, main_left, 128, main_width, 24);
        move_control(VOLUME, main_left, 158, main_width, 42);
        move_control(OUTPUT_LABEL, main_left, 220, main_width, 24);
        move_control(OUTPUT, main_left, 250, main_width - 104, 220);
        move_control(REFRESH, main_right - 90, 250, 90, 34);
        move_control(PREVIEW, main_left, 324, 150, 34);
        move_control(PREVIEW_HINT, main_left + 174, 326, main_width - 174, 48);

        move_control(QUIET, main_left, 128, main_width, 32);
        move_control(QUIET_HINT, main_left, 168, main_width, 28);
        move_control(SCHEDULE, main_left, 214, main_width, 32);
        move_control(START_LABEL, main_left, 260, 48, 24);
        move_control(START, main_left + 52, 256, 84, 34);
        move_control(END_LABEL, main_left + 158, 260, 24, 24);
        move_control(END, main_left + 190, 256, 84, 34);
        move_control(TIME_HINT, main_left + 304, 260, main_width - 304, 24);

        let speech_gap = 24;
        let speech_column_width = (main_width - speech_gap) / 2;
        let default_voice_x = main_left + speech_column_width + speech_gap;
        move_control(MODEL_LABEL, main_left, 128, speech_column_width, 24);
        move_control(MODEL, main_left, 158, speech_column_width, 220);
        move_control(DEFAULT_VOICE_ID_LABEL, default_voice_x, 128, main_right - default_voice_x, 24);
        move_control(DEFAULT_VOICE_ID, default_voice_x, 158, main_right - default_voice_x, 34);
        move_control(DEFAULT_VOICE_ID_HINT, main_left, 198, main_width, 24);
        move_control(API_KEY_LABEL, main_left, 230, main_width, 24);
        move_control(API_KEY, main_left, 258, main_width, 34);
        move_control(API_KEY_HINT, main_left, 300, main_width, 54);
        let usage_y = 372;
        let usage_button_y = footer_y - 42;
        let usage_height = (usage_button_y - usage_y - 10).max(40);
        move_control(VOICE_USAGE, main_left, usage_y, main_width, usage_height);
        move_control(USAGE_REFRESH, main_right - 120, usage_button_y, 120, 34);
        move_control(MY_VOICES, main_left, usage_button_y, 180, 34);
        move_control(OFFLINE_STATUS, main_left, 128, main_width, 160);
        move_control(OFFLINE_INSTALL, main_left, 308, 150, 34);

        move_control(STATUS, 24, height - 96, width - 268, 80);
        move_control(APPLY, width - 228, height - 48, 100, 32);
        move_control(CLOSE, width - 116, height - 48, 100, 32);
    }

    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let form = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Form;
        match message {
            WM_COPYDATA if lparam != 0 => {
                let request = &*(lparam as *const DesktopRequest);
                if request.kind == SHOW_ON_DESKTOP && request.size as usize == std::mem::size_of::<windows::core::GUID>() && !request.desktop.is_null() {
                    let desktop = std::ptr::read_unaligned(request.desktop);
                    show_on_desktop(window, Some(&desktop));
                    return 1;
                }
                0
            }
            WM_SIZE if !form.is_null() => {
                layout(window, (lparam as u32 & 0xffff) as i32, ((lparam as u32 >> 16) & 0xffff) as i32);
                0
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam as *mut MINMAXINFO);
                let mut bounds = RECT { left: 0, top: 0, right: 860, bottom: 640 };
                AdjustWindowRectEx(&mut bounds, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_THICKFRAME | WS_MAXIMIZEBOX, 0, WS_EX_CONTROLPARENT);
                info.ptMinTrackSize.x = bounds.right - bounds.left;
                info.ptMinTrackSize.y = bounds.bottom - bounds.top;
                0
            }
            WM_CTLCOLORSTATIC if !form.is_null() => {
                let dc = wparam as HDC;
                let control = lparam as HWND;
                SetBkMode(dc, 1);
                if [PAGE_HINT, QUIET_HINT, TIME_HINT, PREVIEW_HINT, API_KEY_HINT, DEFAULT_VOICE_ID_HINT, VOICE_USAGE, CHARACTER_EMPTY, STATUS].contains(&GetDlgCtrlID(control)) {
                    SetTextColor(dc, color(92, 101, 112));
                } else {
                    SetTextColor(dc, color(32, 37, 43));
                }
                (*form).ui.main_brush as isize
            }
            WM_CTLCOLORLISTBOX if !form.is_null() => {
                let dc = wparam as HDC;
                let control = lparam as HWND;
                SetBkMode(dc, 1);
                if GetDlgCtrlID(control) == SIDEBAR {
                    SetBkColor(dc, color(246, 248, 251));
                    (*form).ui.sidebar_brush as isize
                } else {
                    SetBkColor(dc, color(255, 255, 255));
                    (*form).ui.main_brush as isize
                }
            }
            WM_DRAWITEM if !form.is_null() && wparam == SIDEBAR as usize && lparam != 0 => {
                let item = &*(lparam as *const DRAWITEMSTRUCT);
                let selected = item.itemState & ODS_SELECTED != 0;
                let background = if selected { color(229, 239, 252) } else { color(246, 248, 251) };
                SetDCBrushColor(item.hDC, background);
                FillRect(item.hDC, &item.rcItem, GetStockObject(DC_BRUSH) as HBRUSH);
                if let Some(spec) = PAGE_SPECS.get(item.itemID as usize) {
                    SetBkMode(item.hDC, 1);
                    SetTextColor(item.hDC, if selected { color(25, 80, 160) } else { color(32, 37, 43) });
                    let previous = SelectObject(item.hDC, (*form).ui.body as HGDIOBJ);
                    let mut bounds = item.rcItem;
                    bounds.left += 10;
                    bounds.right -= 10;
                    let title = wide(spec.label);
                    DrawTextW(item.hDC, title.as_ptr(), title.len() as i32 - 1, &mut bounds, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    SelectObject(item.hDC, previous);
                    if item.itemState & ODS_FOCUS != 0 { DrawFocusRect(item.hDC, &item.rcItem); }
                }
                1
            }
            WM_COMMAND if !form.is_null() => {
                let id = (wparam & 0xffff) as i32;
                let code = ((wparam >> 16) & 0xffff) as u32;
                match id {
                    API_KEY if code == EN_CHANGE => {
                        let status = api_key_status(Some(text(window, API_KEY).trim()));
                        label(window, API_KEY_HINT, &status);
                        clear_voice_usage(window, &mut *form);
                    }
                    APPLY => match save(window, &mut *form) {
                        Ok(()) => {}
                        Err(error) => { MessageBoxW(window, wide(&error).as_ptr(), wide("Civilized Agent settings").as_ptr(), MB_OK | MB_ICONERROR); }
                    },
                    CLOSE => { DestroyWindow(window); }
                    SIDEBAR if code == LBN_SELCHANGE => {
                        let index = SendMessageW(GetDlgItem(window, SIDEBAR), LB_GETCURSEL, 0, 0);
                        if let Some(page) = page_from_index(index) { select_page(window, &mut *form, page); }
                    }
                    REFRESH => {
                        let selected = selected_output(window, &*form);
                        populate_outputs(window, &mut *form, selected.as_deref());
                    }
                    USAGE_REFRESH => refresh_voice_usage(window, &mut *form, false),
                    OFFLINE_INSTALL => install_offline_voice(window, &mut *form),
                    MY_VOICES => {
                        let result = windows_sys::Win32::UI::Shell::ShellExecuteW(
                            window,
                            wide("open").as_ptr(),
                            wide("https://elevenlabs.io/app/voice-lab").as_ptr(),
                            std::ptr::null(),
                            std::ptr::null(),
                            SW_SHOWNORMAL,
                        );
                        if result as isize <= 32 {
                            label(window, STATUS, "Could not open ElevenLabs My Voices in your browser.");
                        }
                    }
                    PREVIEW => {
                        if (*form).voice_preview.take().is_some() {
                            KillTimer(window, 3);
                            label(window, PLAY_VOICE, "Play voice example");
                        }
                        if (*form).preview.take().is_some() {
                            label(window, PREVIEW, "Play example");
                            label(window, STATUS, "Preview stopped.");
                            KillTimer(window, 1);
                        } else {
                            let settings = read_speech_settings(window, &*form);
                            if settings.volume == 0 {
                                label(window, STATUS, "Preview is silent at 0% volume.");
                            } else {
                                (*form).preview = Some(crate::platform::Preview::start((*form).data.clone(), settings, (*form).assets.clone()));
                                label(window, PREVIEW, "Stop example");
                                label(window, STATUS, "Playing static, then: This is an announcement");
                                SetTimer(window, 1, 100, None);
                            }
                        }
                    }
                    SCHEDULE => {
                        let enabled = checked(window, SCHEDULE);
                        EnableWindow(GetDlgItem(window, START), enabled as i32);
                        EnableWindow(GetDlgItem(window, END), enabled as i32);
                    }
                    CHARACTER_LIST if code == LBN_SELCHANGE => {
                        let index = SendMessageW(GetDlgItem(window, CHARACTER_LIST), LB_GETCURSEL, 0, 0);
                        if index >= 0 { select_character(window, &mut *form, index as usize); }
                    }
                    NEW_CHARACTER => {
                        capture_current_draft(window, &mut *form);
                        stop_previews(window, &mut *form);
                        let id = new_character_id(&*form);
                        (*form).drafts.insert(id.clone(), empty_draft());
                        (*form).selected_character = Some(id.clone());
                        (*form).active_draft = Some(id);
                        refresh_characters(window, &mut *form);
                        label(window, STATUS, "New character draft. Apply saves it; Close discards it.");
                    }
                    VIDEO_PATH => browse_video(window, &mut *form),
                    PLAY_VOICE => start_voice_preview(window, &mut *form),
                    REMOVE_CHARACTER => {
                        if let Some(id) = (*form).active_draft.clone() {
                            let name = draft_for(&mut *form, &id).name.clone();
                            let name = if name.trim().is_empty() { "this character" } else { &name };
                            if MessageBoxW(window, wide(&format!("Delete {name}?")).as_ptr(), wide("Delete character").as_ptr(), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) != IDYES { return 0; }
                            stop_previews(window, &mut *form);
                            if (*form).selected_character.as_deref() == Some(id.as_str()) { (*form).selected_character = None; }
                            (*form).drafts.remove(&id);
                            if (*form).settings.characters.contains_key(&id) { (*form).removed_characters.insert(id.clone()); }
                            (*form).active_draft = None;
                            refresh_characters(window, &mut *form);
                            label(window, STATUS, "Character marked for removal. Apply saves it; cloud voices are unchanged.");
                        }
                    }
                    CHARACTER_NAME | VOICE_ID if code == EN_CHANGE => capture_current_draft(window, &mut *form),
                    _ => {}
                }
                0
            }
            WM_HSCROLL => {
                let volume = SendMessageW(GetDlgItem(window, VOLUME), TBM_GETPOS, 0, 0);
                label(window, VOLUME_LABEL, &volume_label(volume as u16));
                0
            }
            WM_TIMER if !form.is_null() && wparam == 1 => {
                if let Some(result) = (*form).preview.as_mut().and_then(crate::platform::Preview::finished) {
                    (*form).preview = None;
                    KillTimer(window, 1);
                    label(window, PREVIEW, "Play example");
                    match result {
                        Ok(()) => label(window, STATUS, "Preview finished."),
                        Err(error) => label(window, STATUS, &format!("Preview failed: {error}")),
                    }
                }
                0
            }
            WM_TIMER if !form.is_null() && wparam == 3 => {
                if let Some(result) = (*form).voice_preview.as_mut().and_then(crate::platform::Preview::finished) {
                    (*form).voice_preview = None;
                    KillTimer(window, 3);
                    label(window, PLAY_VOICE, "Play voice example");
                    match result {
                        Ok(()) => label(window, STATUS, "Voice preview finished."),
                        Err(error) => label(window, STATUS, &format!("Voice preview failed: {error}")),
                    }
                }
                0
            }
            WM_TIMER if !form.is_null() && wparam == 4 => {
                poll_voice_usage(window, &mut *form);
                0
            }
            WM_TIMER if !form.is_null() && wparam == 5 => {
                poll_offline_voice(window, &mut *form);
                0
            }
            WM_CLOSE if !form.is_null() && (*form).offline_install.is_some() => {
                label(window, STATUS, "Wait for offline voice installation to finish before closing.");
                0
            }
            WM_CLOSE => { DestroyWindow(window); 0 }
            WM_DESTROY => { if !form.is_null() { stop_previews(window, &mut *form); } KillTimer(window, 4); PostQuitMessage(0); 0 }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }

    fn with_desktops<T>(action: impl FnOnce(&windows::Win32::UI::Shell::IVirtualDesktopManager) -> windows::core::Result<T>) -> windows::core::Result<T> {
        use windows::Win32::System::Com::*;
        use windows::Win32::UI::Shell::*;
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let result = (|| {
                let desktops: IVirtualDesktopManager = CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL)?;
                action(&desktops)
            })();
            CoUninitialize();
            result
        }
    }

    unsafe fn show_on_desktop(window: HWND, desktop: Option<&windows::core::GUID>) {
        if let Some(desktop) = desktop {
            let _ = with_desktops(|desktops| desktops.MoveWindowToDesktop(windows::Win32::Foundation::HWND(window), desktop));
        }
        ShowWindow(window, SW_RESTORE);
        SetWindowPos(window, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetForegroundWindow(window);
    }

    pub fn run(data: &Path, assets: &Path) -> Result<(), String> {
        let settings = Settings::load(data)?;
        let voice_usage = initial_voice_usage_state(settings.elevenlabs_api_key.as_deref());
        let mut form = Box::new(Form {
            data: data.into(),
            assets: assets.into(),
            selected_character: settings.selected_character.clone(),
            settings,
            devices: Vec::new(),
            missing: None,
            preview: None,
            voice_preview: None,
            drafts: BTreeMap::new(),
            active_draft: None,
            character_ids: Vec::new(),
            voice_usage,
            offline_install: None,
            removed_characters: std::collections::BTreeSet::new(),
            updating: false,
            active_page: Page::Audio,
            ui: unsafe { UiResources::new()? },
        });
        unsafe {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            std::fs::canonicalize(data).unwrap_or_else(|_| data.into()).to_string_lossy().to_lowercase().hash(&mut hash);
            let class = wide(&format!("CivilizedAgentSettings-{:x}", hash.finish()));
            let foreground = GetForegroundWindow();
            let desktop = with_desktops(|desktops| desktops.GetWindowDesktopId(windows::Win32::Foundation::HWND(foreground))).ok();
            let existing = FindWindowW(class.as_ptr(), std::ptr::null());
            if !existing.is_null() {
                let mut process = 0;
                GetWindowThreadProcessId(existing, &mut process);
                AllowSetForegroundWindow(process);
                if let Some(desktop) = desktop.as_ref() {
                    let request = DesktopRequest { kind: SHOW_ON_DESKTOP, size: std::mem::size_of::<windows::core::GUID>() as u32, desktop };
                    let mut result = 0;
                    SendMessageTimeoutW(existing, WM_COPYDATA, 0, &request as *const _ as isize, SMTO_ABORTIFHUNG, 2000, &mut result);
                }
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
                return Ok(());
            }
            InitCommonControlsEx(&INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES });
            let instance = GetModuleHandleW(std::ptr::null());
            let window_class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: instance, lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW), hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH, ..WNDCLASSW::default() };
            if RegisterClassW(&window_class) == 0 { return Err(std::io::Error::last_os_error().to_string()); }
            let window = CreateWindowExW(WS_EX_CONTROLPARENT, class.as_ptr(), wide("Civilized Agent settings").as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_THICKFRAME | WS_MAXIMIZEBOX, CW_USEDEFAULT, CW_USEDEFAULT, 960, 720,
                std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
            if window.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
            SetWindowLongPtrW(window, GWLP_USERDATA, &mut *form as *mut Form as isize);
            let creation = (|| -> Result<(), String> {
                control(window, "LISTBOX", "Settings", SIDEBAR, WS_TABSTOP | LBS_NOTIFY as u32 | LBS_HASSTRINGS as u32 | LBS_OWNERDRAWFIXED as u32 | LBS_NOINTEGRALHEIGHT as u32, (24, 30, 196, 152))?;
                control(window, "STATIC", "", PAGE_TITLE, 0, (252, 22, 676, 42))?;
                control(window, "STATIC", "", PAGE_HINT, 0, (252, 70, 676, 32))?;

                control(window, "BUTTON", "Quiet mode (mute speech and static)", QUIET, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (252, 128, 676, 32))?;
                control(window, "STATIC", "Announcements still appear while quiet mode is on.", QUIET_HINT, 0, (252, 168, 676, 28))?;
                control(window, "BUTTON", "Quiet mode on a daily schedule", SCHEDULE, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (252, 214, 676, 32))?;
                control(window, "STATIC", "From", START_LABEL, 0, (252, 260, 48, 24))?;
                control(window, "EDIT", &format_time(form.settings.quiet_start), START, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (304, 256, 84, 34))?;
                control(window, "STATIC", "to", END_LABEL, 0, (410, 260, 24, 24))?;
                control(window, "EDIT", &format_time(form.settings.quiet_end), END, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (442, 256, 84, 34))?;
                control(window, "STATIC", "Local time, HH:MM", TIME_HINT, 0, (556, 260, 300, 24))?;

                control(window, "STATIC", &volume_label(form.settings.volume), VOLUME_LABEL, 0, (252, 128, 676, 24))?;
                control(window, "msctls_trackbar32", "Announcer volume", VOLUME, WS_TABSTOP | TBS_NOTICKS, (252, 158, 676, 42))?;
                SendMessageW(GetDlgItem(window, VOLUME), TBM_SETRANGEMAX, 0, 100);
                SendMessageW(GetDlgItem(window, VOLUME), TBM_SETPOS, 1, form.settings.volume as isize);
                control(window, "STATIC", "Audio output (speech and static)", OUTPUT_LABEL, 0, (252, 220, 676, 24))?;
                control(window, "COMBOBOX", "Audio output", OUTPUT, WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, (252, 250, 572, 220))?;
                control(window, "BUTTON", "Refresh", REFRESH, WS_TABSTOP, (838, 250, 90, 34))?;
                control(window, "BUTTON", "Play example", PREVIEW, WS_TABSTOP, (252, 324, 150, 34))?;
                control(window, "STATIC", "Previews your selected settings without saving.", PREVIEW_HINT, 0, (426, 326, 502, 48))?;

                control(window, "STATIC", "Speech model", MODEL_LABEL, 0, (252, 128, 318, 24))?;
                control(window, "COMBOBOX", "Speech model", MODEL, WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, (252, 158, 318, 220))?;
                for model in SpeechModel::ALL {
                    SendMessageW(GetDlgItem(window, MODEL), CB_ADDSTRING, 0, wide(model.label()).as_ptr() as isize);
                }
                let selected = SpeechModel::ALL.iter().position(|model| *model == form.settings.speech_model).unwrap_or(0);
                SendMessageW(GetDlgItem(window, MODEL), CB_SETCURSEL, selected, 0);
                control(window, "STATIC", "Default voice ID", DEFAULT_VOICE_ID_LABEL, 0, (594, 128, 318, 24))?;
                control(window, "EDIT", &form.settings.default_voice_id, DEFAULT_VOICE_ID, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (594, 158, 318, 34))?;
                SendMessageW(GetDlgItem(window, DEFAULT_VOICE_ID), EM_SETLIMITTEXT, 256, 0);
                control(window, "STATIC", "Characters without a custom ElevenLabs voice use this ID.", DEFAULT_VOICE_ID_HINT, 0, (252, 198, 660, 24))?;
                control(window, "STATIC", "ElevenLabs key", API_KEY_LABEL, 0, (252, 230, 660, 24))?;
                control(window, "EDIT", form.settings.elevenlabs_api_key.as_deref().unwrap_or_default(), API_KEY, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32, (252, 258, 660, 34))?;
                SendMessageW(GetDlgItem(window, API_KEY), EM_SETLIMITTEXT, 4096, 0);
                control(window, "STATIC", &api_key_status(form.settings.elevenlabs_api_key.as_deref()), API_KEY_HINT, 0, (252, 300, 660, 54))?;
                control(window, "STATIC", &voice_usage_text(&form.voice_usage), VOICE_USAGE, 0, (252, 372, 660, 128))?;
                control(window, "BUTTON", "Refresh", USAGE_REFRESH, WS_TABSTOP, (808, 526, 120, 34))?;
                control(window, "BUTTON", "Open My Voices", MY_VOICES, WS_TABSTOP, (252, 526, 180, 34))?;
                control(window, "STATIC", "", OFFLINE_STATUS, 0, (252, 128, 676, 160))?;
                control(window, "BUTTON", "Install", OFFLINE_INSTALL, WS_TABSTOP, (252, 308, 150, 34))?;

                control(window, "LISTBOX", "Characters", CHARACTER_LIST, WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY as u32 | LBS_HASSTRINGS as u32 | LBS_NOINTEGRALHEIGHT as u32, (252, 126, 220, 420))?;
                control(window, "BUTTON", "New", NEW_CHARACTER, WS_TABSTOP, (252, 558, 220, 34))?;
                control(window, "STATIC", "Select a character or choose New to create one.", CHARACTER_EMPTY, 0, (502, 152, 426, 110))?;
                control(window, "STATIC", "Name", NAME_LABEL, 0, (502, 126, 426, 24))?;
                control(window, "EDIT", "", CHARACTER_NAME, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (502, 154, 426, 34))?;
                control(window, "STATIC", "Video", VIDEO_LABEL, 0, (502, 204, 426, 24))?;
                control(window, "BUTTON", "Choose video…", VIDEO_PATH, WS_TABSTOP | BS_LEFT as u32, (502, 232, 426, 36))?;
                control(window, "STATIC", "ElevenLabs voice ID", VOICE_ID_LABEL, 0, (502, 284, 426, 24))?;
                control(window, "EDIT", "", VOICE_ID, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (502, 312, 426, 34))?;
                control(window, "BUTTON", "Play voice example", PLAY_VOICE, WS_TABSTOP, (502, 362, 160, 34))?;
                control(window, "BUTTON", "Delete", REMOVE_CHARACTER, WS_TABSTOP, (836, 362, 92, 34))?;

                control(window, "EDIT", "Apply saves changes. Close discards unsaved edits.", STATUS, ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL | WS_TABSTOP, (24, 624, 692, 80))?;
                control(window, "BUTTON", "Apply", APPLY, WS_TABSTOP | BS_DEFPUSHBUTTON as u32, (732, 672, 100, 32))?;
                control(window, "BUTTON", "Close", CLOSE, WS_TABSTOP, (844, 672, 100, 32))?;
                Ok(())
            })();
            if let Err(error) = creation {
                DestroyWindow(window);
                return Err(error);
            }
            apply_fonts(window, &form);
            populate_pages(window, &form);
            set_checked(window, QUIET, form.settings.quiet_mode);
            set_checked(window, SCHEDULE, form.settings.schedule_enabled);
            EnableWindow(GetDlgItem(window, START), form.settings.schedule_enabled as i32);
            EnableWindow(GetDlgItem(window, END), form.settings.schedule_enabled as i32);
            let selected = form.settings.output_device.clone();
            populate_outputs(window, &mut form, selected.as_deref());
            refresh_characters(window, &mut form);
            refresh_page_visibility(window, &form);
            render_voice_usage(window, &form);
            render_offline_voice(window, &form);
            let mut client = RECT::default();
            GetClientRect(window, &mut client);
            layout(window, client.right, client.bottom);
            show_on_desktop(window, desktop.as_ref());
            let speech_data = data.to_path_buf();
            std::thread::spawn(move || {
                if let Err(error) = crate::tts::prepare() { crate::state::log(&speech_data, error); }
            });
            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
                if result == 0 { break; }
                if result == -1 { return Err(std::io::Error::last_os_error().to_string()); }
                if IsDialogMessageW(window, &message) == 0 { TranslateMessage(&message); DispatchMessageW(&message); }
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use native::run;
