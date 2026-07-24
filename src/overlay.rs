//! The ghost-text overlay: a layered, always-on-top, click-through, no-activate
//! tool window that paints the suggestion in grey just below the caret. Black is
//! the color key (fully transparent), so only the text shows. Rendering here is
//! intentionally minimal GDI (`DrawTextW`); a Tauri/webview surface could replace
//! it later for richer styling — the engine and the window are decoupled by the
//! `MSG_SHOW`/`MSG_HIDE`/`MSG_ACCEPT` messages, nothing else.

#![cfg(windows)]

use std::sync::atomic::Ordering;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect,
    SetBkMode, SetTextColor, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, PostQuitMessage, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, CW_USEDEFAULT, HWND_TOPMOST,
    LWA_COLORKEY, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_DESTROY, WM_PAINT, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::state::{shared, MSG_ACCEPT, MSG_HIDE, MSG_SHOW, OVERLAY_HWND, SUGGESTION_ACTIVE};

const CLASS_NAME: &str = "RyuPredictOverlay";
/// Grey ghost text. `COLORREF` is `0x00BBGGRR`.
const GHOST_COLOR: u32 = 0x0090_9090;
/// Overlay height in px (single line).
const OVERLAY_HEIGHT: i32 = 22;
/// Rough px-per-char estimate to size the window to the suggestion.
const PX_PER_CHAR: i32 = 8;
const MAX_WIDTH: i32 = 640;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        MSG_SHOW => {
            show_at_caret(hwnd);
            LRESULT(0)
        }
        MSG_HIDE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        MSG_ACCEPT => {
            accept(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);

    // Fill with the color-key (black) → fully transparent background.
    let brush = CreateSolidBrush(COLORREF(0));
    FillRect(hdc, &rc, brush);
    let _ = DeleteObject(brush.into());

    let text = {
        let s = shared().lock().unwrap();
        s.suggestion.clone()
    };
    if !text.is_empty() {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(GHOST_COLOR));
        let mut buf = wide(&text);
        let mut tr = RECT {
            left: rc.left + 4,
            top: rc.top,
            right: rc.right,
            bottom: rc.bottom,
        };
        // `buf` includes a trailing null; drop it for DrawTextW's length.
        let len = buf.len().saturating_sub(1);
        DrawTextW(
            hdc,
            &mut buf[..len],
            &mut tr,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
    }

    let _ = EndPaint(hwnd, &ps);
}

unsafe fn show_at_caret(hwnd: HWND) {
    let (rect, len) = {
        let s = shared().lock().unwrap();
        (s.rect, s.suggestion.chars().count() as i32)
    };
    let Some((x, y, _w, h)) = rect else {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
    };
    let width = (len * PX_PER_CHAR + 16).clamp(40, MAX_WIDTH);
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        x,
        y + h, // just below the caret line
        width,
        OVERLAY_HEIGHT,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    let _ = InvalidateRect(Some(hwnd), None, true);
}

unsafe fn accept(hwnd: HWND) {
    let text = {
        let mut s = shared().lock().unwrap();
        let t = std::mem::take(&mut s.suggestion);
        s.rect = None;
        t
    };
    SUGGESTION_ACTIVE.store(false, Ordering::Relaxed);
    let _ = ShowWindow(hwnd, SW_HIDE);
    if !text.is_empty() {
        // Inject the accepted suggestion via SendInput (shared with ghost).
        if let Err(e) = ghost_hands::type_text(&text, false) {
            eprintln!("predict: inject failed: {e}");
        }
    }
}

/// Register the window class + create the (hidden) overlay window. Publishes the
/// HWND into [`OVERLAY_HWND`] so the worker/hook can post to it.
pub fn create() -> anyhow::Result<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // Non-zero atom on success; 0 means the class failed to register.
        if RegisterClassW(&wc) == 0 {
            anyhow::bail!("RegisterClassW failed for the overlay window class");
        }

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            PCWSTR(wide("Ryu Predict").as_ptr()),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            10,
            OVERLAY_HEIGHT,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;

        // Black is the color key → transparent; only the grey text paints.
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY)?;

        OVERLAY_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        Ok(hwnd)
    }
}
