use std::ffi::c_void;

// Virtual keycodes
const K_VK_V: u16 = 9;

// CGEvent flags
const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(
        source: *mut c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

extern "C" {
    fn CFRelease(cf: *mut c_void);
}

/// Simulate Cmd+V paste keystroke.
pub fn simulate_cmd_v() {
    // SAFETY: CGEventCreateKeyboardEvent returns null on failure (checked).
    // CGEventPost posts to HID event tap (requires Accessibility permission).
    // CFRelease frees each event after posting. 10ms sleep ensures key-up follows key-down.
    unsafe {
        // Key down
        let key_down = CGEventCreateKeyboardEvent(std::ptr::null_mut(), K_VK_V, true);
        if !key_down.is_null() {
            CGEventSetFlags(key_down, K_CG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(0, key_down); // kCGHIDEventTap = 0
            CFRelease(key_down);
        }

        // Small delay between key down and up
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Key up
        let key_up = CGEventCreateKeyboardEvent(std::ptr::null_mut(), K_VK_V, false);
        if !key_up.is_null() {
            CGEventSetFlags(key_up, K_CG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(0, key_up);
            CFRelease(key_up);
        }
    }
}
