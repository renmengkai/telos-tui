use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Extracted thinking/reasoning content (e.g. from <think> tags).
    #[serde(skip)]
    pub thinking_content: Option<String>,
    /// Whether the thinking block is collapsed in the UI.
    #[serde(skip)]
    pub thinking_collapsed: bool,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into(), thinking_content: None, thinking_collapsed: true }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into(), thinking_content: None, thinking_collapsed: true }
    }

    /// Parse `<think>...</think>` tags from content, splitting into
    /// thinking_content and cleaned content.
    pub fn extract_thinking(&mut self) {
        if let Some(start) = self.content.find("<think>") {
            if let Some(end) = self.content.find("</think>") {
                if end <= start {
                    return;
                }
                let think_start = start + "<think>".len();
                let thinking = self.content[think_start..end].trim().to_string();
                let before = self.content[..start].trim_end();
                let after = self.content[end + "</think>".len()..].trim_start();
                self.content = format!("{}{}", before, after).trim().to_string();
                if !thinking.is_empty() {
                    self.thinking_content = Some(thinking);
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Session {
    pub messages: Vec<Message>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, msg: Message) {
        const MAX_MESSAGES: usize = 200;
        self.messages.push(msg);
        if self.messages.len() > MAX_MESSAGES {
            // Keep the oldest system message if any, otherwise drop from the front.
            let drop_count = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(0..drop_count);
        }
    }

    /// Messages to send to the API — excludes System role (handled as separate field).
    /// Returns an iterator to avoid allocating a Vec on every call.
    pub fn api_messages(&self) -> impl Iterator<Item = &Message> + '_ {
        self.messages.iter().filter(|m| m.role != Role::System)
    }

    pub fn last_user_message(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
    }

    /// Append text to the last assistant message (used for soft-prompt suffix).
    pub fn append_to_last_assistant(&mut self, suffix: &str) {
        if let Some(m) = self.messages.iter_mut().rev().find(|m| m.role == Role::Assistant) {
            m.content.push_str(suffix);
        }
    }
}
