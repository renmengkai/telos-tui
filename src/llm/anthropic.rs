use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::app::AppEvent;
use crate::session::Message;
use super::LlmClient;

#[derive(Clone)]
pub struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    http: Client,
}

impl AnthropicClient {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            model,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            http: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn run(
        api_key: String,
        model: String,
        base_url: String,
        http: Client,
        system: String,
        messages: Vec<Message>,
        tx: mpsc::Sender<AppEvent>,
        cancel: oneshot::Receiver<()>,
    ) {
        let msgs: Vec<Value> = messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();

        let body = json!({
            "model": model,
            "max_tokens": 8096,
            "stream": true,
            "system": system,
            "messages": msgs,
        });

        let stream_future = Self::stream_http(api_key, base_url, http, body, tx.clone());

        tokio::select! {
            _ = stream_future => {}
            _ = cancel => {
                tx.send(AppEvent::StreamDone).await.ok();
            }
        }
    }

    async fn stream_http(
        api_key: String,
        base_url: String,
        http: Client,
        body: Value,
        tx: mpsc::Sender<AppEvent>,
    ) {
        let url = format!("{base_url}/v1/messages");
        let resp = match http
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tx.send(AppEvent::StreamError(e.to_string())).await.ok();
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            tx.send(AppEvent::StreamError(format!("HTTP {status}: {err}"))).await.ok();
            return;
        }

        let mut resp = resp;
        let mut buf = String::new();

        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    buf.push_str(&String::from_utf8_lossy(&chunk));
                    if Self::drain_buf(&mut buf, &tx).await {
                        return; // message_stop received
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tx.send(AppEvent::StreamError(e.to_string())).await.ok();
                    return;
                }
            }
        }

        tx.send(AppEvent::StreamDone).await.ok();
    }

    /// Process complete SSE lines from buffer. Returns true if stream should end.
    async fn drain_buf(buf: &mut String, tx: &mpsc::Sender<AppEvent>) -> bool {
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').to_string();
            *buf = buf[pos + 1..].to_string();

            if !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                continue;
            };

            match event["type"].as_str() {
                Some("content_block_delta") => {
                    if let Some(text) = event["delta"]["text"].as_str() {
                        tx.send(AppEvent::StreamChunk(text.to_string())).await.ok();
                    }
                }
                Some("message_stop") => {
                    tx.send(AppEvent::StreamDone).await.ok();
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}

impl LlmClient for AnthropicClient {
    fn start_stream(
        &self,
        system: String,
        messages: Vec<Message>,
        tx: mpsc::Sender<AppEvent>,
        cancel: oneshot::Receiver<()>,
    ) {
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let base_url = self.base_url.clone();
        let http = self.http.clone();

        tokio::spawn(async move {
            Self::run(api_key, model, base_url, http, system, messages, tx, cancel).await;
        });
    }
}
