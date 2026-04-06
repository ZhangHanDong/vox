use crate::MacosError;
use makepad_objc_sys::runtime::Object;
use makepad_objc_sys::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;

type ObjcId = *mut Object;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, key: *mut c_void) -> *mut c_void;
    fn TISCopyInputSourceForLanguage(language: *mut c_void) -> *mut c_void;
    fn TISSelectInputSource(source: *mut c_void) -> i32;
    fn TISCreateInputSourceList(properties: *mut c_void, include_all: i32) -> *mut c_void;
    fn CFRelease(cf: *mut c_void);
    static kTISPropertyInputSourceID: *mut c_void;
}

fn str_to_nsstring(s: &str) -> ObjcId {
    // SAFETY: alloc+initWithBytes from valid UTF-8, autoreleased to prevent leak.
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), alloc];
        let ns_string: ObjcId = msg_send![
            ns_string,
            initWithBytes: s.as_ptr() as *const c_void
            length: s.len()
            encoding: 4u64
        ];
        let _: ObjcId = msg_send![ns_string, autorelease];
        ns_string
    }
}

/// SAFETY: cf_str must be a valid CFStringRef (toll-free bridged to NSString) or null.
/// Null is handled. UTF8String buffer is copied immediately into Rust String.
unsafe fn cfstring_to_rust(cf_str: *mut c_void) -> Option<String> {
    if cf_str.is_null() {
        return None;
    }
    let ns_str = cf_str as ObjcId; // toll-free bridged
    let utf8: *const u8 = msg_send![ns_str, UTF8String];
    if utf8.is_null() {
        return None;
    }
    let c_str = std::ffi::CStr::from_ptr(utf8 as *const std::ffi::c_char);
    c_str.to_str().ok().map(|s| s.to_string())
}

pub fn current_input_source_id() -> String {
    // SAFETY: TISCopy returns owned ref (CFRelease'd below). TISGetInputSourceProperty
    // returns non-owned ref (no release needed). Null checks on both.
    unsafe {
        let source = TISCopyCurrentKeyboardInputSource();
        if source.is_null() { return String::new(); }
        let id_cf = TISGetInputSourceProperty(source, kTISPropertyInputSourceID);
        let result = cfstring_to_rust(id_cf).unwrap_or_default();
        CFRelease(source);
        result
    }
}

pub fn is_cjk_input_source() -> bool {
    let id = current_input_source_id();
    let lower = id.to_lowercase();
    lower.contains("chinese") || lower.contains("scim") || lower.contains("tcim")
        || lower.contains("japanese") || lower.contains("korean")
        || lower.contains("pinyin") || lower.contains("wubi") || lower.contains("cangjie")
        || lower.contains("zhuyin") || lower.contains("sogou") || lower.contains("baidu")
        || lower.contains("rime") || lower.contains("squirrel")
}

pub fn select_ascii_input_source() -> Result<(), MacosError> {
    // SAFETY: TISCopyInputSourceForLanguage returns owned ref (CFRelease'd).
    // TISSelectInputSource is safe to call with a valid source ref.
    unsafe {
        let lang = str_to_nsstring("en") as *mut c_void;
        let source = TISCopyInputSourceForLanguage(lang);
        if source.is_null() {
            return Err(MacosError::InputSourceNotFound("No ASCII input source for 'en'".into()));
        }
        let result = TISSelectInputSource(source);
        CFRelease(source);
        if result != 0 {
            return Err(MacosError::InputSourceSwitchFailed(format!("TISSelectInputSource returned {result}")));
        }
        Ok(())
    }
}

pub fn select_input_source(id: &str) -> Result<(), MacosError> {
    // SAFETY: TISCreateInputSourceList returns owned CFArray (CFRelease'd on both paths).
    // objectAtIndex:0 is bounds-checked (count > 0 verified).
    unsafe {
        let criteria = str_to_nsstring(id) as *mut c_void;
        let key = kTISPropertyInputSourceID;
        let dict: ObjcId = msg_send![class!(NSDictionary), dictionaryWithObject: criteria forKey: key];
        let sources = TISCreateInputSourceList(dict as *mut c_void, 0);
        if sources.is_null() {
            return Err(MacosError::InputSourceNotFound(id.to_string()));
        }
        let sources_ns = sources as ObjcId;
        let count: usize = msg_send![sources_ns, count];
        if count == 0 {
            CFRelease(sources);
            return Err(MacosError::InputSourceNotFound(id.to_string()));
        }
        let source: *mut c_void = msg_send![sources_ns, objectAtIndex: 0usize];
        let result = TISSelectInputSource(source);
        CFRelease(sources);
        if result != 0 {
            return Err(MacosError::InputSourceSwitchFailed(format!("TISSelectInputSource returned {result} for '{id}'")));
        }
        Ok(())
    }
}
