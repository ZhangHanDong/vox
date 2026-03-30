use crate::MacosError;
use crossbeam_channel::Receiver;
use makepad_objc_sys::runtime::{Class, Object, Sel, YES, NO};
use makepad_objc_sys::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;
use std::sync::Once;

type ObjcId = *mut Object;

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub title: String,
    pub action_id: u64,
    pub enabled: bool,
    pub checked: bool,
    pub submenu: Option<Vec<MenuItem>>,
}

impl MenuItem {
    pub fn new(title: &str, action_id: u64) -> Self {
        Self { title: title.to_string(), action_id, enabled: true, checked: false, submenu: None }
    }
    pub fn separator() -> Self {
        Self { title: String::new(), action_id: 0, enabled: false, checked: false, submenu: None }
    }
    pub fn submenu(title: &str, items: Vec<MenuItem>) -> Self {
        Self { title: title.to_string(), action_id: 0, enabled: true, checked: false, submenu: Some(items) }
    }
}

static MENU_TX: std::sync::OnceLock<crossbeam_channel::Sender<u64>> = std::sync::OnceLock::new();

fn str_to_nsstring(s: &str) -> ObjcId {
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), alloc];
        let ns_string: ObjcId = msg_send![
            ns_string,
            initWithBytes: s.as_ptr() as *const c_void
            length: s.len()
            encoding: 4u64 // NSUTF8StringEncoding
        ];
        ns_string
    }
}

fn ensure_menu_target_class() -> *const Class {
    static REGISTER: Once = Once::new();
    static mut TARGET_CLASS: *const Class = std::ptr::null();

    REGISTER.call_once(|| {
        unsafe {
            let superclass = class!(NSObject);
            let mut decl = makepad_objc_sys::declare::ClassDecl::new("VoiceInputMenuTarget", superclass).unwrap();

            // Read action_id from sender's tag, not from ivar
            extern "C" fn menu_action(_this: &Object, _sel: Sel, sender: ObjcId) {
                unsafe {
                    let tag: i64 = msg_send![sender, tag];
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/voice_input_debug.log") {
                        let _ = writeln!(f, "MENU: tag={}", tag);
                    }
                    if let Some(tx) = MENU_TX.get() {
                        let _ = tx.try_send(tag as u64);
                    }
                }
            }

            decl.add_method(
                sel!(menuAction:),
                menu_action as extern "C" fn(&Object, Sel, ObjcId),
            );

            TARGET_CLASS = decl.register();
        }
    });

    unsafe { TARGET_CLASS }
}

/// Single global target instance, retained forever.
fn global_menu_target() -> ObjcId {
    static mut TARGET: ObjcId = std::ptr::null_mut();
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe {
            let cls = ensure_menu_target_class();
            let obj: ObjcId = msg_send![cls, new];
            let () = msg_send![obj, retain];
            TARGET = obj;
        }
    });
    unsafe { TARGET }
}

pub struct StatusBarHandle {
    item: ObjcId,
}

unsafe impl Send for StatusBarHandle {}

pub fn create_status_bar(
    _icon_png_data: &[u8],
    menu_items: Vec<MenuItem>,
) -> Result<(StatusBarHandle, Receiver<u64>), MacosError> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let _ = MENU_TX.set(tx);

    unsafe {
        let status_bar: ObjcId = msg_send![class!(NSStatusBar), systemStatusBar];
        let item: ObjcId = msg_send![status_bar, statusItemWithLength: -1.0f64];

        // Set title via button
        let button: ObjcId = msg_send![item, button];
        if !button.is_null() {
            let title = str_to_nsstring("MIC");
            let () = msg_send![button, setTitle: title];
        } else {
            // Fallback: use deprecated setTitle on item directly
            let title = str_to_nsstring("MIC");
            let () = msg_send![item, setTitle: title];
        }

        // Build and set menu
        let menu = build_ns_menu(&menu_items);
        let () = msg_send![item, setMenu: menu];

        // Retain
        let () = msg_send![item, retain];

        Ok((StatusBarHandle { item }, rx))
    }
}

unsafe fn build_ns_menu(items: &[MenuItem]) -> ObjcId {
    let menu: ObjcId = msg_send![class!(NSMenu), new];
    let () = msg_send![menu, setAutoenablesItems: NO];
    let target = global_menu_target();

    for item in items {
        if item.title.is_empty() && item.action_id == 0 && item.submenu.is_none() {
            let sep: ObjcId = msg_send![class!(NSMenuItem), separatorItem];
            let () = msg_send![menu, addItem: sep];
            continue;
        }

        let title = str_to_nsstring(&item.title);

        if let Some(ref sub_items) = item.submenu {
            let sub_item: ObjcId = msg_send![
                menu,
                addItemWithTitle: title
                action: std::ptr::null::<Sel>()
                keyEquivalent: str_to_nsstring("")
            ];
            let submenu = build_ns_menu(sub_items);
            let () = msg_send![submenu, setTitle: title];
            let () = msg_send![menu, setSubmenu: submenu forItem: sub_item];
        } else {
            let ns_item: ObjcId = msg_send![
                menu,
                addItemWithTitle: title
                action: sel!(menuAction:)
                keyEquivalent: str_to_nsstring("")
            ];

            // Use global singleton target + tag for action_id
            let () = msg_send![ns_item, setTarget: target];
            let () = msg_send![ns_item, setTag: item.action_id as i64];
            let () = msg_send![ns_item, setEnabled: if item.enabled { YES } else { NO }];

            if item.checked {
                let () = msg_send![ns_item, setState: 1i64];
            }
        }
    }
    menu
}

pub fn update_menu(handle: &StatusBarHandle, menu_items: Vec<MenuItem>) {
    unsafe {
        let menu = build_ns_menu(&menu_items);
        let () = msg_send![handle.item, setMenu: menu];
    }
}

pub fn set_status_bar_icon(handle: &StatusBarHandle, title: &str) {
    unsafe {
        let button: ObjcId = msg_send![handle.item, button];
        if !button.is_null() {
            let ns_title = str_to_nsstring(title);
            let () = msg_send![button, setTitle: ns_title];
        }
    }
}

impl Drop for StatusBarHandle {
    fn drop(&mut self) {
        unsafe {
            let status_bar: ObjcId = msg_send![class!(NSStatusBar), systemStatusBar];
            let () = msg_send![status_bar, removeStatusItem: self.item];
            let () = msg_send![self.item, release];
        }
    }
}
