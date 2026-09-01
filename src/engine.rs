//! The engine: ties the caret probe, Core, the overlay, and the hook together.
//!
//! - A worker thread (its own COM apartment) polls the caret, debounces, and asks
//!   Core for a suggestion; on a hit it stores the text + rect and posts
//!   `MSG_SHOW` to the overlay. Heavy work (UIA + HTTP) stays OFF the UI thread so
//!   the keyboard hook never stalls.
//! - The main thread owns the overlay window + the keyboard hook and runs the
//!   message pump that services both (paint, show/hide, and Tab→accept→inject).

#![cfg(windows)]

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostMessageW, TranslateMessage, UnhookWindowsHookEx, MSG,
};

use crate::caret::CaretProbe;
use crate::client::CoreClient;
use crate::state::{shared, MSG_HIDE, MSG_SHOW, OVERLAY_HWND, SUGGESTION_ACTIVE};
use crate::{hook, overlay};

/// How often the worker samples the caret.
const POLL: Duration = Duration::from_millis(120);
/// Re-read the config from Core roughly every ~5s (40 × 120ms).
const CONFIG_REFRESH_TICKS: u32 = 40;

/// Start the companion. Blocks on the message pump until the overlay window is
/// destroyed (WM_QUIT).
pub fn run() -> anyhow::Result<()> {
    let client = CoreClient::from_env()?;
    overlay::create()?;
    let installed_hook = hook::install()?;

    std::thread::spawn(move || {
        if let Err(e) = worker(client) {
            eprintln!("predict: worker exited: {e}");
        }
    });

    unsafe {
        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 == 0 || r.0 == -1 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(installed_hook);
    }
    Ok(())
}

fn overlay_hwnd() -> HWND {
    HWND(OVERLAY_HWND.load(Ordering::Relaxed) as *mut core::ffi::c_void)
}

fn post(msg: u32) {
    unsafe {
        let _ = PostMessageW(Some(overlay_hwnd()), msg, WPARAM(0), LPARAM(0));
    }
}

/// Hide any visible suggestion and report whether one was showing.
fn clear_suggestion() {
    if SUGGESTION_ACTIVE.swap(false, Ordering::Relaxed) {
        post(MSG_HIDE);
    }
}

fn worker(client: CoreClient) -> anyhow::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let probe = CaretProbe::new()?;

    let mut cfg = client.get_config().unwrap_or_default();
    let mut ticks: u32 = 0;
    let mut last_key = String::new();
    let mut last_change = Instant::now();
    let mut requested_key = String::new();

    loop {
        std::thread::sleep(POLL);
        ticks = ticks.wrapping_add(1);
        if ticks % CONFIG_REFRESH_TICKS == 0 {
            if let Ok(fresh) = client.get_config() {
                cfg = fresh;
            }
        }

        // The `predict` plugin (Core) is the single on/off switch: when it is
        // disabled, Core's `/api/predict/complete` returns an empty suggestion
        // (reason "predictive typing plugin is disabled"), which the request path
        // below already renders as "nothing to show". No client-side enable flag.
        let Some(ctx) = probe.probe().filter(|c| c.is_requestable()) else {
            clear_suggestion();
            last_key.clear();
            requested_key.clear();
            continue;
        };

        let key = ctx.dedup_key();
        if key != last_key {
            // Context moved: reset the debounce and drop the stale suggestion.
            last_key = key;
            last_change = Instant::now();
            clear_suggestion();
            continue;
        }

        let debounce = Duration::from_millis(cfg.debounce_ms.max(1));
        if key == requested_key || last_change.elapsed() < debounce {
            continue;
        }
        requested_key = key;

        match client.complete(&ctx) {
            Ok(resp) => {
                let suggestion = resp.suggestion.trim().to_string();
                if suggestion.is_empty() {
                    clear_suggestion();
                } else {
                    {
                        let mut sh = shared().lock().unwrap();
                        sh.suggestion = suggestion;
                        sh.rect = ctx.rect;
                    }
                    SUGGESTION_ACTIVE.store(true, Ordering::Relaxed);
                    post(MSG_SHOW);
                }
            }
            Err(e) => eprintln!("predict: complete failed: {e}"),
        }
    }
}
