//! Core HTTP client. The companion only ever talks to its local Core node — it
//! never reaches a model/provider directly (that is the whole point: Core hands
//! the call to the Gateway). Core URL + token come from the environment, matching
//! the other Ryu clients: `RYU_CORE_URL` (default `http://127.0.0.1:7980`) and
//! `RYU_TOKEN` (the shared node token; optional for a tokenless local node).

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use crate::types::{CaretContext, PredictConfig, PredictResponse};

/// Resolved connection to the local Core node.
#[derive(Debug, Clone)]
pub struct CoreClient {
    base: String,
    token: Option<String>,
    http: reqwest::blocking::Client,
}

impl CoreClient {
    /// Build from the environment. Fails only if the HTTP client cannot be built.
    pub fn from_env() -> Result<Self> {
        let base = std::env::var("RYU_CORE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:7980".to_string());
        let token = std::env::var("RYU_TOKEN").ok().filter(|s| !s.is_empty());
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            http,
        })
    }

    fn auth(&self, req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    /// `GET /api/predict/config` — the current predictive-typing config.
    pub fn get_config(&self) -> Result<PredictConfig> {
        let url = format!("{}/api/predict/config", self.base);
        let resp = self
            .auth(self.http.get(&url))
            .send()
            .context("predict config request failed")?;
        let cfg = resp
            .error_for_status()
            .context("predict config returned an error status")?
            .json::<PredictConfig>()
            .context("predict config was not valid JSON")?;
        Ok(cfg)
    }

    /// `POST /api/predict/complete` — one inline suggestion for the caret context.
    /// Returns the (possibly empty) suggestion; an empty string means "nothing to
    /// suggest" or a Core-side refusal (secure field / app not allowed).
    pub fn complete(&self, ctx: &CaretContext) -> Result<PredictResponse> {
        let url = format!("{}/api/predict/complete", self.base);
        let body = json!({
            "context": ctx.before,
            "app": ctx.app,
            "control": ctx.control,
        });
        let resp = self
            .auth(self.http.post(&url).json(&body))
            .send()
            .context("predict complete request failed")?;
        let out = resp
            .error_for_status()
            .context("predict complete returned an error status")?
            .json::<PredictResponse>()
            .context("predict complete was not valid JSON")?;
        Ok(out)
    }
}
