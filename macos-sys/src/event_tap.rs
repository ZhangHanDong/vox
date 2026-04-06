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
    _run_loop: Arc<std::sync::Mutex<usize>>, // stores CFRunLoopRef as usize for Send
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
    // SAFETY: user_info is a valid TapContext pointer created by Box::into_raw
    // in start_hotkey_monitor (line ~200). CGEventTapCreate passes it through
    // unmodified. We wrap all Rust code in catch_unwind to prevent panic from
    // crossing the extern "C" FFI boundary into CoreGraphics.
    // Defensive: user_info is created by our code (Box::into_raw), but guard against
    // unexpected null from the OS callback contract.
    if user_info.is_null() {
        return event;
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
    // SAFETY: user_info was verified non-null above. It points to a valid
    // TapContext created by Box::into_raw in start_hotkey_monitor.
    // CGEventGetFlags is safe to call with a valid event pointer from macOS.
    unsafe {
        let ctx = &*(user_info as *const TapContext);
        let flags = CGEventGetFlags(event);
        let key_down = (flags & ctx.key_mask) != 0 || (flags & ctx.key_mask_alt) != 0;
        let was_pressed = ctx.was_pressed.load(Ordering::Relaxed);

        if key_down && !was_pressed {
            ctx.was_pressed.store(true, Ordering::Relaxed);
            if let Ok(mut time) = ctx.press_time.try_lock() {
                *time = Some(std::time::Instant::now());
            }
            (ctx.callback)(HotkeyEvent::Pressed);
            return std::ptr::null_mut();
        } else if !key_down && was_pressed {
            ctx.was_pressed.store(false, Ordering::Relaxed);
            let held_long_enough = ctx
                .press_time
                .try_lock()
                .ok()
                .and_then(|t| *t)
                .map(|t| t.elapsed().as_millis() >= MIN_HOLD_MS)
                .unwrap_or(false);

            if held_long_enough {
                (ctx.callback)(HotkeyEvent::Released);
            }
            return std::ptr::null_mut();
        }

        event
    }));

    // If Rust panicked, pass the event through unchanged (conservative fallback)
    result.unwrap_or(event)
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
    // Store CFRunLoopRef as usize for Send safety
    let run_loop_ref: Arc<std::sync::Mutex<usize>> =
        Arc::new(std::sync::Mutex::new(0));
    let run_loop_ref_clone = run_loop_ref.clone();

    let thread = std::thread::Builder::new()
        .name("hotkey-monitor".into())
        .spawn(move || {
            // SAFETY: CGEventTapCreate registers our callback with macOS input system.
            // ctx_ptr is Box::into_raw'd and reclaimed on all exit paths (null tap,
            // null source, or CFRunLoopRun return). CFRunLoopRun blocks until
            // CFRunLoopStop is called from HotkeyHandle::drop.
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
                // Save ref so Drop can call CFRunLoopStop
                if let Ok(mut rl) = run_loop_ref_clone.lock() {
                    *rl = run_loop as usize;
                }
                CFRunLoopAddSource(run_loop, run_loop_source, kCFRunLoopCommonModes);

                CFRunLoopRun();

                // Thread is exiting — clear the cached RunLoop ref so Drop won't
                // call CFRunLoopStop on a stale pointer.
                if let Ok(mut rl) = run_loop_ref_clone.lock() {
                    *rl = 0;
                }

                CFRelease(run_loop_source);
                CFRelease(tap);
                let _ = Box::from_raw(ctx_ptr as *mut TapContext);
            }
        })
        .map_err(|e| MacosError::Platform(format!("Failed to spawn hotkey thread: {e}")))?;

    Ok(HotkeyHandle {
        _running: running,
        _run_loop: run_loop_ref,
        _thread: Some(thread),
    })
}

impl Drop for HotkeyHandle {
    fn drop(&mut self) {
        self._running.store(false, Ordering::Relaxed);
        // Stop the CFRunLoop so the thread can exit.
        // The thread clears the ref to 0 on exit, so if it already exited
        // we skip the call (avoids stale-pointer FFI).
        if let Ok(rl) = self._run_loop.lock() {
            if *rl != 0 {
                // SAFETY: CFRunLoopStop is thread-safe and idempotent.
                // The ref is valid because the thread hasn't cleared it yet.
                unsafe { CFRunLoopStop(*rl as *mut c_void); }
            }
        }
        // Join the thread to ensure clean shutdown and resource release.
        if let Some(thread) = self._thread.take() {
            let _ = thread.join();
        }
        // CFRunLoopRun will exit when there are no more sources, or we need to
        // explicitly stop it. Since we can't easily get the run loop ref from here,
        // the thread will exit on next CFRunLoopRun iteration check.
    }
}
