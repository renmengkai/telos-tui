use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::app::AppEvent;
use crate::session::Message;
use super::LlmClient;

#[derive(Clone)]
pub struct OpenAiClient {
    api_key: String,
    model: String,
    base_url: String,
    http: Client,
}

impl OpenAiClient {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            model,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com".to_string()),
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
        let stream_future = Self::stream_http(api_key, model, base_url, http, system, messages, tx.clone());

        tokio::select! {
            _ = stream_future => {}
            _ = cancel => {
                tx.send(AppEvent::StreamDone).await.ok();
            }
        }
    }

    async fn stream_http(
        api_key: String,
        model: String,
        base_url: String,
        http: Client,
        system: String,
        messages: Vec<Message>,
        tx: mpsc::Sender<AppEvent>,
    ) {
        let mut msgs = vec![json!({ "role": "system", "content": system })];
        for m in &messages {
            msgs.push(json!({ "role": m.role, "content": m.content }));
        }

        let body = json!({ "model": model, "stream": true, "messages": msgs });
        // Normalize: strip trailing "/v1" so both "https://api.openai.com" and
        // "https://api.minimaxi.com/v1" resolve to the same path.
        let base = base_url.trim_end_matches("/v1").trim_end_matches('/');
        let url = format!("{base}/v1/chat/completions");

        let resp = match http
            .post(&url)
            .bearer_auth(&api_key)
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
                        return;
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

    async fn drain_buf(buf: &mut String, tx: &mpsc::Sender<AppEvent>) -> bool {
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').to_string();
            *buf = buf[pos + 1..].to_string();

            if !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];

            if data.trim() == "[DONE]" {
                tx.send(AppEvent::StreamDone).await.ok();
                return true;
            }

            let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                continue;
            };

            if chunk["choices"][0]["finish_reason"].as_str() == Some("stop") {
                tx.send(AppEvent::StreamDone).await.ok();
                return true;
            }

            if let Some(text) = chunk["choices"][0]["delta"]["content"].as_str() {
                if !text.is_empty() {
                    tx.send(AppEvent::StreamChunk(text.to_string())).await.ok();
                }
            }
        }
        false
    }
}

impl LlmClient for OpenAiClient {
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
