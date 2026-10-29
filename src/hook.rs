//! Low-level keyboard hook. Swallows Tab (and notes Esc) ONLY while a suggestion
//! is visible, so the rest of the time Tab behaves normally. The callback does
//! the bare minimum — one atomic read + a `PostMessage` — because a slow
//! `WH_KEYBOARD_LL` callback is silently unhooked by Windows
//! (`LowLevelHooksTimeout`); the actual injection happens on the overlay thread.

#![cfg(windows)]

use std::sync::atomic::Ordering;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_TAB};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::state::{MSG_ACCEPT, MSG_HIDE, OVERLAY_HWND, SUGGESTION_ACTIVE};

fn overlay_hwnd() -> HWND {
    HWND(OVERLAY_HWND.load(Ordering::Relaxed) as *mut core::ffi::c_void)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && SUGGESTION_ACTIVE.load(Ordering::Relaxed) {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let is_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        if is_down {
            if kb.vkCode == VK_TAB.0 as u32 {
                let _ = PostMessageW(Some(overlay_hwnd()), MSG_ACCEPT, WPARAM(0), LPARAM(0));
                return LRESULT(1); // swallow: accept the suggestion, app sees no Tab
            }
            if kb.vkCode == VK_ESCAPE.0 as u32 {
                // Dismiss but let Esc pass through to the app.
                SUGGESTION_ACTIVE.store(false, Ordering::Relaxed);
                let _ = PostMessageW(Some(overlay_hwnd()), MSG_HIDE, WPARAM(0), LPARAM(0));
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Install the low-level keyboard hook on the calling thread (which must run a
/// message pump). Returns the hook handle to unhook on shutdown.
pub fn install() -> anyhow::Result<HHOOK> {
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0)? };
    Ok(hook)
}
