#![warn(clippy::all)]
#![allow(dead_code)]

pub use makepad_widgets;

use makepad_widgets::*;

mod audio;
mod config;
mod llm_refine;
mod text_inject;
mod transcribe;

app_main!(App);

// Menu action IDs
const MENU_QUIT: u64 = 1;
const MENU_LANG_ZH: u64 = 10;
const MENU_LANG_EN: u64 = 11;
const MENU_LANG_ZH_TW: u64 = 12;
const MENU_LANG_JA: u64 = 13;
const MENU_LANG_KO: u64 = 14;
const MENU_LANG_WEN: u64 = 15;
const MENU_LLM_TOGGLE: u64 = 20;
const MENU_SETTINGS: u64 = 21;
const MENU_TEST_CAPSULE: u64 = 99;

const STATE_IDLE: u8 = 0;
const STATE_RECORDING: u8 = 1;
const STATE_TRANSCRIBING: u8 = 2;
const STATE_REFINING: u8 = 3;

script_mod! {
    use mod.prelude.widgets.*

    startup() do #(App::script_component(vm)){
        ui: Root{
            on_startup: ||{
                ui.main_view.render()
            }
            main_window := Window{
                window.title: "Voice Input"
                window.inner_size: vec2(1, 1)
                body +: {
                    main_view := View{
                        width: Fill height: Fill
                        on_render: ||{
                            Label{ text: "Voice Input Running" draw_text.color: #xffffff }
                        }
                    }
                }
            }

            capsule_window := Window{
                show_caption_bar: false
                window.title: ""
                window.inner_size: vec2(400, 90)
                window.position: vec2(1520, 1270)
                window.transparent: true
                pass.clear_color: #x00000000
                body +: {
                    View{
                        width: Fill height: Fill
                        flow: Overlay
                        align: Align{x: 0.5 y: 0.5}
                        capsule := View{
                            width: Fit height: 56
                            padding: Inset{left: 28 right: 28 top: 0 bottom: 0}
                            flow: Right spacing: 12
                            align: Align{y: 0.5}
                            show_bg: true
                            draw_bg +: {
                                pixel: fn() {
                                    let w = self.rect_size.x
                                    let h = self.rect_size.y
                                    let r = h * 0.5
                                    let cx_l = r
                                    let cx_r = w - r
                                    let cy = h * 0.5
                                    let px = self.pos.x * w
                                    let py = self.pos.y * h

                                    let d_left = length(vec2(px - cx_l, py - cy)) - r
                                    let d_right = length(vec2(px - cx_r, py - cy)) - r
                                    let d_rect = max(abs(px - w * 0.5) - (cx_r - cx_l) * 0.5, abs(py - cy) - r)
                                    let d = min(min(d_left, d_right), d_rect)

                                    let aa = 1.0 / self.rect_size.y
                                    let alpha = 1.0 - smoothstep(-aa, aa, d)
                                    return Pal.premul(vec4(0.1, 0.1, 0.18, alpha * 0.82))
                                }
                            }
                            new_batch: true
                            waveform := LoadingSpinner{
                                width: 28 height: 28
                            }
                            transcript_label := Label{
                                width: Fit
                                text: "Listening..."
                                draw_text.color: #xffffffdd
                                draw_text.text_style.font_size: 14
                            }
                        }
                    }
                }
            }

            settings_window := Window{
                window.title: "Voice Input Settings"
                window.inner_size: vec2(480, 560)
                window.position: vec2(500, 200)
                body +: {
                    ScrollYView{
                        width: Fill height: Fill
                        flow: Down spacing: 12 padding: 24
                        new_batch: true

                        Label{ text: "ominix-api" draw_text.color: #xffffff draw_text.text_style.font_size: 16 }
                        Label{ text: "Base URL" draw_text.color: #xaaaaaa draw_text.text_style.font_size: 11 }
                        api_url_input := TextInput{ width: Fill height: 36 empty_text: "http://localhost:8080" }

                        Hr{}

                        Label{ text: "Language" draw_text.color: #xaaaaaa draw_text.text_style.font_size: 11 }
                        language_dropdown := DropDown{
                            labels: ["简体中文", "English", "繁體中文", "日本語", "한국어", "文言文"]
                        }

                        Hr{}

                        View{
                            width: Fill height: Fit
                            flow: Right
                            align: Align{y: 0.5}
                            Label{ text: "LLM Refinement" draw_text.color: #xffffff draw_text.text_style.font_size: 14 }
                            Filler{}
                            llm_toggle := CheckBox{ text: "Enable" }
                        }

                        Label{ text: "LLM API Base URL" draw_text.color: #xaaaaaa draw_text.text_style.font_size: 11 }
                        llm_url_input := TextInput{ width: Fill height: 36 empty_text: "http://localhost:8080" }

                        Label{ text: "API Key" draw_text.color: #xaaaaaa draw_text.text_style.font_size: 11 }
                        llm_key_input := TextInput{ width: Fill height: 36 empty_text: "sk-..." is_password: true }

                        Label{ text: "Model" draw_text.color: #xaaaaaa draw_text.text_style.font_size: 11 }
                        llm_model_input := TextInput{ width: Fill height: 36 empty_text: "qwen3-4b" }

                        Hr{}

                        View{
                            width: Fill height: Fit
                            flow: Right spacing: 8
                            align: Align{x: 1.0}
                            test_button := Button{ text: "Test Connection" }
                            save_button := Button{ text: "Save" }
                        }

                        settings_status := Label{ text: "" draw_text.color: #x88cc88 draw_text.text_style.font_size: 11 }
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    configured: bool,
    capsule_window_id: Option<WindowId>,
    state: u8,
    config: config::AppConfig,
    menu_poll_timer: Timer,
    waveform_next_frame: NextFrame,
    restore_timer: Timer,
    error_dismiss_timer: Timer,
    deferred_setup_timer: Timer,
    menu_rx: Option<crossbeam_channel::Receiver<u64>>,
    hotkey_rx: Option<crossbeam_channel::Receiver<macos_sys::event_tap::HotkeyEvent>>,
    status_bar_handle: Option<macos_sys::status_bar::StatusBarHandle>,
    hotkey_handle: Option<macos_sys::event_tap::HotkeyHandle>,
    audio: audio::AudioCapture,
    smooth_rms: f32,
    last_wav: Vec<u8>,
    last_transcription: String,
    inject_state: text_inject::InjectState,
}

#[derive(Script, ScriptHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
    #[rust]
    inner: Inner,
}

impl App {
    fn configure_startup(&mut self, cx: &mut Cx) {
        self.inner.config = config::load_config();

        // Configure capsule window as floating panel
        let capsule = self.ui.window(cx, ids!(capsule_window));
        let mut macos = MacosWindowConfig::floating_panel();
        macos.chrome = MacosWindowChrome::Borderless;
        macos.becomes_key_only_if_needed = true;
        capsule.configure_macos_window(cx, macos);
        self.inner.capsule_window_id = capsule.window_id();

        // Windows start at 1x1 (invisible). show_capsule/show_settings resize them.

        self.setup_status_bar();
        // Note: show_in_dock(false) hides the status bar item too.
        // Use LSUIElement=true in Info.plist for .app bundle instead.
        // During cargo run, app will show in Dock (expected).

        self.setup_hotkey_monitor();
        self.inner.audio.ensure_callback(cx);
        self.inner.menu_poll_timer = cx.start_interval(0.01);
    }

    fn setup_status_bar(&mut self) {
        let menu = self.build_full_menu();
        match macos_sys::status_bar::create_status_bar(&[], menu) {
            Ok((handle, rx)) => {
                log!("Status bar created successfully");
                self.inner.status_bar_handle = Some(handle);
                self.inner.menu_rx = Some(rx);
            }
            Err(e) => log!("Failed to create status bar: {}", e),
        }
    }

    fn build_full_menu(&self) -> Vec<macos_sys::status_bar::MenuItem> {
        use macos_sys::status_bar::MenuItem;
        let lang = &self.inner.config.language;
        let llm_on = self.inner.config.llm_refine.enabled;

        vec![
            MenuItem::submenu("Language", vec![
                { let mut m = MenuItem::new("简体中文", MENU_LANG_ZH); m.checked = lang == "zh"; m },
                { let mut m = MenuItem::new("English", MENU_LANG_EN); m.checked = lang == "en"; m },
                { let mut m = MenuItem::new("繁體中文", MENU_LANG_ZH_TW); m.checked = lang == "zh-TW"; m },
                { let mut m = MenuItem::new("日本語", MENU_LANG_JA); m.checked = lang == "ja"; m },
                { let mut m = MenuItem::new("한국어", MENU_LANG_KO); m.checked = lang == "ko"; m },
                MenuItem::separator(),
                { let mut m = MenuItem::new("文言文", MENU_LANG_WEN); m.checked = lang == "wen"; m },
            ]),
            MenuItem::separator(),
            MenuItem::submenu("LLM Refinement", vec![
                { let mut m = MenuItem::new(if llm_on { "Disable" } else { "Enable" }, MENU_LLM_TOGGLE); m.checked = llm_on; m },
                MenuItem::new("Settings...", MENU_SETTINGS),
            ]),
            MenuItem::separator(),
            MenuItem::new("Test Capsule", MENU_TEST_CAPSULE),
            MenuItem::separator(),
            MenuItem::new("Quit", MENU_QUIT),
        ]
    }

    fn refresh_menu(&mut self) {
        let menu = self.build_full_menu();
        if let Some(ref handle) = self.inner.status_bar_handle {
            macos_sys::status_bar::update_menu(handle, menu);
        }
    }

    fn setup_hotkey_monitor(&mut self) {
        let (hotkey_tx, hotkey_rx) = crossbeam_channel::unbounded();
        self.inner.hotkey_rx = Some(hotkey_rx);
        let config = macos_sys::event_tap::HotkeyConfig::default();
        match macos_sys::event_tap::start_hotkey_monitor(config, move |event| {
            let _ = hotkey_tx.try_send(event);
        }) {
            Ok(handle) => self.inner.hotkey_handle = Some(handle),
            Err(e) => log!("Failed to start hotkey monitor: {e}"),
        }
    }

    fn handle_menu_action(&mut self, cx: &mut Cx, action_id: u64) {
        match action_id {
            MENU_QUIT => cx.quit(),
            MENU_LANG_ZH => { self.inner.config.language = "zh".into(); self.refresh_menu(); }
            MENU_LANG_EN => { self.inner.config.language = "en".into(); self.refresh_menu(); }
            MENU_LANG_ZH_TW => { self.inner.config.language = "zh-TW".into(); self.refresh_menu(); }
            MENU_LANG_JA => { self.inner.config.language = "ja".into(); self.refresh_menu(); }
            MENU_LANG_KO => { self.inner.config.language = "ko".into(); self.refresh_menu(); }
            MENU_LANG_WEN => { self.inner.config.language = "wen".into(); self.refresh_menu(); }
            MENU_LLM_TOGGLE => {
                self.inner.config.llm_refine.enabled = !self.inner.config.llm_refine.enabled;
                self.refresh_menu();
            }
            MENU_SETTINGS => {
                self.show_settings(cx);
            }
            MENU_TEST_CAPSULE => {
                self.show_capsule(cx);
            }
            _ => {}
        }
    }

    fn populate_settings(&self, cx: &mut Cx) {
        let cfg = &self.inner.config;
        self.ui.text_input(cx, ids!(api_url_input))
            .set_text(cx, &cfg.ominix_api.base_url);
        self.ui.text_input(cx, ids!(llm_url_input))
            .set_text(cx, &cfg.llm_refine.api_base_url);
        self.ui.text_input(cx, ids!(llm_key_input))
            .set_text(cx, &cfg.llm_refine.api_key);
        self.ui.text_input(cx, ids!(llm_model_input))
            .set_text(cx, &cfg.llm_refine.model);

        let lang_idx = match cfg.language.as_str() {
            "zh" => 0, "en" => 1, "zh-TW" => 2, "ja" => 3, "ko" => 4, "wen" => 5, _ => 0,
        };
        self.ui.drop_down(cx, ids!(language_dropdown))
            .set_selected_item(cx, lang_idx);

        self.ui.label(cx, ids!(settings_status)).set_text(cx, "");
    }

    fn save_settings(&mut self, cx: &mut Cx) {
        let api_url = self.ui.text_input(cx, ids!(api_url_input)).text();
        let llm_url = self.ui.text_input(cx, ids!(llm_url_input)).text();
        let llm_key = self.ui.text_input(cx, ids!(llm_key_input)).text();
        let llm_model = self.ui.text_input(cx, ids!(llm_model_input)).text();
        let lang_idx = self.ui.drop_down(cx, ids!(language_dropdown)).selected_item();

        self.inner.config.ominix_api.base_url = api_url;
        self.inner.config.llm_refine.api_base_url = llm_url;
        self.inner.config.llm_refine.api_key = llm_key;
        self.inner.config.llm_refine.model = llm_model;
        self.inner.config.language = match lang_idx {
            0 => "zh", 1 => "en", 2 => "zh-TW", 3 => "ja", 4 => "ko", 5 => "wen", _ => "zh",
        }.to_string();

        match config::save_config(&self.inner.config) {
            Ok(()) => {
                self.ui.label(cx, ids!(settings_status)).set_text(cx, "Saved");
                self.refresh_menu();
            }
            Err(e) => {
                self.ui.label(cx, ids!(settings_status))
                    .set_text(cx, &format!("Save failed: {e}"));
            }
        }
    }

    fn test_connection(&mut self, cx: &mut Cx) {
        let url = format!(
            "{}/v1/models",
            self.ui.text_input(cx, ids!(api_url_input)).text().trim_end_matches('/')
        );
        let req = HttpRequest::new(url, HttpMethod::GET);
        cx.http_request(live_id!(test_connection), req);
        self.ui.label(cx, ids!(settings_status)).set_text(cx, "Testing...");
    }

    fn show_capsule(&mut self, cx: &mut Cx) {
        let capsule = self.ui.window(cx, ids!(capsule_window));
        let win_w = 400.0;
        let win_h = 90.0;
        let screen_w = 3440.0;
        let screen_h = 1440.0;
        let x = (screen_w - win_w) / 2.0;
        let y = screen_h - win_h - 80.0;
        capsule.resize(cx, dvec2(win_w, win_h));
        capsule.reposition(cx, dvec2(x, y));
    }

    fn hide_capsule(&mut self, cx: &mut Cx) {
        let capsule = self.ui.window(cx, ids!(capsule_window));
        capsule.resize(cx, dvec2(1.0, 1.0));
    }

    fn show_settings(&mut self, cx: &mut Cx) {
        self.populate_settings(cx);
        let settings = self.ui.window(cx, ids!(settings_window));
        // configure_window triggers makeKeyAndOrderFront on macOS
        settings.configure_window(
            cx,
            dvec2(480.0, 560.0),
            dvec2(500.0, 200.0),
            false,
            "Voice Input Settings".to_string(),
        );
    }

    fn start_recording(&mut self, cx: &mut Cx) {
        {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
                let _ = writeln!(f, "APP: start_recording called, state={}", self.inner.state);
            }
        }
        if self.inner.state != STATE_IDLE { return; }
        self.inner.state = STATE_RECORDING;
        self.inner.smooth_rms = 0.0;
        self.inner.audio.start(cx);
        self.inner.waveform_next_frame = cx.new_next_frame();
        self.ui.label(cx, ids!(transcript_label)).set_text(cx, "Listening...");
        self.show_capsule(cx);

        if let Some(ref handle) = self.inner.status_bar_handle {
            macos_sys::status_bar::set_status_bar_icon(handle, "🔴");
        }
    }

    fn stop_recording(&mut self, cx: &mut Cx) {
        {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
                let _ = writeln!(f, "APP: stop_recording called, state={}", self.inner.state);
            }
        }
        if self.inner.state != STATE_RECORDING { return; }
        self.inner.state = STATE_TRANSCRIBING;
        let samples = self.inner.audio.stop(cx);
        {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
                let _ = writeln!(f, "APP: got {} samples", samples.len());
            }
        }
        if samples.is_empty() {
            self.inner.state = STATE_IDLE;
            return;
        }
        self.inner.last_wav = audio::encode_wav(&samples, 16_000);
        self.ui.label(cx, ids!(transcript_label)).set_text(cx, "Transcribing...");
        transcribe::send_transcribe_request(
            cx,
            &self.inner.config.ominix_api.base_url,
            &self.inner.last_wav,
            &self.inner.config.language,
        );
    }

    fn handle_transcribe_result(&mut self, cx: &mut Cx, text: &str) {
        if text.trim().is_empty() {
            self.inner.state = STATE_IDLE;
            return;
        }

        self.inner.last_transcription = text.to_string();
        self.ui.label(cx, ids!(transcript_label)).set_text(cx, text);

        // LLM refine if enabled, or forced for Traditional Chinese (ASR only outputs simplified)
        let cfg = &self.inner.config.llm_refine;
        let needs_llm = cfg.enabled
            || self.inner.config.language == "zh-TW"   // simplified→traditional
            || self.inner.config.language == "en"       // translation
            || self.inner.config.language == "wen";     // 白话→文言文
        if needs_llm && !cfg.api_base_url.is_empty() && !cfg.api_key.is_empty() {
            self.inner.state = STATE_REFINING;
            self.ui.label(cx, ids!(transcript_label)).set_text(cx, "Refining...");
            // Map ISO code to full language name for LLM
            let target_lang = match self.inner.config.language.as_str() {
                "zh" => "Chinese",
                "en" => "English",
                "zh-TW" => "Traditional Chinese",
                "ja" => "Japanese",
                "ko" => "Korean",
                "wen" => "Classical Chinese (文言文)",
                _ => "Chinese",
            };
            llm_refine::send_refine_request(
                cx,
                &cfg.api_base_url,
                &cfg.api_key,
                &cfg.model,
                &cfg.system_prompt,
                text,
                target_lang,
            );
        } else {
            self.inject_text(cx, text);
        }
    }

    fn handle_refine_result(&mut self, cx: &mut Cx, text: &str) {
        let final_text = if text.trim().is_empty() {
            self.inner.last_transcription.clone()
        } else {
            text.to_string()
        };
        self.ui.label(cx, ids!(transcript_label)).set_text(cx, &final_text);
        self.inject_text(cx, &final_text);
    }

    fn inject_text(&mut self, cx: &mut Cx, text: &str) {
        self.inner.inject_state.inject(text);
        self.inner.restore_timer = cx.start_timeout(0.05);

        // Restore status bar icon to default
        if let Some(ref handle) = self.inner.status_bar_handle {
            macos_sys::status_bar::set_status_bar_icon(handle, "🎤");
        }
    }

    fn handle_error(&mut self, cx: &mut Cx, msg: &str) {
        self.ui.label(cx, ids!(transcript_label)).set_text(cx, msg);
        self.inner.state = STATE_IDLE;
        self.inner.error_dismiss_timer = cx.start_timeout(3.0);
    }

    fn update_waveform(&mut self, cx: &mut Cx) {
        let raw_rms = self.inner.audio.read_rms();
        let alpha = if raw_rms > self.inner.smooth_rms { 0.4 } else { 0.15 };
        self.inner.smooth_rms += (raw_rms - self.inner.smooth_rms) * alpha;

        // Redraw the entire capsule window to update draw_pass.time in shader
        self.ui.widget(cx, ids!(capsule_window)).redraw(cx);
    }
}

impl MatchEvent for App {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        if self.ui.button(cx, ids!(save_button)).clicked(actions) {
            self.save_settings(cx);
        }
        if self.ui.button(cx, ids!(test_button)).clicked(actions) {
            self.test_connection(cx);
        }
    }

    fn handle_audio_devices(&mut self, _cx: &mut Cx, e: &AudioDevicesEvent) {
        self.inner.audio.handle_audio_devices(e);
    }

    fn handle_next_frame(&mut self, cx: &mut Cx, _e: &NextFrameEvent) {
        if self.inner.state == STATE_RECORDING {
            self.update_waveform(cx);
            self.inner.waveform_next_frame = cx.new_next_frame();
        }
    }

    fn handle_timer(&mut self, cx: &mut Cx, event: &TimerEvent) {
        if self.inner.restore_timer.is_timer(event).is_some() {
            self.inner.inject_state.restore();
            self.inner.state = STATE_IDLE;
            self.hide_capsule(cx);
        }
        if self.inner.error_dismiss_timer.is_timer(event).is_some() {
            self.ui.label(cx, ids!(transcript_label)).set_text(cx, "");
            self.hide_capsule(cx);
        }
        if self.inner.deferred_setup_timer.is_timer(event).is_some() {
                self.setup_status_bar();
        }
        if self.inner.menu_poll_timer.is_timer(event).is_some() {
            let menu_actions: Vec<u64> = self.inner.menu_rx.as_ref()
                .map(|rx| rx.try_iter().collect()).unwrap_or_default();
            for action_id in menu_actions {
                self.handle_menu_action(cx, action_id);
            }
            let hotkey_events: Vec<macos_sys::event_tap::HotkeyEvent> = self.inner.hotkey_rx.as_ref()
                .map(|rx| rx.try_iter().collect()).unwrap_or_default();
            if !hotkey_events.is_empty() {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
                    let _ = writeln!(f, "APP: received {} hotkey events", hotkey_events.len());
                }
            }
            for evt in hotkey_events {
                match evt {
                    macos_sys::event_tap::HotkeyEvent::Pressed => self.start_recording(cx),
                    macos_sys::event_tap::HotkeyEvent::Released => self.stop_recording(cx),
                }
            }
        }
    }

    fn handle_http_response(&mut self, cx: &mut Cx, request_id: LiveId, response: &HttpResponse) {
        if request_id == transcribe::TRANSCRIBE_REQUEST_ID {
            {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
                    let body = response.body_string().unwrap_or_default();
                    let _ = writeln!(f, "APP: HTTP response status={} body={}", response.status_code, &body[..body.len().min(200)]);
                }
            }
            match transcribe::parse_transcribe_response(response) {
                Ok(text) => self.handle_transcribe_result(cx, &text),
                Err(e) => self.handle_error(cx, &format!("Transcription failed: {e}")),
            }
        } else if request_id == llm_refine::LLM_REFINE_REQUEST_ID {
            match llm_refine::parse_refine_response(response) {
                Ok(text) => self.handle_refine_result(cx, &text),
                Err(e) => {
                    log!("LLM refine failed: {e}, using original transcription");
                    let original = self.inner.last_transcription.clone();
                    self.inject_text(cx, &original);
                    self.inner.state = STATE_IDLE;
                }
            }
        } else if request_id == live_id!(test_connection) {
            if response.status_code == 200 {
                self.ui.label(cx, ids!(settings_status)).set_text(cx, "Connected");
            } else {
                self.ui.label(cx, ids!(settings_status))
                    .set_text(cx, &format!("Error: HTTP {}", response.status_code));
            }
        }
    }

    fn handle_http_request_error(&mut self, cx: &mut Cx, request_id: LiveId, _err: &HttpError) {
        if request_id == transcribe::TRANSCRIBE_REQUEST_ID {
            self.handle_error(cx, "Service unavailable");
        } else if request_id == llm_refine::LLM_REFINE_REQUEST_ID {
            let original = self.inner.last_transcription.clone();
            self.inject_text(cx, &original);
            self.inner.state = STATE_IDLE;
        } else if request_id == live_id!(test_connection) {
            self.ui.label(cx, ids!(settings_status)).set_text(cx, "Connection failed");
        }
    }
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        crate::makepad_widgets::script_mod(vm);
        self::script_mod(vm)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        if let Event::Startup = event {
            if !self.inner.configured {
                self.inner.configured = true;
                self.configure_startup(cx);
            }
        }
        if let Event::WindowDragQuery(dq) = event {
            if Some(dq.window_id) == self.inner.capsule_window_id {
                let capsule = self.ui.window(cx, ids!(capsule_window));
                let size = capsule.get_inner_size(cx);
                if dq.abs.y < 56.0 && dq.abs.x < size.x {
                    dq.response.set(WindowDragQueryResponse::Caption);
                    cx.set_cursor(MouseCursor::Default);
                }
            }
        }
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}
