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
        let token = std::env::var("RYU_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Self::new(base, token, Duration::from_secs(20))
    }

    fn new(base: String, token: Option<String>, timeout: Duration) -> Result<Self> {
        let token = token.filter(|value| !value.trim().is_empty());
        let http = reqwest::blocking::Client::builder()
            // Core is a node-local control surface. Never send its bearer token
            // or caret context through a process-wide HTTP proxy.
            .no_proxy()
            .timeout(timeout)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base: base.trim().trim_end_matches('/').to_string(),
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

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    #[derive(Debug)]
    struct RecordedRequest {
        head: String,
        body: String,
    }

    fn read_request(mut stream: &TcpStream) -> RecordedRequest {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        let header_end = loop {
            let count = stream
                .read(&mut buffer)
                .expect("request should be readable");
            assert!(count > 0, "connection closed before request headers");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let head = String::from_utf8(bytes[..header_end].to_vec()).expect("ASCII request headers");
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| {
                    value
                        .trim()
                        .parse::<usize>()
                        .expect("numeric content length")
                })
            })
            .unwrap_or(0);
        while bytes.len() - header_end < content_length {
            let count = stream.read(&mut buffer).expect("body should be readable");
            assert!(count > 0, "connection closed before request body");
            bytes.extend_from_slice(&buffer[..count]);
        }
        RecordedRequest {
            head,
            body: String::from_utf8(bytes[header_end..header_end + content_length].to_vec())
                .expect("JSON request body"),
        }
    }

    fn serve(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (
        String,
        Arc<Mutex<Vec<RecordedRequest>>>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
        let address = listener.local_addr().expect("listener address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let task = thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("client connection");
                captured
                    .lock()
                    .expect("request lock")
                    .push(read_request(&stream));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("response should be writable");
            }
        });
        (format!("http://{address}"), requests, task)
    }

    fn context() -> CaretContext {
        CaretContext {
            app: "editor.exe".into(),
            control: "Edit \"safe\"".into(),
            before: "Hello\nworld".into(),
            rect: Some((1, 2, 3, 4)),
        }
    }

    #[test]
    fn config_uses_production_route_and_bearer_auth() {
        let (base, requests, task) = serve(vec![(
            "200 OK",
            r#"{"debounceMs":125,"appAllowlist":["editor.exe"]}"#,
        )]);
        let client = CoreClient::new(
            format!("  {base}/  "),
            Some("node-secret".into()),
            Duration::from_secs(2),
        )
        .expect("client");

        let config = client.get_config().expect("valid config response");
        task.join().expect("server task");

        assert_eq!(config.debounce_ms, 125);
        assert_eq!(config.app_allowlist, ["editor.exe"]);
        let requests = requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .head
            .starts_with("GET /api/predict/config HTTP/1.1"));
        assert!(requests[0]
            .head
            .to_ascii_lowercase()
            .contains("authorization: bearer node-secret"));
        assert!(requests[0].body.is_empty());
    }

    #[test]
    fn completion_sends_exact_context_and_does_not_retry_refusal() {
        let (base, requests, task) = serve(vec![("403 Forbidden", r#"{"error":"denied"}"#)]);
        let client = CoreClient::new(base, None, Duration::from_secs(2)).expect("client");

        let error = client
            .complete(&context())
            .expect_err("refusal must be visible");
        task.join().expect("server task");

        assert!(error.to_string().contains("error status"));
        let requests = requests.lock().expect("request lock");
        assert_eq!(
            requests.len(),
            1,
            "a refusal must not be retried implicitly"
        );
        assert!(requests[0]
            .head
            .starts_with("POST /api/predict/complete HTTP/1.1"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request JSON"),
            json!({
                "app": "editor.exe",
                "context": "Hello\nworld",
                "control": "Edit \"safe\"",
            })
        );
    }

    #[test]
    fn caller_controlled_retry_replays_exact_payload_after_transient_error() {
        let (base, requests, task) = serve(vec![
            ("503 Service Unavailable", r#"{"error":"busy"}"#),
            ("200 OK", r#"{"suggestion":" again"}"#),
        ]);
        let client = CoreClient::new(base, None, Duration::from_secs(2)).expect("client");
        let ctx = context();

        assert!(client.complete(&ctx).is_err());
        let response = client
            .complete(&ctx)
            .expect("explicit retry should succeed");
        task.join().expect("server task");

        assert_eq!(response.suggestion, " again");
        let requests = requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body, requests[1].body);
    }

    #[test]
    fn malformed_success_response_is_not_treated_as_empty_suggestion() {
        let (base, requests, task) = serve(vec![("200 OK", "not-json")]);
        let client = CoreClient::new(base, None, Duration::from_secs(2)).expect("client");

        let error = client
            .complete(&context())
            .expect_err("malformed JSON must fail");
        task.join().expect("server task");

        assert!(error.to_string().contains("not valid JSON"));
        assert_eq!(requests.lock().expect("request lock").len(), 1);
    }

    #[test]
    fn blank_token_is_not_sent_as_an_authorization_credential() {
        let (base, requests, task) = serve(vec![("200 OK", r#"{"debounceMs":400}"#)]);
        let client =
            CoreClient::new(base, Some("  \t".into()), Duration::from_secs(2)).expect("client");

        client.get_config().expect("config response");
        task.join().expect("server task");

        assert!(!requests.lock().expect("request lock")[0]
            .head
            .to_ascii_lowercase()
            .contains("authorization:"));
    }

    #[test]
    fn request_timeout_is_bounded_and_reported_without_an_implicit_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
        let address = listener.local_addr().expect("listener address");
        let request_count = Arc::new(Mutex::new(0_usize));
        let captured_count = Arc::clone(&request_count);
        let server_task = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client connection");
            let _request = read_request(&stream);
            *captured_count.lock().expect("request count lock") += 1;
            thread::sleep(Duration::from_millis(150));
        });
        let client = CoreClient::new(format!("http://{address}"), None, Duration::from_millis(40))
            .expect("client");

        let error = client
            .complete(&context())
            .expect_err("an unresponsive Core must time out");
        server_task.join().expect("server task");

        assert!(error.to_string().contains("request failed"));
        assert_eq!(*request_count.lock().expect("request count lock"), 1);
    }

    #[test]
    fn cloned_clients_keep_concurrent_requests_independent() {
        const REQUEST_COUNT: usize = 6;
        let responses = (0..REQUEST_COUNT)
            .map(|_| ("200 OK", r#"{"suggestion":"ok"}"#))
            .collect();
        let (base, requests, server_task) = serve(responses);
        let client = CoreClient::new(base, None, Duration::from_secs(2)).expect("client");
        let tasks: Vec<_> = (0..REQUEST_COUNT)
            .map(|index| {
                let client = client.clone();
                thread::spawn(move || {
                    let mut ctx = context();
                    ctx.before = format!("context-{index}");
                    client
                        .complete(&ctx)
                        .expect("concurrent completion")
                        .suggestion
                })
            })
            .collect();

        for task in tasks {
            assert_eq!(task.join().expect("client task"), "ok");
        }
        server_task.join().expect("server task");

        let mut contexts: Vec<String> = requests
            .lock()
            .expect("request lock")
            .iter()
            .map(|request| {
                serde_json::from_str::<serde_json::Value>(&request.body).expect("request JSON")
                    ["context"]
                    .as_str()
                    .expect("context string")
                    .to_string()
            })
            .collect();
        contexts.sort();
        assert_eq!(
            contexts,
            (0..REQUEST_COUNT)
                .map(|index| format!("context-{index}"))
                .collect::<Vec<_>>()
        );
    }
}
