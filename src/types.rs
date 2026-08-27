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

#[cfg(test)]
mod tests {
    use super::*;

    // --- PredictConfig ---------------------------------------------------

    #[test]
    fn predict_config_default_matches_core_debounce() {
        let cfg = PredictConfig::default();
        assert_eq!(cfg.debounce_ms, 400);
        assert!(cfg.app_allowlist.is_empty());
    }

    #[test]
    fn predict_config_deserializes_camelcase_fields() {
        let cfg: PredictConfig =
            serde_json::from_str(r#"{"debounceMs": 250, "appAllowlist": ["code.exe", "notepad"]}"#)
                .expect("valid config JSON should deserialize");
        assert_eq!(cfg.debounce_ms, 250);
        assert_eq!(cfg.app_allowlist, vec!["code.exe", "notepad"]);
    }

    #[test]
    fn predict_config_ignores_unknown_and_defaults_missing() {
        // Missing fields fall back to serde defaults (0 / empty), and unknown
        // fields Core may add are ignored — the companion only reads what it needs.
        let cfg: PredictConfig =
            serde_json::from_str(r#"{"futureFieldCoreAdds": true}"#).expect("should deserialize");
        assert_eq!(cfg.debounce_ms, 0);
        assert!(cfg.app_allowlist.is_empty());
    }

    // --- CaretContext::dedup_key ----------------------------------------

    #[test]
    fn dedup_key_is_stable_and_combines_all_fields() {
        let ctx = CaretContext {
            app: "code.exe".into(),
            control: "Edit".into(),
            before: "hello wor".into(),
            rect: Some((1, 2, 3, 4)),
        };
        // Rect is deliberately excluded — it moves as the caret does but the
        // context is unchanged, so the key must not depend on it.
        assert_eq!(ctx.dedup_key(), "code.exe|Edit|hello wor");
        let mut moved = ctx.clone();
        moved.rect = Some((99, 99, 3, 4));
        assert_eq!(ctx.dedup_key(), moved.dedup_key());
    }

    #[test]
    fn dedup_key_changes_when_typed_text_changes() {
        let base = CaretContext {
            app: "code.exe".into(),
            control: "Edit".into(),
            before: "hello wor".into(),
            rect: None,
        };
        let mut typed = base.clone();
        typed.before = "hello worl".into();
        assert_ne!(base.dedup_key(), typed.dedup_key());
    }

    // --- CaretContext::is_requestable -----------------------------------

    #[test]
    fn is_requestable_true_only_with_rect_and_nonblank_text() {
        let ok = CaretContext {
            app: "code.exe".into(),
            control: "Edit".into(),
            before: "type ".into(),
            rect: Some((0, 0, 1, 1)),
        };
        assert!(ok.is_requestable());
    }

    #[test]
    fn is_requestable_false_without_rect() {
        let no_rect = CaretContext {
            before: "type ".into(),
            rect: None,
            ..Default::default()
        };
        assert!(!no_rect.is_requestable());
    }

    #[test]
    fn is_requestable_false_when_text_blank_or_whitespace() {
        let empty = CaretContext {
            before: String::new(),
            rect: Some((0, 0, 1, 1)),
            ..Default::default()
        };
        assert!(!empty.is_requestable());

        let whitespace = CaretContext {
            before: "   \t\n".into(),
            rect: Some((0, 0, 1, 1)),
            ..Default::default()
        };
        assert!(!whitespace.is_requestable());
    }

    // --- PredictResponse ------------------------------------------------

    #[test]
    fn predict_response_defaults_when_fields_missing() {
        let resp: PredictResponse =
            serde_json::from_str("{}").expect("empty object should deserialize");
        assert_eq!(resp.suggestion, "");
        assert!(resp.reason.is_none());
    }

    #[test]
    fn predict_response_reads_suggestion_and_reason() {
        let resp: PredictResponse =
            serde_json::from_str(r#"{"suggestion": "ld!", "reason": "secure field"}"#)
                .expect("should deserialize");
        assert_eq!(resp.suggestion, "ld!");
        assert_eq!(resp.reason.as_deref(), Some("secure field"));
    }
}
