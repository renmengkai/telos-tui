pub mod anthropic;
pub mod openai;

use tokio::sync::{mpsc, oneshot};

use crate::app::AppEvent;
use crate::config::Provider;
use crate::session::Message;

pub trait LlmClient: Send + Sync {
    /// Spawn an async task that streams response chunks into `tx`.
    /// Dropping / sending to `cancel` aborts the stream and sends StreamDone.
    fn start_stream(
        &self,
        system: String,
        messages: Vec<Message>,
        tx: mpsc::Sender<AppEvent>,
        cancel: oneshot::Receiver<()>,
    );
}

pub fn build(
    provider: &Provider,
    api_key: String,
    model: String,
    base_url: Option<String>,
) -> Box<dyn LlmClient> {
    match provider {
        Provider::Anthropic => Box::new(anthropic::AnthropicClient::new(api_key, model, base_url)),
        Provider::OpenAi => Box::new(openai::OpenAiClient::new(api_key, model, base_url)),
    }
}
