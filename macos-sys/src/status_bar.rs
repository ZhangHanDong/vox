use crate::MacosError;
use crossbeam_channel::Receiver;
use makepad_objc_sys::runtime::{Class, Object, Sel, YES, NO};
use makepad_objc_sys::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;

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
    // SAFETY: alloc+initWithBytes from valid UTF-8, autoreleased to prevent leak.
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), alloc];
        let ns_string: ObjcId = msg_send![
            ns_string,
            initWithBytes: s.as_ptr() as *const c_void
            length: s.len()
            encoding: 4u64 // NSUTF8StringEncoding
        ];
        let _: ObjcId = msg_send![ns_string, autorelease];
        ns_string
    }
}

/// ObjC class registered once, stored as usize for Send+Sync safety.
fn ensure_menu_target_class() -> *const Class {
    static TARGET_CLASS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

    let ptr = TARGET_CLASS.get_or_init(|| {
        // SAFETY: Registering a new ObjC class with the runtime. This is safe because:
        // - ClassDecl::new returns None if the class already exists
        // - The method signature matches extern "C" fn(&Object, Sel, ObjcId)
        // - menu_action wraps its body in catch_unwind to prevent panic crossing FFI
        unsafe {
            let superclass = class!(NSObject);
            let mut decl = makepad_objc_sys::declare::ClassDecl::new("VoiceInputMenuTarget", superclass).unwrap();

            extern "C" fn menu_action(_this: &Object, _sel: Sel, sender: ObjcId) {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // SAFETY: msg_send![sender, tag] returns 0 if sender is nil (ObjC nil-messaging).
                    let tag: i64 = unsafe { msg_send![sender, tag] };
                    if let Some(tx) = MENU_TX.get() {
                        let _ = tx.try_send(tag as u64);
                    }
                }));
            }

            decl.add_method(
                sel!(menuAction:),
                menu_action as extern "C" fn(&Object, Sel, ObjcId),
            );

            decl.register() as *const Class as usize
        }
    });
    *ptr as *const Class
}

/// Global singleton menu target instance, retained forever. Stored as usize for Send+Sync.
fn global_menu_target() -> ObjcId {
    static TARGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

    let ptr = TARGET.get_or_init(|| {
        // SAFETY: Creates one ObjC object of our registered class. Retained to prevent dealloc.
        unsafe {
            let cls = ensure_menu_target_class();
            let obj: ObjcId = msg_send![cls, new];
            let () = msg_send![obj, retain];
            obj as usize
        }
    });
    *ptr as ObjcId
}

/// Handle to the macOS status bar item.
///
/// NOT Send — NSStatusItem must only be accessed from the main thread (AppKit requirement).
/// All methods (update_menu, set_status_bar_icon, Drop) send ObjC messages that are
/// only safe on the main thread.
pub struct StatusBarHandle {
    item: ObjcId,
}

pub fn create_status_bar(
    _icon_png_data: &[u8],
    menu_items: Vec<MenuItem>,
) -> Result<(StatusBarHandle, Receiver<u64>), MacosError> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let _ = MENU_TX.set(tx);

    // SAFETY: NSStatusBar.systemStatusBar returns a singleton (never null).
    // statusItemWithLength:-1 creates a variable-width item. item is retained
    // via explicit retain call below; released in Drop.
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

/// SAFETY: Caller must ensure this runs on the main thread (AppKit requirement).
/// All NSMenu/NSMenuItem allocations are autoreleased. The global_menu_target()
/// singleton is retained forever and safe to reference from any menu item.
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
    // SAFETY: setMenu: on a valid NSStatusItem. Old menu is released by AppKit.
    unsafe {
        let menu = build_ns_menu(&menu_items);
        let () = msg_send![handle.item, setMenu: menu];
    }
}

pub fn set_status_bar_icon(handle: &StatusBarHandle, title: &str) {
    // SAFETY: button returns nil if no button (null-checked). setTitle: accepts valid NSString.
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
        // SAFETY: removeStatusItem: detaches from status bar. release balances
        // the retain in create_status_bar. Must run on main thread (enforced by
        // StatusBarHandle not being Send).
        unsafe {
            let status_bar: ObjcId = msg_send![class!(NSStatusBar), systemStatusBar];
            let () = msg_send![status_bar, removeStatusItem: self.item];
            let () = msg_send![self.item, release];
        }
    }
}
