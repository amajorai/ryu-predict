//! Shared, platform-agnostic types for the predictive-typing companion.

use serde::Deserialize;

/// The predictive-typing config as Core normalizes it (`GET /api/predict/config`).
/// Mirrors `ryu_core::predict::PredictConfig` — only the fields the companion
/// needs are read; unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct PredictConfig {
    #[serde(default, rename = "debounceMs")]
    pub debounce_ms: u64,
    /// Per-app allowlist — the companion forwards `app`/`control` and Core
    /// enforces this, so the companion does not need to read it for correctness.
    /// Kept for potential client-side short-circuiting / display.
    #[serde(default, rename = "appAllowlist")]
    pub app_allowlist: Vec<String>,
}

impl Default for PredictConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 400,
            app_allowlist: Vec::new(),
        }
    }
}

/// What the OS told us about the focused caret. `before` is the text from the
/// document start to the caret (the model context); `rect` is the caret's screen
/// rectangle `(x, y, w, h)` to anchor ghost text against. `app` is the focused
/// process name and `control` its localized control type (for Core's allowlist +
/// secure-field denylist).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaretContext {
    pub app: String,
    pub control: String,
    pub before: String,
    pub rect: Option<(i32, i32, i32, i32)>,
}

impl CaretContext {
    /// Stable key for debounce/dedup — re-requesting the same context is wasteful.
    pub fn dedup_key(&self) -> String {
        format!("{}|{}|{}", self.app, self.control, self.before)
    }

    /// Whether this context is worth sending to Core: a non-secure field with a
    /// caret rect and some preceding text. (Core re-checks all of this; this is
    /// just a cheap local short-circuit to avoid pointless round-trips.)
    pub fn is_requestable(&self) -> bool {
        self.rect.is_some() && !self.before.trim().is_empty()
    }
}

/// The prediction reply from `POST /api/predict/complete`.
#[derive(Debug, Clone, Deserialize)]
pub struct PredictResponse {
    #[serde(default)]
    pub suggestion: String,
    #[serde(default)]
    pub reason: Option<String>,
}
