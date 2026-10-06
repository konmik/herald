#[cfg(not(target_os = "windows"))]
pub fn run(_data: &std::path::Path, _assets: &std::path::Path) -> Result<(), String> {
    Err("The settings app is currently available on Windows.".into())
}

#[cfg(target_os = "windows")]
mod native {
    use crate::audio::OutputDevice;
    use crate::characters::{validate_registry, Character, CharacterVoice};
    use crate::elevenlabs::{Client, SpeechModel};
    use crate::settings::{format_time, parse_time, Settings};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::hash::{Hash, Hasher};
    use std::sync::mpsc::{self, Receiver};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::{Controls::*, WindowsAndMessaging::*};
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OFN_NOCHANGEDIR};
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
    const TAB: i32 = 200;
    const CHARACTER_LIST: i32 = 201;
    const NEW_CHARACTER: i32 = 202;
    const CHARACTER_NAME: i32 = 203;
    const VOICE_PROMPT: i32 = 204;
    const VIDEO_PATH: i32 = 206;
    const PLAY_VOICE: i32 = 210;
    const REMOVE_CHARACTER: i32 = 214;
    const QUIET_HINT: i32 = 301;
    const START_LABEL: i32 = 303;
    const END_LABEL: i32 = 304;
    const TIME_HINT: i32 = 305;
    const OUTPUT_LABEL: i32 = 306;
    const MODEL_LABEL: i32 = 307;
    const PREVIEW_HINT: i32 = 308;
    const NAME_LABEL: i32 = 310;
    const PROMPT_LABEL: i32 = 311;
    const VIDEO_LABEL: i32 = 313;
    const SHOW_ON_DESKTOP: usize = 0x43415354;
    const VOICE_EXAMPLE: &str = "I bring news for your attention. Listen as I deliver this announcement. Your work is ready, and every check has passed.";

    #[repr(C)]
    struct DesktopRequest {
        kind: usize,
        size: u32,
        desktop: *const windows::core::GUID,
    }

    struct VoiceJob {
        id: String,
        receiver: Receiver<Result<String, String>>,
        cancelled: Arc<AtomicBool>,
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
        voice_job: Option<VoiceJob>,
        updating: bool,
    }

    fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }

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

    fn empty_draft() -> Character {
        Character::default()
    }

    unsafe fn draft_for<'a>(form: &'a mut Form, id: &str) -> &'a mut Character {
        if !form.drafts.contains_key(id) {
            let draft = form.settings.characters.get(id).cloned().unwrap_or_else(empty_draft);
            form.drafts.insert(id.to_owned(), draft);
        }
        form.drafts.get_mut(id).unwrap()
    }

    unsafe fn capture_current_draft(window: HWND, form: &mut Form) {
        if form.updating || form.voice_job.is_some() { return; }
        let Some(id) = form.active_draft.clone() else { return; };
        let name = text(window, CHARACTER_NAME);
        let voice_description = text(window, VOICE_PROMPT);
        let video = text(window, VIDEO_PATH);
        let assets = form.assets.clone();
        let draft = draft_for(form, &id);
        draft.name = name;
        if draft.voice_description != voice_description {
            draft.voice = CharacterVoice::default();
            draft.voice_description = voice_description;
        }
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

    unsafe fn set_draft_controls(window: HWND, form: &mut Form) {
        form.updating = true;
        if let Some(id) = form.active_draft.clone() {
            let assets = form.assets.clone();
            let draft = draft_for(form, &id);
            label(window, CHARACTER_NAME, &draft.name);
            label(window, VOICE_PROMPT, &draft.voice_description);
            label(window, VIDEO_PATH, &draft.animation_path.as_ref().map_or_else(|| "Choose video…".into(), |path| crate::characters::animation_path(&id, path, &assets).to_string_lossy().into_owned()));
        } else {
            for id in [CHARACTER_NAME, VOICE_PROMPT, VIDEO_PATH] { label(window, id, ""); }
        }
        form.updating = false;
        show_page(window, SendMessageW(GetDlgItem(window, TAB), TCM_GETCURSEL, 0, 0) == 1);
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
        EnableWindow(list, (!form.character_ids.is_empty() && form.voice_job.is_none()) as i32);
        set_draft_controls(window, form);
        show_page(window, SendMessageW(GetDlgItem(window, TAB), TCM_GETCURSEL, 0, 0) == 1);
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
        stop_previews(window, form);
        capture_current_draft(window, form);
        let Some(id) = form.character_ids.get(index).cloned() else { return; };
        form.active_draft = Some(id.clone());
        form.selected_character = Some(id.clone());
        SendMessageW(GetDlgItem(window, CHARACTER_LIST), LB_SETCURSEL, index, 0);
        let _ = draft_for(form, &id);
        set_draft_controls(window, form);
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
        if index <= 0 { None }
        else { form.devices.get(index as usize - 1).map(|device| device.id.clone()).or_else(|| form.missing.clone()) }
    }

    unsafe fn read_speech_settings(window: HWND, form: &Form) -> Settings {
        let mut settings = form.settings.clone();
        settings.volume = SendMessageW(GetDlgItem(window, VOLUME), TBM_GETPOS, 0, 0) as u16;
        settings.output_device = selected_output(window, form);
        let index = SendMessageW(GetDlgItem(window, MODEL), CB_GETCURSEL, 0, 0);
        settings.speech_model = SpeechModel::ALL.get(index.max(0) as usize).copied().unwrap_or_default();
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
            Some(Ok(())) => "Using the key entered in Settings. Kitten CPU provides local speech.".into(),
            None => "Enter an ElevenLabs key, or leave blank to use Kitten CPU.".into(),
            Some(Err(error)) => format!("ElevenLabs API key unavailable: {error}"),
        }
    }

    unsafe fn set_cloud_controls(window: HWND, enabled: bool) {
        EnableWindow(GetDlgItem(window, APPLY), enabled as i32);
        for id in [CHARACTER_LIST, NEW_CHARACTER, CHARACTER_NAME, VOICE_PROMPT, VIDEO_PATH, REMOVE_CHARACTER] {
            EnableWindow(GetDlgItem(window, id), enabled as i32);
        }
    }

    unsafe fn poll_cloud_job(window: HWND, form: &mut Form) {
        let Some(job) = form.voice_job.as_ref() else { return; };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Voice generation stopped unexpectedly.".into()),
        };
        let job = form.voice_job.take().unwrap();
        KillTimer(window, 2);
        set_cloud_controls(window, true);
        label(window, PLAY_VOICE, "Play voice example");
        if let Ok(voice_id) = &result {
            if let Some(draft) = form.drafts.get_mut(&job.id) { draft.voice = CharacterVoice::ElevenLabs { voice_id: voice_id.clone() }; }
        }
        if job.cancelled.load(Ordering::Relaxed) { return; }
        let mut settings = read_speech_settings(window, form);
        if let Err(error) = result {
            settings.elevenlabs_api_key = None;
            label(window, STATUS, &format!("Voice generation failed; playing Kitten CPU: {error}"));
        }
        play_character_example(window, form, settings);
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
        if let Some(job) = &form.voice_job {
            job.cancelled.store(true, Ordering::Relaxed);
            label(window, STATUS, "Voice example stopped. Finishing the pending voice request.");
            return;
        }
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
        let Some(id) = form.active_draft.clone() else { return; };
        let settings = read_speech_settings(window, form);
        if settings.volume == 0 { label(window, STATUS, "Voice preview is silent at 0% volume."); return; }
        let draft = draft_for(form, &id);
        if matches!(draft.voice, CharacterVoice::ElevenLabs { .. }) || settings.elevenlabs_api_key.is_none() {
            play_character_example(window, form, settings);
            return;
        }
        let name = draft.name.clone();
        let description = draft.voice_description.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        form.voice_job = Some(VoiceJob { id, receiver, cancelled });
        set_cloud_controls(window, false);
        label(window, PLAY_VOICE, "Stop example");
        label(window, STATUS, "Preparing the character voice…");
        SetTimer(window, 2, 100, None);
        std::thread::spawn(move || {
            let result = (|| {
                let client = Client::from_settings(&settings)?.ok_or("Enter an ElevenLabs key in General settings.")?;
                let previews = client.design(&description, VOICE_EXAMPLE)?;
                if stop.load(Ordering::Relaxed) { return Err("Voice example was cancelled.".into()); }
                let preview = previews.first().ok_or("No voice example was returned.")?;
                client.create_voice(&name, &description, &preview.generated_voice_id)
            })();
            let _ = sender.send(result);
        });
    }

    unsafe fn play_character_example(window: HWND, form: &mut Form, mut settings: Settings) {
        if settings.volume == 0 { label(window, STATUS, "Voice preview is silent at 0% volume."); return; }
        let Some(id) = form.active_draft.clone() else { return; };
        let character = draft_for(form, &id).clone();
        settings.characters.insert(id.clone(), character);
        settings.selected_character = Some(id);
        form.voice_preview = Some(crate::platform::Preview::voice(settings, VOICE_EXAMPLE.into(), form.assets.clone()));
        label(window, PLAY_VOICE, "Stop example");
        label(window, STATUS, "Playing voice example.");
        SetTimer(window, 3, 100, None);
    }

    unsafe fn stop_previews(window: HWND, form: &mut Form) {
        if let Some(job) = &form.voice_job { job.cancelled.store(true, Ordering::Relaxed); }
        form.voice_preview = None;
        form.preview = None;
        KillTimer(window, 1);
        KillTimer(window, 3);
        label(window, PLAY_VOICE, "Play voice example");
        label(window, PREVIEW, "Play example");
    }

    unsafe fn layout(window: HWND, width: i32, height: i32) {
        let width = width.max(680);
        let height = height.max(660);
        let move_control = |id: i32, x: i32, y: i32, w: i32, h: i32| {
            SetWindowPos(GetDlgItem(window, id), std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        };
        move_control(TAB, 12, 12, width - 24, height - 64);
        move_control(QUIET, 28, 54, width - 56, 28);
        move_control(QUIET_HINT, 28, 84, width - 56, 24);
        move_control(SCHEDULE, 28, 120, width - 56, 28);
        move_control(START_LABEL, 28, 156, 45, 24);
        move_control(START, 76, 152, 78, 28);
        move_control(END_LABEL, 168, 156, 24, 24);
        move_control(END, 198, 152, 78, 28);
        move_control(TIME_HINT, 290, 156, width - 318, 24);
        move_control(VOLUME_LABEL, 28, 202, width - 56, 24);
        move_control(VOLUME, 28, 230, width - 56, 40);
        move_control(OUTPUT_LABEL, 28, 278, width - 56, 24);
        move_control(OUTPUT, 28, 306, width - 154, 220);
        move_control(REFRESH, width - 118, 306, 90, 28);
        move_control(MODEL_LABEL, 28, 350, 110, 24);
        move_control(MODEL, 144, 346, width - 172, 180);
        move_control(API_KEY_LABEL, 28, 392, 110, 24);
        move_control(API_KEY, 144, 388, width - 172, 28);
        move_control(API_KEY_HINT, 28, 426, width - 56, 42);
        move_control(PREVIEW, 28, 484, 140, 30);
        move_control(PREVIEW_HINT, 180, 486, width - 208, 36);
        move_control(CHARACTER_LIST, 28, 54, 196, height - 204);
        move_control(NEW_CHARACTER, 28, height - 138, 196, 30);
        move_control(NAME_LABEL, 248, 54, width - 276, 24);
        move_control(CHARACTER_NAME, 248, 82, width - 276, 28);
        move_control(VIDEO_LABEL, 248, 130, width - 276, 24);
        move_control(VIDEO_PATH, 248, 158, width - 276, 30);
        move_control(PROMPT_LABEL, 248, 210, width - 276, 24);
        move_control(VOICE_PROMPT, 248, 238, width - 276, height - 438);
        move_control(PLAY_VOICE, 248, height - 174, 160, 32);
        move_control(REMOVE_CHARACTER, width - 118, height - 174, 90, 32);
        move_control(STATUS, 28, height - 94, width - 56, 36);
        move_control(APPLY, width - 228, height - 48, 100, 30);
        move_control(CLOSE, width - 116, height - 48, 100, 30);
    }

    unsafe fn show_page(window: HWND, characters: bool) {
        for id in [QUIET, QUIET_HINT, SCHEDULE, START_LABEL, START, END_LABEL, END, TIME_HINT, VOLUME_LABEL, VOLUME, OUTPUT_LABEL, OUTPUT, REFRESH, MODEL, MODEL_LABEL, API_KEY, API_KEY_LABEL, API_KEY_HINT, PREVIEW, PREVIEW_HINT] {
            ShowWindow(GetDlgItem(window, id), if characters { SW_HIDE } else { SW_SHOW });
        }
        for id in [CHARACTER_LIST, NEW_CHARACTER] {
            ShowWindow(GetDlgItem(window, id), if characters { SW_SHOW } else { SW_HIDE });
        }
        let form = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Form;
        let show_editor = characters && !form.is_null() && (*form).active_draft.is_some();
        for id in [NAME_LABEL, CHARACTER_NAME, PROMPT_LABEL, VOICE_PROMPT, VIDEO_LABEL, VIDEO_PATH, PLAY_VOICE, REMOVE_CHARACTER] {
            ShowWindow(GetDlgItem(window, id), if show_editor { SW_SHOW } else { SW_HIDE });
        }
    }

    unsafe fn add_tabs(window: HWND) -> Result<(), String> {
        let tab = GetDlgItem(window, TAB);
        for title in ["General", "Characters"] {
            let mut title = wide(title);
            let item = TCITEMW { mask: TCIF_TEXT, pszText: title.as_mut_ptr(), ..TCITEMW::default() };
            if SendMessageW(tab, TCM_INSERTITEMW, SendMessageW(tab, TCM_GETITEMCOUNT, 0, 0) as usize, &item as *const _ as isize) < 0 {
                return Err("Could not create settings tabs.".into());
            }
        }
        Ok(())
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
                let mut bounds = RECT { left: 0, top: 0, right: 680, bottom: 660 };
                AdjustWindowRectEx(&mut bounds, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_THICKFRAME | WS_MAXIMIZEBOX, 0, WS_EX_CONTROLPARENT);
                info.ptMinTrackSize.x = bounds.right - bounds.left;
                info.ptMinTrackSize.y = bounds.bottom - bounds.top;
                0
            }
            WM_NOTIFY if !form.is_null() && lparam != 0 => {
                let header = &*(lparam as *const NMHDR);
                if header.idFrom == TAB as usize && header.code == TCN_SELCHANGE {
                    stop_previews(window, &mut *form);
                    let characters = SendMessageW(GetDlgItem(window, TAB), TCM_GETCURSEL, 0, 0) == 1;
                    show_page(window, characters);
                }
                0
            }
            WM_COMMAND if !form.is_null() => {
                let id = (wparam & 0xffff) as i32;
                let code = ((wparam >> 16) & 0xffff) as u32;
                match id {
                    API_KEY if code == EN_CHANGE => {
                        let status = api_key_status(Some(text(window, API_KEY).trim()));
                        label(window, API_KEY_HINT, &status);
                    }
                    APPLY => match save(window, &mut *form) {
                        Ok(()) => {}
                        Err(error) => { MessageBoxW(window, wide(&error).as_ptr(), wide("Civilized Agent settings").as_ptr(), MB_OK | MB_ICONERROR); }
                    },
                    CLOSE => { DestroyWindow(window); }
                    REFRESH => {
                        let selected = selected_output(window, &*form);
                        populate_outputs(window, &mut *form, selected.as_deref());
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
                        stop_previews(window, &mut *form);
                        capture_current_draft(window, &mut *form);
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
                    CHARACTER_NAME | VOICE_PROMPT if code == EN_CHANGE => capture_current_draft(window, &mut *form),
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
            WM_TIMER if !form.is_null() && wparam == 2 => {
                poll_cloud_job(window, &mut *form);
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
            WM_CLOSE => { DestroyWindow(window); 0 }
            WM_DESTROY => { if !form.is_null() { stop_previews(window, &mut *form); } KillTimer(window, 2); PostQuitMessage(0); 0 }
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
            voice_job: None,
            removed_characters: std::collections::BTreeSet::new(),
            updating: false,
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
            InitCommonControlsEx(&INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES | ICC_TAB_CLASSES });
            let instance = GetModuleHandleW(std::ptr::null());
            let window_class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: instance, lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW), hbrBackground: (COLOR_BTNFACE + 1) as usize as HBRUSH, ..WNDCLASSW::default() };
            if RegisterClassW(&window_class) == 0 { return Err(std::io::Error::last_os_error().to_string()); }
            let window = CreateWindowExW(WS_EX_CONTROLPARENT, class.as_ptr(), wide("Civilized Agent settings").as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_THICKFRAME | WS_MAXIMIZEBOX, CW_USEDEFAULT, CW_USEDEFAULT, 720, 700,
                std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
            if window.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
            SetWindowLongPtrW(window, GWLP_USERDATA, &mut *form as *mut Form as isize);
            control(window, "SysTabControl32", "", TAB, WS_TABSTOP, (12, 12, 696, 604))?;
            add_tabs(window)?;
            control(window, "BUTTON", "Quiet mode (mute speech and static)", QUIET, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (28, 54, 664, 28))?;
            control(window, "STATIC", "Announcements still appear while quiet mode is on.", QUIET_HINT, 0, (28, 84, 664, 24))?;
            control(window, "BUTTON", "Quiet mode on a daily schedule", SCHEDULE, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (28, 120, 664, 28))?;
            control(window, "STATIC", "From", START_LABEL, 0, (28, 156, 45, 24))?;
            control(window, "EDIT", &format_time(form.settings.quiet_start), START, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (76, 152, 78, 28))?;
            control(window, "STATIC", "to", END_LABEL, 0, (168, 156, 24, 24))?;
            control(window, "EDIT", &format_time(form.settings.quiet_end), END, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (198, 152, 78, 28))?;
            control(window, "STATIC", "Local time, HH:MM", TIME_HINT, 0, (290, 156, 300, 24))?;
            control(window, "STATIC", &volume_label(form.settings.volume), VOLUME_LABEL, 0, (28, 202, 664, 24))?;
            control(window, "msctls_trackbar32", "Announcer volume", VOLUME, WS_TABSTOP | TBS_AUTOTICKS, (28, 230, 664, 40))?;
            SendMessageW(GetDlgItem(window, VOLUME), TBM_SETRANGEMAX, 0, 100);
            SendMessageW(GetDlgItem(window, VOLUME), TBM_SETPOS, 1, form.settings.volume as isize);
            control(window, "STATIC", "Audio output (speech and static)", OUTPUT_LABEL, 0, (28, 278, 664, 24))?;
            control(window, "COMBOBOX", "Audio output", OUTPUT, WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, (28, 306, 566, 220))?;
            control(window, "BUTTON", "Refresh", REFRESH, WS_TABSTOP, (602, 306, 90, 28))?;
            control(window, "STATIC", "Speech model", MODEL_LABEL, 0, (28, 350, 110, 24))?;
            control(window, "COMBOBOX", "Speech model", MODEL, WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, (144, 346, 548, 180))?;
            for model in SpeechModel::ALL {
                SendMessageW(GetDlgItem(window, MODEL), CB_ADDSTRING, 0, wide(model.label()).as_ptr() as isize);
            }
            let selected = SpeechModel::ALL.iter().position(|model| *model == form.settings.speech_model).unwrap_or(0);
            SendMessageW(GetDlgItem(window, MODEL), CB_SETCURSEL, selected, 0);
            control(window, "STATIC", "ElevenLabs key", API_KEY_LABEL, 0, (28, 392, 110, 24))?;
            control(window, "EDIT", form.settings.elevenlabs_api_key.as_deref().unwrap_or_default(), API_KEY, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32, (144, 388, 548, 28))?;
            SendMessageW(GetDlgItem(window, API_KEY), EM_SETLIMITTEXT, 4096, 0);
            control(window, "STATIC", &api_key_status(form.settings.elevenlabs_api_key.as_deref()), API_KEY_HINT, 0, (28, 426, 664, 42))?;
            control(window, "BUTTON", "Play example", PREVIEW, WS_TABSTOP, (28, 412, 140, 30))?;
            control(window, "STATIC", "Previews your selected settings without saving.", PREVIEW_HINT, 0, (180, 414, 512, 36))?;
            control(window, "LISTBOX", "Characters", CHARACTER_LIST, WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32, (28, 54, 196, 456))?;
            control(window, "BUTTON", "New", NEW_CHARACTER, WS_TABSTOP, (28, 522, 196, 30))?;
            control(window, "STATIC", "Name", NAME_LABEL, 0, (28, 94, 110, 24))?;
            control(window, "EDIT", "", CHARACTER_NAME, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (144, 90, 548, 28))?;
            control(window, "STATIC", "Prompt", PROMPT_LABEL, 0, (28, 132, 110, 24))?;
            control(window, "EDIT", "", VOICE_PROMPT, WS_BORDER | WS_TABSTOP | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL, (144, 128, 548, 74))?;
            control(window, "STATIC", "Video", VIDEO_LABEL, 0, (248, 130, 400, 24))?;
            control(window, "BUTTON", "Choose video…", VIDEO_PATH, WS_TABSTOP | BS_LEFT as u32, (248, 158, 400, 30))?;
            control(window, "BUTTON", "Play voice example", PLAY_VOICE, WS_TABSTOP, (248, 486, 160, 32))?;
            control(window, "BUTTON", "Delete", REMOVE_CHARACTER, WS_TABSTOP, (574, 486, 90, 32))?;
            control(window, "STATIC", "Uses the system default if your selected device is unavailable.", STATUS, 0, (28, 586, 664, 36))?;
            control(window, "BUTTON", "Apply", APPLY, WS_TABSTOP | BS_DEFPUSHBUTTON as u32, (492, 632, 100, 30))?;
            control(window, "BUTTON", "Close", CLOSE, WS_TABSTOP, (604, 632, 100, 30))?;
            set_checked(window, QUIET, form.settings.quiet_mode);
            set_checked(window, SCHEDULE, form.settings.schedule_enabled);
            EnableWindow(GetDlgItem(window, START), form.settings.schedule_enabled as i32);
            EnableWindow(GetDlgItem(window, END), form.settings.schedule_enabled as i32);
            let selected = form.settings.output_device.clone();
            populate_outputs(window, &mut form, selected.as_deref());
            refresh_characters(window, &mut form);
            show_page(window, false);
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
