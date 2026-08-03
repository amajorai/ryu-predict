//! Process-global state shared between the engine worker (UIA + HTTP), the
//! keyboard hook, and the overlay window proc — all of which are `extern "system"`
//! callbacks or separate threads, so the channel between them is necessarily a
//! set of statics. Kept tiny and explicit.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicIsize};
use std::sync::{Mutex, OnceLock};

use windows::Win32::UI::WindowsAndMessaging::WM_USER;

/// Worker → overlay: show the current suggestion at the stored rect.
pub const MSG_SHOW: u32 = WM_USER + 1;
/// Worker/hook → overlay: hide the ghost text.
pub const MSG_HIDE: u32 = WM_USER + 2;
/// Hook → overlay: the user pressed Tab — accept (inject) the suggestion.
pub const MSG_ACCEPT: u32 = WM_USER + 3;

/// True while a suggestion is visible. The keyboard hook reads ONLY this (one
/// atomic load — anything heavier in the LL hook callback gets it unhooked), so
/// Tab is swallowed only when there is actually a suggestion to accept.
pub static SUGGESTION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The overlay window handle as an `isize` (HWND is not `Send`; we pass the raw
/// pointer value across threads and rebuild the HWND at the call site).
pub static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

/// The current suggestion text + the caret rect to anchor it against.
#[derive(Default)]
pub struct Shared {
    pub suggestion: String,
    pub rect: Option<(i32, i32, i32, i32)>,
}

/// The single shared-suggestion slot.
pub fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Shared::default()))
}
