//! Windows caret probe (UIA `TextPattern`). Lifted from `predict-spike`'s proven
//! `run_probe`, refactored into a reusable [`CaretProbe`] that returns a
//! [`CaretContext`]. Must be constructed + called on a COM-initialized thread
//! (the engine's worker thread does `CoInitializeEx` once, then reuses one probe).
//!
//! The mechanism, unchanged from the spike: focus → `GetFocusedElement` →
//! `TextPattern` → `GetSelection` → `GetBoundingRectangles` for the caret rect,
//! and `MoveEndpointByRange(document-start → caret)` + `GetText` for the
//! preceding context. Falls back to the element bounding rect when the field
//! exposes no caret rect.

#![cfg(windows)]

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, SAFEARRAY};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange,
    TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::types::CaretContext;

/// A reusable focused-caret probe. Holds one `IUIAutomation` instance.
pub struct CaretProbe {
    automation: IUIAutomation,
}

impl CaretProbe {
    /// Create the probe. The calling thread must already be COM-initialized.
    pub fn new() -> anyhow::Result<Self> {
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
        Ok(Self { automation })
    }

    /// Probe the currently focused element. Returns `None` when there is no
    /// focused text element we can read.
    pub fn probe(&self) -> Option<CaretContext> {
        unsafe {
            let hwnd = GetForegroundWindow();
            let app = process_name(hwnd);

            let element = self.automation.GetFocusedElement().ok()?;
            let control = element
                .CurrentLocalizedControlType()
                .map(|s| s.to_string())
                .unwrap_or_else(|_| "element".into());

            let text_pattern = element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .ok();

            let mut caret = None;
            let mut before = None;
            if let Some(tp) = text_pattern.as_ref() {
                if let Ok(sel) = tp.GetSelection() {
                    if sel.Length().unwrap_or(0) > 0 {
                        if let Ok(range) = sel.GetElement(0) {
                            caret = caret_rect(&range);
                            if let Ok(doc) = tp.DocumentRange() {
                                if let Ok(ctx) = range.Clone() {
                                    let _ = ctx.MoveEndpointByRange(
                                        TextPatternRangeEndpoint_Start,
                                        &doc,
                                        TextPatternRangeEndpoint_Start,
                                    );
                                    before = ctx.GetText(4096).ok().map(|s| s.to_string());
                                }
                            }
                        }
                    }
                }
            }

            // Fall back to the element bounding rect when no caret rect exists,
            // so the overlay can still anchor near the field (popup fallback).
            let rect = caret.or_else(|| {
                element
                    .CurrentBoundingRectangle()
                    .ok()
                    .map(|r| (r.left, r.top, r.right - r.left, r.bottom - r.top))
            });

            // Nothing to predict from without preceding text.
            let before = before?;

            Some(CaretContext {
                app,
                control,
                before,
                rect,
            })
        }
    }
}

/// Read a `SAFEARRAY` of doubles (UIA rects come as `[l,t,w,h, ...]`).
unsafe fn read_f64_safearray(psa: *mut SAFEARRAY) -> Vec<f64> {
    if psa.is_null() {
        return Vec::new();
    }
    let (Ok(lb), Ok(ub)) = (SafeArrayGetLBound(psa, 1), SafeArrayGetUBound(psa, 1)) else {
        let _ = SafeArrayDestroy(psa);
        return Vec::new();
    };
    let count = (ub - lb + 1).max(0) as usize;
    let mut out = Vec::with_capacity(count);
    let mut data: *mut c_void = std::ptr::null_mut();
    if SafeArrayAccessData(psa, &mut data).is_ok() && !data.is_null() {
        let slice = std::slice::from_raw_parts(data as *const f64, count);
        out.extend_from_slice(slice);
        let _ = SafeArrayUnaccessData(psa);
    }
    let _ = SafeArrayDestroy(psa);
    out
}

/// First `[l,t,w,h]` rect of a range, expanding a degenerate caret to one char.
unsafe fn caret_rect(range: &IUIAutomationTextRange) -> Option<(i32, i32, i32, i32)> {
    let to_i32 = |r: &[f64]| (r[0] as i32, r[1] as i32, r[2] as i32, r[3] as i32);
    if let Ok(psa) = range.GetBoundingRectangles() {
        let rects = read_f64_safearray(psa);
        if rects.len() >= 4 {
            return Some(to_i32(&rects));
        }
    }
    if let Ok(c) = range.Clone() {
        if c.ExpandToEnclosingUnit(TextUnit_Character).is_ok() {
            if let Ok(psa) = c.GetBoundingRectangles() {
                let rects = read_f64_safearray(psa);
                if rects.len() >= 4 {
                    return Some(to_i32(&rects));
                }
            }
        }
    }
    None
}

/// The focused window's process file name (e.g. `chrome.exe`), or a best-effort
/// fallback. Used for Core's per-app allowlist.
unsafe fn process_name(hwnd: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return String::new();
    }
    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return format!("pid:{pid}");
    };
    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let full = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        windows::core::PWSTR(buf.as_mut_ptr()),
        &mut size,
    );
    let _ = CloseHandle(handle);
    if full.is_err() {
        return format!("pid:{pid}");
    }
    let path = String::from_utf16_lossy(&buf[..size as usize]);
    path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string()
}
