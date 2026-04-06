use makepad_widgets::log;

/// Orchestrate text injection into the currently focused input field.
///
/// Steps:
/// 1. Save original clipboard content
/// 2. Check if current input source is CJK
/// 3. If CJK, switch to ASCII input source
/// 4. Write text to clipboard
/// 5. Simulate Cmd+V paste
/// 6. (After delay) Restore original input source
/// 7. (After delay) Restore original clipboard
///
/// The delay steps are handled by the caller via timers.
///
/// State for the injection process.
#[derive(Default)]
pub struct InjectState {
    pub original_clipboard: Option<String>,
    pub original_input_source: Option<String>,
    pub was_cjk: bool,
    pub pending_restore: bool,
}

impl InjectState {
    /// Begin text injection via clipboard + Cmd+V simulation.
    ///
    /// WARNING: Temporarily clobbers the user's clipboard for ~80ms.
    /// The original content is saved and restored in `restore()`.
    /// Caller must call `restore()` after a short delay to minimize the window.
    pub fn inject(&mut self, text: &str) {
        // Save original clipboard before clobbering
        self.original_clipboard = macos_sys::clipboard::read_clipboard();

        self.was_cjk = macos_sys::input_source::is_cjk_input_source();
        if self.was_cjk {
            self.original_input_source =
                Some(macos_sys::input_source::current_input_source_id());
            if let Err(e) = macos_sys::input_source::select_ascii_input_source() {
                log!("Failed to switch to ASCII input source: {e}");
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }

        macos_sys::clipboard::write_clipboard(text);
        macos_sys::key_inject::simulate_cmd_v();

        self.pending_restore = true;
        log!("Text injected, pending restore");
    }

    /// Restore original clipboard and input source.
    /// Call after a delay (50ms) to ensure paste has completed.
    pub fn restore(&mut self) {
        if !self.pending_restore {
            return;
        }
        self.pending_restore = false;

        if self.was_cjk {
            if let Some(ref id) = self.original_input_source {
                if let Err(e) = macos_sys::input_source::select_input_source(id) {
                    log!("Failed to restore input source: {e}");
                }
            }
        }

        if let Some(ref original) = self.original_clipboard {
            macos_sys::clipboard::write_clipboard(original);
        }

        self.original_clipboard = None;
        self.original_input_source = None;
        self.was_cjk = false;

        log!("Clipboard and input source restored");
    }
}
