#![warn(clippy::all)]

#[cfg(target_os = "macos")]
pub mod event_tap;
#[cfg(target_os = "macos")]
pub mod status_bar;
#[cfg(target_os = "macos")]
pub mod clipboard;
#[cfg(target_os = "macos")]
pub mod input_source;
#[cfg(target_os = "macos")]
pub mod key_inject;

mod error;
pub use error::MacosError;
