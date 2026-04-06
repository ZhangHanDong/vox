use makepad_objc_sys::runtime::Object;
use makepad_objc_sys::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;

type ObjcId = *mut Object;

fn str_to_nsstring(s: &str) -> ObjcId {
    // SAFETY: alloc+initWithBytes creates a retained NSString from valid UTF-8 bytes.
    // Autoreleased to prevent leak; caller must use within current autorelease pool scope.
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), alloc];
        let ns_string: ObjcId = msg_send![
            ns_string,
            initWithBytes: s.as_ptr() as *const c_void
            length: s.len()
            encoding: 4u64
        ];
        // Autorelease to prevent leak (alloc+init gives +1 retain)
        let _: ObjcId = msg_send![ns_string, autorelease];
        ns_string
    }
}

/// SAFETY: Caller must pass a valid NSString pointer (or null, which is handled).
/// UTF8String returns an internal buffer valid for the NSString's lifetime.
/// We copy into a Rust String immediately, so no dangling reference.
unsafe fn nsstring_to_rust(ns_str: ObjcId) -> Option<String> {
    if ns_str.is_null() {
        return None;
    }
    let utf8: *const u8 = msg_send![ns_str, UTF8String];
    if utf8.is_null() {
        return None;
    }
    let c_str = std::ffi::CStr::from_ptr(utf8 as *const std::ffi::c_char);
    c_str.to_str().ok().map(|s| s.to_string())
}

/// Read the current clipboard text content.
pub fn read_clipboard() -> Option<String> {
    // SAFETY: NSPasteboard class methods are safe to call; generalPasteboard is null-checked.
    // stringForType: returns nil (handled by nsstring_to_rust) or a valid NSString.
    unsafe {
        let pb: ObjcId = msg_send![class!(NSPasteboard), generalPasteboard];
        if pb.is_null() {
            return None;
        }
        let ns_type = str_to_nsstring("public.utf8-plain-text");
        let ns_str: ObjcId = msg_send![pb, stringForType: ns_type];
        nsstring_to_rust(ns_str)
    }
}

/// Write text to the clipboard.
pub fn write_clipboard(text: &str) {
    // SAFETY: generalPasteboard is null-checked. clearContents + setString:forType:
    // are standard NSPasteboard API; str_to_nsstring creates valid autoreleased NSStrings.
    unsafe {
        let pb: ObjcId = msg_send![class!(NSPasteboard), generalPasteboard];
        if pb.is_null() {
            return;
        }
        let () = msg_send![pb, clearContents];
        let ns_str = str_to_nsstring(text);
        let ns_type = str_to_nsstring("public.utf8-plain-text");
        let _: bool = msg_send![pb, setString: ns_str forType: ns_type];
    }
}
