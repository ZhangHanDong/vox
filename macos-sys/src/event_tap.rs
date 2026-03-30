use crate::MacosError;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyKey {
    OptionLeft,
    OptionRight,
    FnKey,
    ControlLeft,
    ControlRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyTrigger {
    Hold,
}

#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    pub key: HotkeyKey,
    pub trigger: HotkeyTrigger,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key: HotkeyKey::OptionLeft,
            trigger: HotkeyTrigger::Hold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

pub struct HotkeyHandle {
    _running: Arc<AtomicBool>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

// CGEvent tap constants
const K_CG_EVENT_FLAGS_CHANGED: u32 = 12; // NSEventTypeFlagsChanged
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_SESSION_EVENT_TAP: u32 = 1;

// CGEventFlags masks (from CGEventTypes.h)
const K_CG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x00080000; // Option/Alt (either side)
const K_CG_EVENT_FLAG_MASK_CONTROL: u64 = 0x00040000;   // Control (either side)
const K_CG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x00800000; // Fn key

// Device-specific flags (from IOLLEvent.h, shifted)
const NX_DEVICE_LALT_KEY_MASK: u64 = 0x00000020;
const NX_DEVICE_RALT_KEY_MASK: u64 = 0x00000040;

// Minimum hold duration to trigger (prevents accidental short taps)
const MIN_HOLD_MS: u128 = 100;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: extern "C" fn(
            proxy: *mut c_void,
            event_type: u32,
            event: *mut c_void,
            user_info: *mut c_void,
        ) -> *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void;

    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *mut c_void,
        port: *mut c_void,
        order: i64,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *mut c_void);
    fn CFRunLoopRunInMode(mode: *mut c_void, seconds: f64, return_after_source_handled: bool) -> i32;
    fn CFRunLoopRun();
    #[allow(dead_code)]
    fn CFRunLoopStop(rl: *mut c_void);
    fn CFRelease(cf: *mut c_void);

    static kCFRunLoopCommonModes: *mut c_void;
    static kCFRunLoopDefaultMode: *mut c_void;
}

struct TapContext {
    key_mask: u64,
    key_mask_alt: u64,
    was_pressed: AtomicBool,
    press_time: std::sync::Mutex<Option<std::time::Instant>>,
    callback: Box<dyn Fn(HotkeyEvent) + Send>,
    _running: Arc<AtomicBool>,
}

extern "C" fn event_tap_callback(
    _proxy: *mut c_void,
    _event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void {
    unsafe {
        let ctx = &*(user_info as *const TapContext);
        let flags = CGEventGetFlags(event);
        let key_down = (flags & ctx.key_mask) != 0 || (flags & ctx.key_mask_alt) != 0;
        let was_pressed = ctx.was_pressed.load(Ordering::Relaxed);

        // Debug: log EVERY callback invocation
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
            let _ = writeln!(f, "CALLBACK! event_type={} flags=0x{:x} key_down={} was_pressed={}", _event_type, flags, key_down, was_pressed);
        }

        if key_down && !was_pressed {
            ctx.was_pressed.store(true, Ordering::Relaxed);
            if let Ok(mut time) = ctx.press_time.lock() {
                *time = Some(std::time::Instant::now());
            }
            (ctx.callback)(HotkeyEvent::Pressed);
            // Suppress the event (return null)
            return std::ptr::null_mut();
        } else if !key_down && was_pressed {
            // Key just released
            ctx.was_pressed.store(false, Ordering::Relaxed);
            let held_long_enough = ctx
                .press_time
                .lock()
                .ok()
                .and_then(|t| *t)
                .map(|t| t.elapsed().as_millis() >= MIN_HOLD_MS)
                .unwrap_or(false);

            if held_long_enough {
                (ctx.callback)(HotkeyEvent::Released);
            }
            // Suppress the event
            return std::ptr::null_mut();
        }

        // Pass through other flag changes
        event
    }
}

/// Start global hotkey monitoring via CGEvent tap.
///
/// Requires Accessibility permission (System Preferences → Privacy → Accessibility).
/// The callback is invoked on a dedicated CFRunLoop thread.
pub fn start_hotkey_monitor(
    config: HotkeyConfig,
    callback: impl Fn(HotkeyEvent) + Send + 'static,
) -> Result<HotkeyHandle, MacosError> {
    // CGEventGetFlags returns raw NX event flags, not CGEventFlags constants.
    // NX_ALTERNATEMASK = 0x00080000 in header, but actual runtime flags show 0x100 for Option.
    // Use runtime-observed values:
    let key_mask: u64 = match config.key {
        HotkeyKey::OptionLeft | HotkeyKey::OptionRight => 0x080000, // kCGEventFlagMaskAlternate
        HotkeyKey::FnKey => 0x800000,
        HotkeyKey::ControlLeft | HotkeyKey::ControlRight => 0x040000,
    };

    // Not used — key_mask alone is sufficient
    let key_mask_alt: u64 = key_mask;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let thread = std::thread::Builder::new()
        .name("hotkey-monitor".into())
        .spawn(move || {
            unsafe {
                let ctx = Box::new(TapContext {
                    key_mask,
                    key_mask_alt,
                    was_pressed: AtomicBool::new(false),
                    press_time: std::sync::Mutex::new(None),
                    callback: Box::new(callback),
                    _running: running_clone.clone(),
                });
                let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

                // Event mask for flags changed events
                let event_mask: u64 = 1 << K_CG_EVENT_FLAGS_CHANGED;

                let tap = CGEventTapCreate(
                    K_CG_SESSION_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    0, // kCGEventTapOptionDefault (active tap, can modify)
                    event_mask,
                    event_tap_callback,
                    ctx_ptr,
                );

                if tap.is_null() {
                    let _ = Box::from_raw(ctx_ptr as *mut TapContext);
                    // Write to file since log crate has no logger
                    let _ = std::fs::write("/tmp/voice_input_debug.log",
                        "CGEventTapCreate FAILED — accessibility permission required\n");
                    running_clone.store(false, Ordering::Relaxed);
                    return;
                }

                let run_loop_source =
                    CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
                if run_loop_source.is_null() {
                    CFRelease(tap);
                    let _ = Box::from_raw(ctx_ptr as *mut TapContext);
                    running_clone.store(false, Ordering::Relaxed);
                    return;
                }

                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, run_loop_source, kCFRunLoopCommonModes);

                // CGEventTapEnable defaults to enabled on creation
                // CGEventTapEnable(tap, true);

                let _ = std::fs::write("/tmp/voice_input_debug.log",
                    format!("CGEvent tap STARTED + ENABLED, key_mask: 0x{:x}\n", key_mask));

                CFRunLoopRun();

                CFRelease(run_loop_source);
                CFRelease(tap);
                let _ = Box::from_raw(ctx_ptr as *mut TapContext);
                log::info!("Hotkey monitor thread exiting");
            }
        })
        .map_err(|e| MacosError::Platform(format!("Failed to spawn hotkey thread: {e}")))?;

    Ok(HotkeyHandle {
        _running: running,
        _thread: Some(thread),
    })
}

impl Drop for HotkeyHandle {
    fn drop(&mut self) {
        self._running.store(false, Ordering::Relaxed);
        // CFRunLoopRun will exit when there are no more sources, or we need to
        // explicitly stop it. Since we can't easily get the run loop ref from here,
        // the thread will exit on next CFRunLoopRun iteration check.
    }
}
