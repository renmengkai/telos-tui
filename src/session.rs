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
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
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
        self.messages.push(msg);
    }

    /// Messages to send to the API — excludes System role (handled as separate field)
    pub fn api_messages(&self) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| m.role != Role::System)
            .collect()
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
