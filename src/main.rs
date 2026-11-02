//! `ryu-predict` — the system-wide predictive-typing companion entrypoint.
//!
//! Windows: starts the engine (caret probe + Core loop + layered overlay +
//! Tab-swallow hook). Other platforms: a clear stub — the caret/hook mechanism is
//! Win32/UIA today (the macOS equivalent is AXTextMarker /
//! `kAXBoundsForRangeParameterizedAttribute`, not yet implemented).

mod client;
mod types;

#[cfg(windows)]
mod caret;
#[cfg(windows)]
mod engine;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod state;

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        engine::run()
    }
    #[cfg(not(windows))]
    {
        eprintln!(
            "ryu-predict targets Windows (UIA caret + WH_KEYBOARD_LL hook + layered overlay). \
             The macOS equivalent (AXTextMarker / kAXBoundsForRangeParameterizedAttribute) is \
             not yet implemented."
        );
        Ok(())
    }
}
