use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::{mpsc, oneshot};

use crate::config::Config;
use crate::codeword::{
    Action, CodewordDetector,
    action_label, extract_domain_name, extract_skill_ref,
};
use crate::llm::{self, LlmClient};
use crate::project::{
    EpisodicLogger, ProjectState, append_file, last_project_id,
    projects_with_quality_events, timestamp_now,
};
use crate::session::{Message, Session};
use crate::skill::{Skill, matcher::SkillMatcher, store::SkillStore};

// ── Events ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    StreamChunk(String),
    StreamDone,
    StreamError(String),
    /// All parallel skill analyses done; ready for synthesis.
    ParallelAnalysisDone { project_id: String, responses: Vec<(String, String)> },
}

// ── Group-chat state ──────────────────────────────────────────────────────────

pub struct GroupChatState {
    pub project_id: String,
    pub skills: Vec<String>,
    pub material: String,
    pub current_idx: usize,
    pub responses: Vec<(String, String)>,
    pub total_assumptions: u32,
}

impl GroupChatState {
    fn current_skill(&self) -> &str {
        &self.skills[self.current_idx]
    }
    fn has_next(&self) -> bool {
        self.current_idx + 1 < self.skills.len()
    }
}

// ── Mode state machine ────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub enum AppMode {
    #[default]
    Chat,
    /// LLM is auto-inferring skill metadata + body (replaces old 4-step wizard)
    AutoSave,
    ConfirmSave(SkillDraft),
    ConfirmDelete { domain: String, name: String },
    /// Mini-distillation: @skill + correction signal — skill_name is a real skill name
    Distilling { skill_name: String },
    ConfirmDistill { skill_name: String, new_content: String },
    /// Full Ring-3 distillation from project logs — skill_name is a project_id
    DistillProject { project_id: String, skills: Vec<String> },
    ConfirmDistillProject { project_id: String, skills: Vec<String>, new_content: String },
    GroupChatAnalyzing,
    GroupChatSynthesizing,
    /// Interactive /add-skill wizard
    CreateSkill(CreateStage),
    /// Ask user to confirm before starting the create-skill wizard
    ConfirmCreateSkill,
    /// Codeword management wizard
    AddCodeword(CwStage),
}

#[derive(Debug, PartialEq)]
pub enum CreateStage {
    AskName,
    AskDescription { name: String },
    AskTriggers { name: String, desc: String },
}

#[derive(Debug, PartialEq)]
pub enum CwStage {
    AskTrigger,
    AskAction { trigger: String },
    RemoveTrigger,
}

#[derive(Debug, PartialEq, Clone)]
pub struct SkillDraft {
    pub domain: String,
    pub name: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub content: String,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub should_quit: bool,
    pub input: String,
    pub cursor_pos: usize,
    pub session: Session,
    pub mode: AppMode,
    pub streaming: bool,
    pub current_response: String,
    pub status_msg: String,
    pub config: Config,

    pub loaded_skills: Vec<Skill>,
    pub available_skills: Vec<Skill>,
    pub current_project_id: Option<String>,
    pub group_chat_state: Option<GroupChatState>,

    /// Soft-prompt was already shown this session — don't show again.
    soft_prompt_used: bool,

    input_history: Vec<String>,
    history_idx: Option<usize>,
    saved_input: String,

    cancel_tx: Option<oneshot::Sender<()>>,
    /// Handle for a running parallel group-chat task (so Esc can abort it).
    parallel_task: Option<tokio::task::JoinHandle<()>>,
    /// Oneshot senders for in-flight parallel LLM streams (so Esc can cancel each sub-stream).
    parallel_cancels: Vec<oneshot::Sender<()>>,
    /// Lines scrolled UP from the bottom in the message panel (0 = pinned to bottom).
    pub message_scroll: u16,

    skill_store: SkillStore,
    codeword: CodewordDetector,
    llm: Box<dyn LlmClient>,
    pub event_tx: mpsc::Sender<AppEvent>,
}

impl App {
    pub fn new(
        config: Config,
        event_tx: mpsc::Sender<AppEvent>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let llm = llm::build(
            &config.provider,
            config.api_key.clone(),
            config.model.clone(),
            config.base_url.clone(),
        );
        let skill_store = SkillStore::new(config.skills_dir.clone());
        let available_skills = skill_store.scan_all();

        // Load custom codewords
        let mut codeword = CodewordDetector::new();
        codeword.load_custom(&config.codewords_config_path());

        // Restore most-recent project
        let current_project_id = last_project_id(&config.projects_dir());

        // Distillation pending?
        let distill_pending = check_distill_needed(&config.distill_needed_dir());

        let status_msg = if !distill_pending.is_empty() {
            format!("⚠ {} 个蒸馏待处理 — 说「启动飞轮」处理", distill_pending.len())
        } else if let Some(ref id) = current_project_id {
            format!("已恢复项目 {id}  Ctrl+Q 退出  Ctrl+L 清屏")
        } else {
            "Ready  Ctrl+Q 退出  Ctrl+L 清屏  Esc 取消".to_string()
        };

        Ok(App {
            should_quit: false,
            input: String::new(),
            cursor_pos: 0,
            session: Session::new(),
            mode: AppMode::Chat,
            streaming: false,
            current_response: String::new(),
            status_msg,
            config,
            loaded_skills: Vec::new(),
            available_skills,
            current_project_id,
            group_chat_state: None,
            soft_prompt_used: false,
            input_history: Vec::new(),
            history_idx: None,
            saved_input: String::new(),
            cancel_tx: None,
            parallel_task: None,
            parallel_cancels: Vec::new(),
            message_scroll: 0,
            skill_store,
            codeword,
            llm,
            event_tx,
        })
    }

    // ── Event dispatch ────────────────────────────────────────────────────────

    pub async fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => self.handle_key(key).await,
            AppEvent::StreamChunk(text) => {
                self.current_response.push_str(&text);
            }
            AppEvent::StreamDone => {
                self.cancel_tx = None;
                let response = std::mem::take(&mut self.current_response);
                self.streaming = false;

                let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
                match old_mode {
                    // ── Auto-save: parse JSON → ConfirmSave ──
                    AppMode::AutoSave => {
                        match extract_skill_draft(&response) {
                            Some(draft) => {
                                let preview = format!(
                                    "📝 推断完成：**{}/{}**\n摘要：{}\n标签：{}\n\n```\n{}\n```\n\n输入 `y` 确认保存，`n` 取消。",
                                    draft.domain,
                                    draft.name,
                                    draft.summary,
                                    draft.tags.join(", "),
                                    truncate(&draft.content, 600),
                                );
                                self.session.push(Message::assistant(preview));
                                self.mode = AppMode::ConfirmSave(draft);
                                self.status_msg = "确认保存？(y / n)".to_string();
                            }
                            None => {
                                self.session.push(Message::assistant(format!(
                                    "⚠ 无法自动推断技能结构。原始输出：\n\n{response}\n\n\
                                     请说「记下来」重试，或直接编辑技能文件。"
                                )));
                                self.status_msg = "Ready".to_string();
                            }
                        }
                    }
                    // ── Mini-distillation done → ConfirmDistill ──
                    AppMode::Distilling { skill_name } => {
                        let preview = format!(
                            "已生成更新内容：\n\n```\n{response}\n```\n\n\
                             输入 `y` 应用到 @{skill_name}，`n` 取消。"
                        );
                        self.session.push(Message::assistant(preview));
                        self.mode = AppMode::ConfirmDistill { skill_name, new_content: response };
                        self.status_msg = "确认应用更新？(y / n)".to_string();
                    }
                    // ── Full Ring-3 project distillation done → ConfirmDistillProject ──
                    AppMode::DistillProject { project_id, skills } => {
                        let skills_label = skills
                            .iter()
                            .map(|s| format!("@{s}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let preview = format!(
                            "蒸馏完成（项目 {project_id}）\n\
                             将保存到以下 Skill 的 memory/ 目录：{skills_label}\n\n\
                             ```\n{}\n```\n\n\
                             输入 `y` 保存，`n` 取消。",
                            truncate(&response, 800)
                        );
                        self.session.push(Message::assistant(preview));
                        self.mode = AppMode::ConfirmDistillProject { project_id, skills, new_content: response };
                        self.status_msg = "确认保存蒸馏结果？(y / n)".to_string();
                    }
                    // ── Group chat ──
                    AppMode::GroupChatAnalyzing => {
                        self.handle_gc_analysis_done(response).await;
                    }
                    AppMode::GroupChatSynthesizing => {
                        self.handle_gc_synthesis_done(response);
                    }
                    // ── Normal chat ──
                    _ => {
                        if !response.is_empty() {
                            let mut msg = Message::assistant(response);
                            msg.extract_thinking();
                            self.session.push(msg);
                            self.try_soft_prompt();
                        }
                        self.message_scroll = 0; // scroll to bottom on new message
                        self.status_msg = "Ready".to_string();
                    }
                }
            }
            AppEvent::StreamError(err) => {
                self.cancel_tx = None;
                self.parallel_cancels.clear();
                self.streaming = false;
                self.current_response.clear();
                self.group_chat_state = None;
                self.mode = AppMode::Chat;
                self.status_msg = format!("Error: {err}");
            }
            AppEvent::ParallelAnalysisDone { project_id, responses } => {
                self.streaming = false;
                self.parallel_task = None;
                self.parallel_cancels.clear();
                self.message_scroll = 0;
                // Push individual skill results
                for (skill_name, resp) in &responses {
                    self.session.push(Message::assistant(format!("[@{skill_name}]\n{resp}")));
                }
                // Set up group_chat_state for synthesis
                let skills: Vec<String> = responses.iter().map(|(n, _)| n.clone()).collect();
                let total_assumptions = responses.iter().map(|(_, r)| count_assumptions(r)).sum();
                self.group_chat_state = Some(GroupChatState {
                    project_id,
                    skills,
                    material: String::new(),
                    current_idx: 0,
                    responses: responses.clone(),
                    total_assumptions,
                });
                // Start synthesis
                let (system, messages) = {
                    let state = self.group_chat_state.as_ref().unwrap();
                    Self::build_synthesis_prompt(&state.responses, &state.skills)
                };
                self.mode = AppMode::GroupChatSynthesizing;
                self.start_stream(system, messages);
            }
        }
    }

    // ── Soft prompt ───────────────────────────────────────────────────────────

    fn try_soft_prompt(&mut self) {
        if self.soft_prompt_used {
            return;
        }
        let signal = self
            .session
            .last_user_message()
            .map(|m| self.codeword.has_soft_signal(m))
            .unwrap_or(false);

        if signal {
            self.session.append_to_last_assistant(
                "\n\n---\n💡 刚才聊的技巧挺实用，要存起来吗？说「记下来」即可，说「不用了」跳过。",
            );
            self.soft_prompt_used = true;
        }
    }

    // ── Group chat callbacks ──────────────────────────────────────────────────

    async fn handle_gc_analysis_done(&mut self, response: String) {
        let (should_continue, next_skill, material) = {
            let state = match self.group_chat_state.as_mut() {
                Some(s) => s,
                None => {
                    self.session.push(Message::assistant(response));
                    return;
                }
            };
            let skill_name = state.current_skill().to_string();
            state.total_assumptions += count_assumptions(&response);
            state.responses.push((skill_name.clone(), response.clone()));

            self.session.push(Message::assistant(format!(
                "[@{skill_name}]\n{response}"
            )));

            if state.has_next() {
                state.current_idx += 1;
                let next = state.current_skill().to_string();
                let mat = state.material.clone();
                (true, next, mat)
            } else {
                state.current_idx += 1;
                (false, String::new(), String::new())
            }
        };

        if should_continue {
            let (system, messages) = self.build_gc_skill_prompt(&next_skill, &material);
            self.mode = AppMode::GroupChatAnalyzing;
            self.start_stream(system, messages);
        } else {
            let (system, messages) = {
                let state = self.group_chat_state.as_ref().unwrap();
                Self::build_synthesis_prompt(&state.responses, &state.skills)
            };
            self.mode = AppMode::GroupChatSynthesizing;
            self.start_stream(system, messages);
        }
    }

    fn handle_gc_synthesis_done(&mut self, response: String) {
        self.message_scroll = 0;
        self.session.push(Message::assistant(format!(
            "━━━ 综合视图 ━━━\n{response}\n━━━━━━━━━━━━━━━━"
        )));
        if let Some(state) = self.group_chat_state.take() {
            // Update projects_involved for each skill involved in this group chat
            for skill_name in &state.skills {
                if let Some(skill) = self.available_skills.iter()
                    .find(|s| &s.name == skill_name || &s.domain == skill_name)
                    .cloned()
                {
                    self.skill_store.add_project_to_skill(&skill, &state.project_id);
                }
            }

            // Persist project state (skills, ring, assumptions)
            let mut pstate = ProjectState::new(state.project_id.clone());
            pstate.skills = state.skills.clone();
            pstate.ring = 1;
            pstate.assumptions = state.total_assumptions;
            let _ = pstate.save(&self.config.projects_dir());

            self.log_gc_event(&state.project_id, &state.skills);
            self.log_gc_synthesis(&state.project_id, &response);
            self.check_distill_trigger(&state.project_id);
            self.current_project_id = Some(state.project_id.clone());
            let assumptions = state.total_assumptions;
            self.status_msg = if assumptions > 0 {
                format!("[Ring 1] 完成 | 项目: {} | 假设: {}项", state.project_id, assumptions)
            } else {
                format!("[Ring 1] 完成 | 项目: {}", state.project_id)
            };
        }
    }

    // ── Keyboard handling ─────────────────────────────────────────────────────

    async fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }
        if key.code == KeyCode::Esc {
            self.cancel_operation();
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.streaming {
                self.cancel_operation();
            }
            return;
        }
        if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.session.messages.clear();
            self.loaded_skills.clear();
            self.group_chat_state = None;
            self.soft_prompt_used = false;
            self.mode = AppMode::Chat;
            self.status_msg = "已清空对话".to_string();
            return;
        }
        if self.streaming {
            return;
        }

        // Toggle thinking visibility for the most recent assistant message
        if key.code == KeyCode::Char('t') && !key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(msg) = self.session.messages.iter_mut().rev().find(|m| m.role == crate::session::Role::Assistant && m.thinking_content.is_some()) {
                msg.thinking_collapsed = !msg.thinking_collapsed;
            }
            return;
        }

        match key.code {
            KeyCode::Enter => {
                let msg = self.input.trim().to_string();
                if !msg.is_empty() {
                    if self.input_history.last().map(|s| s.as_str()) != Some(&msg) {
                        self.input_history.push(msg.clone());
                    }
                    self.history_idx = None;
                    self.saved_input.clear();
                    self.input.clear();
                    self.cursor_pos = 0;
                    self.submit(msg).await;
                }
            }
            KeyCode::Up => self.history_prev(),
            KeyCode::Down => self.history_next(),
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_pos, c);
                self.cursor_pos += c.len_utf8();
            }
            KeyCode::Backspace if self.cursor_pos > 0 => {
                // Walk back one full char boundary
                let new_pos = prev_char_boundary(&self.input, self.cursor_pos);
                self.input.remove(new_pos);
                self.cursor_pos = new_pos;
            }
            KeyCode::Delete if self.cursor_pos < self.input.len() => {
                self.input.remove(self.cursor_pos);
            }
            KeyCode::Left if self.cursor_pos > 0 => {
                self.cursor_pos = prev_char_boundary(&self.input, self.cursor_pos);
            }
            KeyCode::Right if self.cursor_pos < self.input.len() => {
                self.cursor_pos += self.input[self.cursor_pos..].chars().next().map_or(0, |c| c.len_utf8());
            }
            KeyCode::Home => self.cursor_pos = 0,
            KeyCode::End => self.cursor_pos = self.input.len(),
            KeyCode::PageUp => {
                self.message_scroll = self.message_scroll.saturating_add(10);
            }
            KeyCode::PageDown => {
                self.message_scroll = self.message_scroll.saturating_sub(10);
                if self.message_scroll == 0 {
                    self.message_scroll = 0; // already at bottom
                }
            }
            _ => {}
        }
    }

    fn cancel_operation(&mut self) {
        if self.streaming {
            if let Some(tx) = self.cancel_tx.take() {
                let _ = tx.send(());
            }
            for tx in self.parallel_cancels.drain(..) {
                let _ = tx.send(());
            }
            if let Some(handle) = self.parallel_task.take() {
                handle.abort();
            }
            self.group_chat_state = None;
            self.streaming = false;
            self.mode = AppMode::Chat;
            self.status_msg = "已取消".to_string();
        } else if !matches!(self.mode, AppMode::Chat) {
            self.mode = AppMode::Chat;
            self.session.push(Message::assistant("已取消。".to_string()));
            self.status_msg = "Ready".to_string();
        }
    }

    // ── Input history ─────────────────────────────────────────────────────────

    fn history_prev(&mut self) {
        if self.input_history.is_empty() { return; }
        match self.history_idx {
            None => {
                self.saved_input = self.input.clone();
                self.history_idx = Some(self.input_history.len() - 1);
            }
            Some(0) => return,
            Some(idx) => self.history_idx = Some(idx - 1),
        }
        let idx = self.history_idx.unwrap();
        self.input = self.input_history[idx].clone();
        self.cursor_pos = self.input.len();
    }

    fn history_next(&mut self) {
        let Some(idx) = self.history_idx else { return };
        if idx + 1 >= self.input_history.len() {
            self.input = std::mem::take(&mut self.saved_input);
            self.cursor_pos = self.input.len();
            self.history_idx = None;
        } else {
            self.history_idx = Some(idx + 1);
            self.input = self.input_history[idx + 1].clone();
            self.cursor_pos = self.input.len();
        }
    }

    // ── Submit ────────────────────────────────────────────────────────────────

    async fn submit(&mut self, message: String) {
        // Mode-specific intercepts
        if matches!(self.mode, AppMode::ConfirmSave(_)) {
            self.handle_confirm_save(message).await;
            return;
        }
        if matches!(self.mode, AppMode::ConfirmDelete { .. }) {
            self.handle_confirm_delete(message).await;
            return;
        }
        if matches!(self.mode, AppMode::ConfirmDistill { .. }) {
            self.handle_confirm_distill(message).await;
            return;
        }
        if matches!(self.mode, AppMode::ConfirmDistillProject { .. }) {
            self.handle_confirm_distill_project(message).await;
            return;
        }
        if matches!(self.mode, AppMode::ConfirmCreateSkill) {
            self.handle_confirm_create_skill(message).await;
            return;
        }
        if matches!(self.mode, AppMode::CreateSkill(_)) {
            self.handle_create_skill_input(message).await;
            return;
        }
        if matches!(self.mode, AppMode::AddCodeword(_)) {
            self.handle_add_codeword_input(message).await;
            return;
        }

        let trimmed = message.trim();

        // @? / @list
        if trimmed == "@?" || trimmed == "@list" {
            self.session.push(Message::user(message));
            let list = self.build_skill_list();
            self.session.push(Message::assistant(list));
            return;
        }

        let at_skills = CodewordDetector::extract_at_skills(&message);

        // Mini-distill: @skill + correction
        if !at_skills.is_empty() && self.codeword.is_correction(&message) {
            let skill_name = at_skills[0].clone();
            self.session.push(Message::user(message.clone()));
            self.start_mini_distill(skill_name, message).await;
            return;
        }

        // Codeword actions
        if let Some((action, needs_confirmation)) = self.codeword.detect(&message) {
            self.session.push(Message::user(message.clone()));
            if needs_confirmation {
                if let Some(prompt) = self.codeword.confirmation_prompt(&action) {
                    self.session.push(Message::assistant(prompt));
                    self.mode = AppMode::ConfirmCreateSkill;
                    self.status_msg = "确认创建？(y/n)".to_string();
                    return;
                }
            }
            self.track_action_usage(&action);
            self.handle_action(action, &message).await;
            return;
        }

        // @all
        if at_skills.iter().any(|s| s == "all") {
            self.activate_all_skills();
        }

        let explicit: Vec<String> = at_skills
            .into_iter()
            .filter(|s| s != "all" && s != "?" && s != "list")
            .collect();

        // Two or more @skills → group chat (sequential or --parallel)
        if explicit.len() >= 2 {
            let parallel = message.contains("--parallel");
            let clean_material = message.replace("--parallel", "").trim().to_string();
            self.session.push(Message::user(message.clone()));
            if parallel {
                self.trigger_group_chat_parallel(explicit, clean_material).await;
            } else {
                self.trigger_group_chat(explicit, message).await;
            }
            return;
        }

        // Single @skill → load as context
        if !explicit.is_empty() {
            self.activate_skills(&explicit);
        }

        // Silent auto-match
        if explicit.is_empty() {
            let to_load: Vec<Skill> = SkillMatcher::match_skills(&message, &self.available_skills)
                .into_iter()
                .map(|(s, _)| s.clone())
                .collect();
            for mut skill in to_load {
                if !self.loaded_skills.iter().any(|s| s.name == skill.name) {
                    self.skill_store.load_content(&mut skill);
                    self.loaded_skills.push(skill);
                }
            }
        }

        // Normal LLM call
        let system = self.build_system_prompt();
        self.session.push(Message::user(message));
        let msgs = self.session.api_messages().iter().map(|m| (*m).clone()).collect();
        self.start_stream(system, msgs);
    }

    // ── Auto-save (LLM-inferred) ──────────────────────────────────────────────

    fn start_auto_save(&mut self) {
        let system =
            "你是技能提炼助手。从对话中提炼程序性知识（如何做某件事）。\n\
             请严格按以下 JSON 格式输出，放在 ```json 代码块中，不要其他内容：\n\
             ```json\n\
             {\n\
               \"domain\": \"领域英文小写（如 cooking）\",\n\
               \"name\": \"技能名英文小写连字符（如 braising）\",\n\
               \"tags\": [\"tag1\", \"tag2\"],\n\
               \"summary\": \"一句话摘要（中文）\",\n\
               \"content\": \"完整 Markdown 正文（格式：\\n# 标题\\n\\n## 核心原则\\n- \\n\\n## 关键技巧\\n- \\n\\n## 常见陷阱\\n- ）\"\n\
             }\n\
             ```"
                .to_string();

        let messages = self.session.api_messages().iter().map(|m| (*m).clone()).collect();
        self.session
            .push(Message::assistant("正在分析对话，自动推断技能...".to_string()));
        self.mode = AppMode::AutoSave;
        self.start_stream(system, messages);
    }

    async fn handle_confirm_save(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        let AppMode::ConfirmSave(draft) = old_mode else { return };

        let confirmed = matches!(
            input.trim().to_lowercase().as_str(),
            "y" | "yes" | "确认" | "是"
        );

        if confirmed {
            match self.skill_store.save_or_merge(
                &draft.domain,
                &draft.name,
                &draft.content,
                &draft.tags,
                &draft.summary,
            ) {
                Ok(path) => {
                    self.session.push(Message::assistant(format!(
                        "✓ 已保存到 {}\n下次话题相关时自动加载。\n\n说「看看家底」查看全部技能。",
                        path.display()
                    )));
                    self.available_skills = self.skill_store.scan_all();
                    self.soft_prompt_used = true; // don't double-prompt
                }
                Err(e) => {
                    self.session.push(Message::assistant(format!("❌ 保存失败：{e}")));
                }
            }
        } else {
            self.session.push(Message::assistant("已取消。".to_string()));
        }
        self.status_msg = "Ready".to_string();
    }

    async fn handle_confirm_delete(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        let AppMode::ConfirmDelete { domain, name } = old_mode else { return };

        let confirmed = matches!(
            input.trim().to_lowercase().as_str(),
            "y" | "yes" | "确认" | "是"
        );

        if confirmed {
            match self.skill_store.delete_skill(&domain, &name) {
                Ok(_) => {
                    self.loaded_skills.retain(|s| !(s.domain == domain && s.name == name));
                    self.available_skills = self.skill_store.scan_all();
                    self.session.push(Message::assistant(format!("✓ 已删除 {domain}/{name}")));
                }
                Err(e) => {
                    self.session.push(Message::assistant(format!("❌ 删除失败：{e}")));
                }
            }
        } else {
            self.session.push(Message::assistant("已取消。".to_string()));
        }
        self.status_msg = "Ready".to_string();
    }

    // ── Mini-distillation ─────────────────────────────────────────────────────

    async fn start_mini_distill(&mut self, skill_name: String, correction: String) {
        let found = self
            .available_skills
            .iter()
            .find(|s| s.name == skill_name || s.domain == skill_name)
            .cloned();

        let Some(mut skill) = found else {
            self.session.push(Message::assistant(format!(
                "❌ Skill '@{skill_name}' 不存在"
            )));
            return;
        };

        self.skill_store.load_content(&mut skill);
        let existing = skill.content.clone().unwrap_or_default();
        let system = format!(
            "你是技能文件维护助手。根据用户的纠正/补充更新技能内容。\n\
             保持原有 Markdown 格式和章节结构，只修改相关部分。\n\
             只输出正文，不包含 YAML frontmatter。\n\n\
             当前技能内容：\n---\n{existing}\n---"
        );
        let messages = vec![Message::user(format!("纠正/补充：{correction}"))];
        self.session.push(Message::assistant(format!("正在更新 @{skill_name}...")));
        self.mode = AppMode::Distilling { skill_name };
        self.start_stream(system, messages);
    }

    async fn handle_confirm_distill(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        let AppMode::ConfirmDistill { skill_name, new_content } = old_mode else { return };

        let confirmed = matches!(
            input.trim().to_lowercase().as_str(),
            "y" | "yes" | "确认" | "是"
        );

        if confirmed {
            let found = self
                .available_skills
                .iter()
                .find(|s| s.name == skill_name || s.domain == skill_name)
                .cloned();

            match found {
                Some(skill) => match self.skill_store.update_content(&skill, &new_content) {
                    Ok(_) => {
                        if let Some(s) = self.loaded_skills.iter_mut().find(|s| s.name == skill.name) {
                            s.content = Some(new_content);
                        }
                        self.available_skills = self.skill_store.scan_all();
                        let (_, distill_count, projects) = self.skill_store.skill_evolution_info(&skill);
                        let evo_note = if projects > 0 {
                            format!("  [第{}次蒸馏 · 参与{}个项目]", distill_count, projects)
                        } else {
                            format!("  [第{}次蒸馏]", distill_count)
                        };
                        self.session.push(Message::assistant(format!("✓ @{skill_name} 已更新{evo_note}")));
                        // Clean up .distill-needed marker if this was a project distillation
                        let marker = self.config.distill_needed_dir().join(&skill_name);
                        if marker.exists() {
                            let _ = fs::remove_file(&marker);
                        }
                    }
                    Err(e) => {
                        self.session.push(Message::assistant(format!("❌ 更新失败：{e}")));
                    }
                },
                None => {
                    self.session.push(Message::assistant(format!(
                        "❌ Skill '@{skill_name}' 不存在"
                    )));
                }
            }
        } else {
            self.session.push(Message::assistant("已取消。".to_string()));
        }
        self.status_msg = "Ready".to_string();
    }

    // ── Ring-3 full distillation confirm ─────────────────────────────────────

    async fn handle_confirm_distill_project(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        let AppMode::ConfirmDistillProject { project_id, skills, new_content } = old_mode else {
            return;
        };

        let confirmed = matches!(
            input.trim().to_lowercase().as_str(),
            "y" | "yes" | "确认" | "是"
        );

        if confirmed {
            let date = crate::project::timestamp_now();
            let header = format!("\n\n## 蒸馏 {date}（来自项目 {project_id}）\n\n");
            let mut saved = Vec::new();
            let mut failed = Vec::new();

            for skill_name in &skills {
                let found = self
                    .available_skills
                    .iter()
                    .find(|s| s.name == *skill_name || s.domain == *skill_name)
                    .cloned();

                match found {
                    Some(skill) => {
                        let memory_dir = skill.path.join("memory");
                        let _ = fs::create_dir_all(&memory_dir);
                        let dest = memory_dir.join("distilled.md");
                        let entry = format!("{header}{new_content}\n");
                        match append_file(&dest, &entry) {
                            Ok(_) => {
                                self.skill_store.increment_distill_count(&skill);
                                saved.push(skill_name.clone());
                            }
                            Err(e) => failed.push(format!("{skill_name}: {e}")),
                        }
                    }
                    None => failed.push(format!("{skill_name}: skill 不存在")),
                }
            }

            // Clean up .distill-needed marker
            let _ = fs::remove_file(self.config.distill_needed_dir().join(&project_id));
            self.available_skills = self.skill_store.scan_all();

            let mut msg = format!("✓ 蒸馏结果已保存到 memory/distilled.md：{}", saved.join(", "));
            if !failed.is_empty() {
                msg.push_str(&format!("\n⚠ 失败：{}", failed.join(", ")));
            }
            self.session.push(Message::assistant(msg));
            self.message_scroll = 0;

            // Auto-chain: if more pending distillations exist, start the next one
            let remaining = check_distill_needed(&self.config.distill_needed_dir());
            if !remaining.is_empty() {
                let next_id = remaining[0].clone();
                self.session.push(Message::assistant(format!(
                    "还有 {} 个蒸馏待处理，继续下一个...", remaining.len()
                )));
                self.run_distillation(next_id).await;
                return;
            }
        } else {
            self.session.push(Message::assistant("已取消。".to_string()));
        }
        self.status_msg = "Ready".to_string();
    }

    // ── Confirm create skill ─────────────────────────────────────────────────

    async fn handle_confirm_create_skill(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        if !matches!(old_mode, AppMode::ConfirmCreateSkill) {
            return;
        }

        let confirmed = matches!(
            input.trim().to_lowercase().as_str(),
            "y" | "yes" | "确认" | "是"
        );

        if confirmed {
            // Start the create-skill wizard directly (avoid re-entering confirmation loop)
            self.session.push(Message::assistant(
                "━━━ 新建协议 Skill ━━━\nSkill 名称（英文小写连字符）？".to_string(),
            ));
            self.mode = AppMode::CreateSkill(CreateStage::AskName);
            self.status_msg = "Skill 名称".to_string();
        } else {
            if let Some(prompt) = self.codeword.cancellation_prompt(&Action::CreateSkill) {
                self.session.push(Message::assistant(prompt));
            } else {
                self.session.push(Message::assistant("已取消。".to_string()));
            }
            self.status_msg = "Ready".to_string();
        }
    }

    // ── /add-skill wizard ─────────────────────────────────────────────────────

    async fn handle_create_skill_input(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        let AppMode::CreateSkill(stage) = old_mode else { return };

        match stage {
            CreateStage::AskName => {
                let name = input.trim().to_lowercase().replace(' ', "-");
                if name.is_empty() {
                    self.session.push(Message::assistant("名称不能为空，请重新输入：".to_string()));
                    self.mode = AppMode::CreateSkill(CreateStage::AskName);
                    return;
                }
                self.session.push(Message::assistant("一句话描述（中文）？".to_string()));
                self.mode = AppMode::CreateSkill(CreateStage::AskDescription { name });
            }
            CreateStage::AskDescription { name } => {
                let desc = input.trim().to_string();
                self.session.push(Message::assistant(
                    "激活触发词，逗号分隔（可直接 Enter 跳过）？".to_string(),
                ));
                self.mode = AppMode::CreateSkill(CreateStage::AskTriggers { name, desc });
            }
            CreateStage::AskTriggers { name, desc } => {
                let triggers: Vec<String> = input
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();

                match self.create_protocol_skill_files(&name, &desc, &triggers) {
                    Ok(_) => {
                        self.available_skills = self.skill_store.scan_all();
                        self.session.push(Message::assistant(format!(
                            "✓ 已创建 @{name} 协议 Skill：\n\
                             ```\nskills/{name}/\n  skill.yaml\n  domain.md\n  artifacts.md\n  memory/failure-patterns.md\n```\n\n\
                             下一步：编辑 `skills/{name}/domain.md` 填写分析协议步骤。"
                        )));
                    }
                    Err(e) => {
                        self.session.push(Message::assistant(format!("❌ 创建失败：{e}")));
                    }
                }
            }
        }
    }

    fn create_protocol_skill_files(
        &self,
        name: &str,
        desc: &str,
        triggers: &[String],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("Skill 名称包含非法字符".into());
        }
        let skill_dir = self.config.skills_dir.join(name);
        fs::create_dir_all(skill_dir.join("memory"))?;

        let triggers_yaml = if triggers.is_empty() {
            "  # - \"触发词\"".to_string()
        } else {
            triggers.iter().map(|t| format!("  - \"{t}\"")).collect::<Vec<_>>().join("\n")
        };

        fs::write(
            skill_dir.join("skill.yaml"),
            format!(
                "name: {name}\nversion: 0.1.0\ndescription: {desc}\nauthor: \"\"\nlicense: MIT\ntier: workspace\n\n\
                 ring1_protocol: skills/{name}/domain.md\n\ntriggers:\n{triggers_yaml}\n\n\
                 memory:\n  - skills/{name}/memory/failure-patterns.md\n"
            ),
        )?;

        fs::write(
            skill_dir.join("domain.md"),
            format!(
                "# {desc}\n\n## 使用场景\n\n<!-- 描述什么情况下激活此 Skill -->\n\n\
                 ## 分析协议\n\n### Step 1：信息收集\n\n<!-- 描述第一步 -->\n\n\
                 ### Step 2：分析\n\n<!-- 描述分析步骤 -->\n\n\
                 ### Step 3：输出\n\n<!-- 描述输出格式，含假设清单和信息缺口 -->\n"
            ),
        )?;

        fs::write(
            skill_dir.join("artifacts.md"),
            format!(
                "# {desc} 产物规格\n\n## 输出前自检\n\n\
                 ```\n□ 分析是否完整？\n□ 假设是否已标注？\n□ 信息缺口是否已列出？\n```\n\n\
                 ## 执行文档\n\n<!-- 定义产物结构 -->\n"
            ),
        )?;

        fs::write(
            skill_dir.join("memory").join("failure-patterns.md"),
            format!(
                "# {desc} 失败模式库\n\n\
                 **加载时机：** 每次会话常驻加载\n\n\
                 ## 模式列表\n\n（暂无记录，蒸馏后自动填充）\n\n\
                 *本文件由蒸馏流程自动维护，请勿手动编辑已有条目。*\n"
            ),
        )?;

        Ok(())
    }

    // ── Codeword management ───────────────────────────────────────────────────

    async fn handle_add_codeword_input(&mut self, input: String) {
        let old_mode = std::mem::replace(&mut self.mode, AppMode::Chat);
        let AppMode::AddCodeword(stage) = old_mode else { return };

        match stage {
            CwStage::AskTrigger => {
                let trigger = input.trim().to_string();
                if trigger.is_empty() {
                    self.session.push(Message::assistant("触发词不能为空，请重新输入：".to_string()));
                    self.mode = AppMode::AddCodeword(CwStage::AskTrigger);
                    return;
                }
                self.session.push(Message::assistant(
                    "对应动作（输入数字）：\n\
                     1. 保存技能\n2. 查看技能库\n3. 启动飞轮/蒸馏\n\
                     4. 加载领域技能\n5. 查看暗号列表".to_string(),
                ));
                self.mode = AppMode::AddCodeword(CwStage::AskAction { trigger });
            }
            CwStage::AskAction { trigger } => {
                let action = match input.trim() {
                    "1" | "保存技能" => Some(Action::SaveSkill),
                    "2" | "查看技能库" => Some(Action::ListSkills),
                    "3" | "启动飞轮" | "蒸馏" => Some(Action::StartFlywheel),
                    "4" | "加载领域技能" => Some(Action::LoadDomain),
                    "5" | "查看暗号列表" => Some(Action::ListCodewords),
                    _ => None,
                };
                match action {
                    Some(act) => {
                        let label = action_label(&act);
                        self.codeword.add_alias_runtime(
                            &self.config.codewords_config_path(),
                            &trigger,
                            &act,
                        );
                        self.session.push(Message::assistant(format!(
                            "✓ 已添加暗号：「{trigger}」→ {label}"
                        )));
                    }
                    None => {
                        self.session.push(Message::assistant(
                            "未识别的动作，已取消。说「新增暗号」重试。".to_string(),
                        ));
                    }
                }
            }
            CwStage::RemoveTrigger => {
                let trigger = input.trim().to_string();
                if trigger.is_empty() || trigger == "n" {
                    self.session.push(Message::assistant("已取消。".to_string()));
                    return;
                }
                let path = self.config.codewords_config_path();
                self.codeword.remove_custom_alias(&path, &trigger);
                self.session.push(Message::assistant(format!(
                    "✓ 已删除自定义暗号「{trigger}」（内置暗号不受影响）。"
                )));
            }
        }
    }

    // ── Ring 1 Group chat ─────────────────────────────────────────────────────

    async fn trigger_group_chat(&mut self, skills: Vec<String>, material: String) {
        let project_id = self.ensure_project_id();

        let mut valid = Vec::new();
        let mut errors = Vec::new();
        for name in &skills {
            if self.available_skills.iter().any(|s| &s.name == name || &s.domain == name) {
                valid.push(name.clone());
            } else {
                errors.push(format!("❌ Skill '@{name}' 不存在"));
            }
        }
        if !errors.is_empty() {
            let avail: Vec<&str> = self.available_skills.iter().map(|s| s.name.as_str()).collect();
            errors.push(format!("可用: {}", avail.join(", ")));
            self.session.push(Message::assistant(errors.join("\n")));
            return;
        }

        self.session.push(Message::assistant(format!(
            "━━━ 群聊开启 | Skills: {} | 项目: {project_id} ━━━",
            valid.iter().map(|s| format!("@{s}")).collect::<Vec<_>>().join(" ")
        )));

        let first = valid[0].clone();
        let mat = material.clone();
        self.group_chat_state = Some(GroupChatState {
            project_id,
            skills: valid,
            material,
            current_idx: 0,
            responses: Vec::new(),
            total_assumptions: 0,
        });

        let (system, messages) = self.build_gc_skill_prompt(&first, &mat);
        self.mode = AppMode::GroupChatAnalyzing;
        self.start_stream(system, messages);
    }

    async fn trigger_group_chat_parallel(&mut self, skills: Vec<String>, material: String) {
        let project_id = self.ensure_project_id();

        // Validate all skills exist
        let mut valid = Vec::new();
        let mut errors = Vec::new();
        for name in &skills {
            if self.available_skills.iter().any(|s| &s.name == name || &s.domain == name) {
                valid.push(name.clone());
            } else {
                errors.push(format!("❌ Skill '@{name}' 不存在"));
            }
        }
        if !errors.is_empty() {
            let avail: Vec<&str> = self.available_skills.iter().map(|s| s.name.as_str()).collect();
            errors.push(format!("可用: {}", avail.join(", ")));
            self.session.push(Message::assistant(errors.join("\n")));
            return;
        }

        self.session.push(Message::assistant(format!(
            "━━━ 并行群聊开启 | Skills: {} | 项目: {project_id} ━━━",
            valid.iter().map(|s| format!("@{s}")).collect::<Vec<_>>().join(" ")
        )));

        // Build all prompts before spawning (avoid borrow in async task)
        let prompt_triples: Vec<(String, String, Vec<Message>)> = valid
            .iter()
            .map(|name| {
                let (system, msgs) = self.build_gc_skill_prompt(name, &material);
                (name.clone(), system, msgs)
            })
            .collect();

        // Clone config fields needed to build fresh LLM clients inside the task
        let provider = self.config.provider.clone();
        let api_key = self.config.api_key.clone();
        let model = self.config.model.clone();
        let base_url = self.config.base_url.clone();
        let event_tx = self.event_tx.clone();
        let skill_order = valid.clone();

        self.mode = AppMode::GroupChatAnalyzing;
        self.streaming = true;
        self.status_msg = "并行分析中...".to_string();

        // Pre-create cancel channels so we can cancel sub-streams from the main thread
        let mut cancel_rxs: Vec<oneshot::Receiver<()>> = Vec::new();
        self.parallel_cancels.clear();
        for _ in 0..prompt_triples.len() {
            let (tx, rx) = oneshot::channel::<()>();
            self.parallel_cancels.push(tx);
            cancel_rxs.push(rx);
        }

        let handle = tokio::spawn(async move {
            let mut join_set = tokio::task::JoinSet::new();

            for ((skill_name, system, messages), cancel_rx) in prompt_triples.into_iter().zip(cancel_rxs) {
                let llm = crate::llm::build(&provider, api_key.clone(), model.clone(), base_url.clone());
                let (tx, mut rx) = mpsc::channel::<AppEvent>(128);
                llm.start_stream(system, messages, tx, cancel_rx);

                join_set.spawn(async move {
                    let mut resp = String::new();
                    while let Some(ev) = rx.recv().await {
                        match ev {
                            AppEvent::StreamChunk(s) => resp.push_str(&s),
                            AppEvent::StreamDone | AppEvent::StreamError(_) => break,
                            _ => {}
                        }
                    }
                    (skill_name, resp)
                });
            }

            // Collect all results preserving original order
            let mut unordered = Vec::new();
            while let Some(res) = join_set.join_next().await {
                if let Ok(pair) = res {
                    unordered.push(pair);
                }
            }
            let responses: Vec<(String, String)> = skill_order
                .into_iter()
                .filter_map(|name| {
                    unordered
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|(n, r)| (n.clone(), r.clone()))
                })
                .collect();

            let _ = event_tx
                .send(AppEvent::ParallelAnalysisDone { project_id, responses })
                .await;
        });
        self.parallel_task = Some(handle);
    }

    fn build_gc_skill_prompt(&self, skill_name: &str, material: &str) -> (String, Vec<Message>) {
        let found = self
            .available_skills
            .iter()
            .find(|s| s.name == skill_name || s.domain == skill_name)
            .cloned();

        let (domain_content, memory_content, industry_pack, artifacts_content) = match found {
            Some(mut skill) => {
                self.skill_store.load_content(&mut skill);
                let domain = skill.content.clone().unwrap_or_default();
                let memory = self.skill_store.load_memory(&skill);
                let industry = self.skill_store.find_industry_pack(&skill, material);
                let artifacts = if skill.is_protocol {
                    fs::read_to_string(skill.path.join("artifacts.md")).ok()
                } else {
                    None
                };
                (domain, memory, industry, artifacts)
            }
            None => (String::new(), String::new(), None, None),
        };

        let mut system = format!(
            "你是 @{skill_name} 专家协议。严格按以下分析协议执行：\n\n{domain_content}"
        );
        if !memory_content.is_empty() {
            system.push_str(&format!("\n\n## 已知失败模式\n{memory_content}"));
        }
        if let Some(pack) = industry_pack {
            system.push_str(&format!("\n\n## 行业包\n{pack}"));
        }
        if let Some(art) = artifacts_content {
            system.push_str(&format!("\n\n## 产物规格（分析完成后按此格式输出）\n{art}"));
        }

        let messages = vec![Message::user(format!("请对以下材料执行分析：\n\n{material}"))];
        (system, messages)
    }

    fn build_synthesis_prompt(
        responses: &[(String, String)],
        skill_names: &[String],
    ) -> (String, Vec<Message>) {
        let system = format!(
            "你是 Telos 综合分析助手。整合以下 {} 个专家协议的分析结果。\n\
             严格按以下格式输出：\n共识：...\n分歧：...\n建议下一步：...",
            skill_names.len()
        );
        let mut analysis = String::new();
        for (name, resp) in responses {
            analysis.push_str(&format!("[@{name}]\n{resp}\n\n"));
        }
        let messages = vec![Message::user(format!("请综合以下专家分析：\n\n{analysis}"))];
        (system, messages)
    }

    // ── Ring 3 蒸馏 ───────────────────────────────────────────────────────────

    async fn run_distillation(&mut self, project_id: String) {
        let protocol_path = self.config.skills_dir.join("_kernel/distillation.md");
        let logs_path = self.config.logs_dir().join(format!("{project_id}.jsonl"));

        let protocol = fs::read_to_string(&protocol_path).unwrap_or_default();
        let logs = fs::read_to_string(&logs_path).unwrap_or_default();

        if protocol.is_empty() {
            self.session.push(Message::assistant(
                "⚠ 未找到 skills/_kernel/distillation.md，无法执行蒸馏协议。".to_string(),
            ));
            return;
        }
        if logs.is_empty() {
            self.session.push(Message::assistant(format!(
                "⚠ 未找到 episodic-logs/{project_id}.jsonl 日志文件。"
            )));
            return;
        }

        // Load project state to know which skills were involved
        let project_state = ProjectState::load(&self.config.projects_dir(), &project_id);
        let skills_involved: Vec<String> = project_state
            .map(|s| s.skills)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                // Fallback: use currently loaded skills
                self.loaded_skills.iter().map(|s| s.name.clone()).collect()
            });

        // Build skill context for the LLM
        let mut skill_context = String::new();
        for skill_name in &skills_involved {
            if let Some(mut skill) = self
                .available_skills
                .iter()
                .find(|s| s.name == *skill_name || s.domain == *skill_name)
                .cloned()
            {
                self.skill_store.load_content(&mut skill);
                let memory = self.skill_store.load_memory(&skill);
                skill_context.push_str(&format!(
                    "\n### @{skill_name}\n**domain.md：**\n{}\n\n**memory：**\n{}\n",
                    skill.content.as_deref().unwrap_or("（空）"),
                    if memory.is_empty() { "（无）".to_string() } else { memory }
                ));
            }
        }

        let skills_label = if skills_involved.is_empty() {
            "（未知）".to_string()
        } else {
            skills_involved.iter().map(|s| format!("@{s}")).collect::<Vec<_>>().join(", ")
        };

        let system = format!("你是蒸馏助手。严格按以下蒸馏协议执行：\n\n{protocol}");
        let messages = vec![Message::user(format!(
            "项目 {project_id}，涉及 Skills：{skills_label}\n\n\
             ## Skills 当前状态\n{skill_context}\n\n\
             ## 事件日志\n```jsonl\n{logs}\n```\n\n\
             请提炼失败模式和改进建议。\
             每个 Skill 单独一节（标题格式：`## @skill_name`），\
             输出可直接追加到该 Skill memory/ 的 Markdown 内容。"
        ))];

        self.session.push(Message::assistant(format!(
            "开始蒸馏项目 {project_id}（Skills: {skills_label}）..."
        )));
        self.mode = AppMode::DistillProject { project_id, skills: skills_involved };
        self.start_stream(system, messages);
    }

    // ── Codeword actions ──────────────────────────────────────────────────────

    async fn handle_action(&mut self, action: Action, message: &str) {
        match action {
            Action::SaveSkill => {
                self.start_auto_save();
            }

            Action::ListSkills => {
                self.session.push(Message::assistant(self.build_skill_list()));
            }

            Action::ListCodewords => {
                self.session.push(Message::assistant(self.build_codeword_list()));
            }

            Action::StartFlywheel => {
                let pending = check_distill_needed(&self.config.distill_needed_dir());
                if !pending.is_empty() {
                    let project_id = pending[0].clone();
                    self.run_distillation(project_id).await;
                } else {
                    self.session.push(Message::assistant(
                        "飞轮启动。提供材料或用 @skill1 @skill2 材料 触发群聊分析。".to_string(),
                    ));
                }
            }

            Action::LoadDomain => {
                let domain = extract_domain_name(message)
                    .or_else(|| {
                        self.available_skills
                            .iter()
                            .find(|s| !s.is_protocol && message.contains(&s.domain))
                            .map(|s| s.domain.clone())
                    });

                if let Some(domain) = domain {
                    let to_load: Vec<Skill> = self
                        .available_skills
                        .iter()
                        .filter(|s| s.domain == domain && !s.is_protocol)
                        .cloned()
                        .collect();

                    let count = to_load.len();
                    let names: Vec<String> = to_load.iter().map(|s| s.name.clone()).collect();
                    for mut skill in to_load {
                        if !self.loaded_skills.iter().any(|s| s.name == skill.name) {
                            self.skill_store.load_content(&mut skill);
                            self.loaded_skills.push(skill);
                        }
                    }
                    self.session.push(Message::assistant(format!(
                        "✓ 已加载 [{domain}] 知识技能：{}（共 {} 个）\n接下来以你已掌握这些技能为前提对话。",
                        names.join("、"),
                        count
                    )));
                } else {
                    let domains: Vec<String> = {
                        let mut seen = std::collections::BTreeSet::new();
                        self.available_skills.iter()
                            .filter(|s| !s.is_protocol)
                            .map(|s| s.domain.clone())
                            .filter(|d| seen.insert(d.clone()))
                            .collect()
                    };
                    self.session.push(Message::assistant(format!(
                        "加载哪个领域？可用领域：{}\n（例：加载技能 cooking）",
                        if domains.is_empty() { "（暂无知识技能）".to_string() } else { domains.join(", ") }
                    )));
                }
            }

            Action::DeleteSkill => {
                if let Some((domain, name)) = extract_skill_ref(message) {
                    self.session.push(Message::assistant(format!(
                        "确认删除 {domain}/{name}？此操作不可恢复。(y / n)"
                    )));
                    self.mode = AppMode::ConfirmDelete { domain, name };
                    self.status_msg = "确认删除？(y / n)".to_string();
                } else {
                    let list = self.build_skill_list();
                    self.session.push(Message::assistant(format!(
                        "删除哪个技能？格式：删除技能 domain/name\n\n{list}"
                    )));
                }
            }

            Action::ShowSkill => {
                if let Some((domain, name)) = extract_skill_ref(message) {
                    let found = self
                        .available_skills
                        .iter()
                        .find(|s| s.domain == domain && s.name == name)
                        .cloned();

                    match found {
                        Some(mut skill) => {
                            self.skill_store.load_content(&mut skill);
                            let kind = if skill.is_protocol { "⚡ 协议 Skill" } else { "📚 知识 Skill" };
                            let body = skill.content.as_deref().unwrap_or("（内容为空）");
                            self.session.push(Message::assistant(format!(
                                "# {kind}: {domain}/{name}\n\n{body}"
                            )));
                        }
                        None => {
                            self.session.push(Message::assistant(format!(
                                "❌ 未找到技能 {domain}/{name}\n说「看看家底」查看所有技能。"
                            )));
                        }
                    }
                } else {
                    self.session.push(Message::assistant(
                        "查看哪个技能？格式：查看技能 domain/name".to_string(),
                    ));
                }
            }

            Action::CreateSkill => {
                self.session.push(Message::assistant(
                    "━━━ 新建协议 Skill ━━━\n确认创建新 Skill？（y/n）".to_string(),
                ));
                self.mode = AppMode::ConfirmCreateSkill;
                self.status_msg = "确认创建 Skill".to_string();
            }

            Action::AddCodeword => {
                self.session.push(Message::assistant(
                    "新增暗号 → 触发词（说出这个词时触发，如「码住」）？".to_string(),
                ));
                self.mode = AppMode::AddCodeword(CwStage::AskTrigger);
                self.status_msg = "新增暗号（Esc 取消）".to_string();
            }

            Action::EditCodeword => {
                self.session.push(Message::assistant(self.build_codeword_list()));
                self.session.push(Message::assistant(
                    "说「新增暗号」可添加别名。直接编辑 ~/.config/telos/codewords.json 可完整管理。".to_string(),
                ));
            }

            Action::RemoveCodeword => {
                let path = self.config.codewords_config_path();
                let text = fs::read_to_string(&path).unwrap_or_default();
                if text.trim().is_empty() || text.trim() == "{}" {
                    self.session.push(Message::assistant(
                        "没有自定义暗号可删除（内置暗号无法删除）。".to_string(),
                    ));
                } else {
                    let map: std::collections::HashMap<String, Vec<String>> =
                        serde_json::from_str(&text).unwrap_or_default();
                    let mut list = String::from("当前自定义暗号：\n");
                    for (action_key, aliases) in &map {
                        for alias in aliases {
                            list.push_str(&format!("  • 「{alias}」→ {action_key}\n"));
                        }
                    }
                    list.push_str("\n输入要删除的触发词（Esc 取消）：");
                    self.session.push(Message::assistant(list));
                    self.mode = AppMode::AddCodeword(CwStage::RemoveTrigger);
                    self.status_msg = "删除暗号（Esc 取消）".to_string();
                }
            }

            Action::Stats => {
                self.session.push(Message::assistant(self.build_stats_report()));
            }

            Action::ExportSkill => {
                if let Some((domain, name)) = extract_skill_ref(message) {
                    let found = self.available_skills.iter()
                        .find(|s| s.domain == domain && s.name == name)
                        .cloned();
                    match found {
                        Some(mut skill) => {
                            self.skill_store.load_content(&mut skill);
                            let (conv, distill, projects) = self.skill_store.skill_evolution_info(&skill);
                            let memory_dir = if skill.is_protocol {
                                skill.path.join("memory")
                            } else {
                                skill.path.parent()
                                    .map(|p| p.join("memory"))
                                    .unwrap_or_default()
                            };
                            let memory_count = fs::read_dir(&memory_dir)
                                .map(|entries| entries.filter_map(|e| e.ok())
                                    .filter(|e| e.path().extension()
                                        .and_then(|x| x.to_str()) == Some("md"))
                                    .count())
                                .unwrap_or(0);
                            let body = skill.content.as_deref().unwrap_or("（空）");
                            let mut msg = format!(
                                "# 导出预览: {domain}/{name}\n\n```\n{}\n```\n\n",
                                truncate(body, 1000)
                            );
                            let mut losses = Vec::new();
                            if conv > 1 { losses.push(format!("{conv} 次对话进化历史")); }
                            if distill > 0 { losses.push(format!("{distill} 次蒸馏结果")); }
                            if memory_count > 0 {
                                losses.push(format!("{memory_count} 个记忆文件（失败模式 + 蒸馏）"));
                            }
                            if projects > 0 { losses.push(format!("{projects} 个项目的实战反馈")); }
                            if losses.is_empty() {
                                msg.push_str("ℹ 此技能尚无积累历史，导出不会损失上下文。");
                            } else {
                                msg.push_str(&format!(
                                    "⚠ 导出仅包含协议正文，以下上下文**不随文件迁移**：\n{}\n\n\
                                     留在 Telos 的版本持续进化；导出版本在此刻凝固。",
                                    losses.iter().map(|l| format!("  • {l}")).collect::<Vec<_>>().join("\n")
                                ));
                            }
                            self.session.push(Message::assistant(msg));
                        }
                        None => {
                            self.session.push(Message::assistant(format!(
                                "❌ 未找到技能 {domain}/{name}。说「看看家底」查看所有技能。"
                            )));
                        }
                    }
                } else {
                    self.session.push(Message::assistant(
                        "导出哪个技能？格式：导出技能 domain/name\n例：导出技能 cooking/braising".to_string()
                    ));
                }
            }
        }
    }

    // ── Project helpers ───────────────────────────────────────────────────────

    fn ensure_project_id(&mut self) -> String {
        if let Some(id) = &self.current_project_id {
            return id.clone();
        }
        let projects_dir = self.config.projects_dir();
        let _ = fs::create_dir_all(&projects_dir);
        let id = ProjectState::generate_id(&projects_dir);
        let state = ProjectState::new(id.clone());
        let _ = state.save(&projects_dir);
        self.current_project_id = Some(id.clone());
        id
    }

    fn log_gc_event(&self, project_id: &str, skills: &[String]) {
        let logger = EpisodicLogger::new(
            project_id.to_string(),
            self.config.logs_dir(),
            self.config.distill_needed_dir(),
        );
        logger.log(serde_json::json!({
            "type": "group_chat",
            "project_id": project_id,
            "skills": skills,
            "timestamp": timestamp_now(),
            "quality_score": 0.7,
        }));
    }

    fn log_gc_synthesis(&self, project_id: &str, synthesis: &str) {
        let logger = EpisodicLogger::new(
            project_id.to_string(),
            self.config.logs_dir(),
            self.config.distill_needed_dir(),
        );
        logger.log(serde_json::json!({
            "type": "synthesis",
            "project_id": project_id,
            "timestamp": timestamp_now(),
            "summary": truncate(synthesis, 2000),
            "quality_score": 0.8,
        }));
    }

    fn check_distill_trigger(&self, project_id: &str) {
        let logger = EpisodicLogger::new(
            project_id.to_string(),
            self.config.logs_dir(),
            self.config.distill_needed_dir(),
        );
        let single = logger.quality_event_count();
        let cross = projects_with_quality_events(&self.config.logs_dir());

        if single >= 3 || cross >= 3 {
            logger.mark_distill_needed();
        }
    }

    // ── Stream helper ─────────────────────────────────────────────────────────

    fn start_stream(&mut self, system: String, messages: Vec<Message>) {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        self.cancel_tx = Some(cancel_tx);
        self.streaming = true;
        self.status_msg = "Thinking…".to_string();
        self.llm.start_stream(system, messages, self.event_tx.clone(), cancel_rx);
    }

    fn build_system_prompt(&self) -> String {
        let mut parts = vec![
            "你是 Telos，支持技能积累和多专家协作的个人助手。用中文回答，简洁直接。".to_string(),
        ];
        for skill in &self.loaded_skills {
            if let Some(content) = &skill.content {
                parts.push(format!(
                    "\n---\n# 已加载技能: {}/{}\n\n{content}",
                    skill.domain, skill.name
                ));
            }
        }
        parts.join("\n")
    }

    fn build_skill_list(&self) -> String {
        let mut lines = vec!["━━━ 技能库 ━━━\n".to_string()];

        let protocols: Vec<&Skill> = self.available_skills.iter().filter(|s| s.is_protocol).collect();
        let knowledge: Vec<&Skill> = self.available_skills.iter().filter(|s| !s.is_protocol).collect();

        if !protocols.is_empty() {
            lines.push(format!("▶ 协议 Skills（{}个）", protocols.len()));
            for s in &protocols {
                let loaded = if self.loaded_skills.iter().any(|l| l.name == s.name) { " ✓" } else { "" };
                let (conv, distill, _) = self.skill_store.skill_evolution_info(s);
                let evo = evolution_tag(conv, distill);
                lines.push(format!("  ⚡ @{}{loaded}{evo}  —  {}", s.name, s.summary));
            }
            lines.push(String::new());
        }

        if !knowledge.is_empty() {
            lines.push(format!("📚 知识 Skills（{}个）", knowledge.len()));
            let mut cur_domain = String::new();
            for s in &knowledge {
                if s.domain != cur_domain {
                    cur_domain = s.domain.clone();
                    lines.push(format!("  [{}]", cur_domain));
                }
                let loaded = if self.loaded_skills.iter().any(|l| l.name == s.name) { " ✓" } else { "" };
                let (conv, distill, _) = self.skill_store.skill_evolution_info(s);
                let evo = evolution_tag(conv, distill);
                lines.push(format!("    @{}{loaded}{evo}  —  {}", s.name, s.summary));
            }
        }

        if self.available_skills.is_empty() {
            lines.push("（暂无技能）说「记下来」可保存当前对话。".to_string());
        }

        lines.push("\n用 @skill 召集  @s1 @s2 材料 群聊  @all 全部加载  @? 显示此列表".to_string());
        lines.push("说「使用统计」查看进化报告  说「导出技能 d/n」查看上下文损失".to_string());
        lines.push("━━━━━━━━━━━━━━".to_string());
        lines.join("\n")
    }

    fn build_codeword_list(&self) -> String {
        let mut lines = vec!["━━━ 暗号大全 ━━━\n".to_string()];
        for rule in self.codeword.all_rules() {
            let label = action_label(&rule.action);
            let mut all_phrases = rule.exact_phrases.clone();
            all_phrases.extend(rule.fuzzy_phrases.clone());
            lines.push(format!(
                "• {}  →  {label}",
                all_phrases.iter().map(|s| format!("「{s}」")).collect::<Vec<_>>().join(" / ")
            ));
        }
        lines.push("\n说「新增暗号」可自定义触发词".to_string());
        lines.push("━━━━━━━━━━━━━━".to_string());
        lines.join("\n")
    }

    fn activate_skills(&mut self, names: &[String]) {
        for name in names {
            let found = self
                .available_skills
                .iter()
                .find(|s| &s.name == name || &s.domain == name)
                .cloned();
            match found {
                Some(mut skill) => {
                    if !self.loaded_skills.iter().any(|s| s.name == skill.name) {
                        self.skill_store.load_content(&mut skill);
                        let (conv, distill, projects) = self.skill_store.skill_evolution_info(&skill);
                        let evo = if conv > 0 || distill > 0 {
                            let mut parts = Vec::new();
                            if conv > 0 { parts.push(format!("对话×{conv}")); }
                            if distill > 0 { parts.push(format!("蒸馏×{distill}")); }
                            if projects > 0 { parts.push(format!("{projects}个项目")); }
                            format!("  [{}]", parts.join(" · "))
                        } else {
                            String::new()
                        };
                        self.status_msg = format!("@{name} 已加载{evo}");
                        self.loaded_skills.push(skill);
                    }
                }
                None => {
                    let avail: Vec<&str> = self.available_skills.iter().map(|s| s.name.as_str()).collect();
                    self.status_msg = format!("❌ @{name} 不存在  可用: {}", avail.join(", "));
                }
            }
        }
    }

    fn activate_all_skills(&mut self) {
        let to_load: Vec<Skill> = self
            .available_skills
            .iter()
            .filter(|s| !self.loaded_skills.iter().any(|l| l.name == s.name))
            .cloned()
            .collect();
        let count = to_load.len();
        for mut skill in to_load {
            self.skill_store.load_content(&mut skill);
            self.loaded_skills.push(skill);
        }
        self.status_msg = if count > 0 {
            format!("已加载全部 {count} 个技能")
        } else {
            "所有技能已加载".to_string()
        };
    }

    // ── Stats & usage tracking ────────────────────────────────────────────────

    fn track_action_usage(&self, action: &Action) {
        let key = match action {
            Action::SaveSkill => "save_skill",
            Action::ListSkills => "list_skills",
            Action::ListCodewords => "list_codewords",
            Action::StartFlywheel => "start_flywheel",
            Action::LoadDomain => "load_domain",
            Action::DeleteSkill => "delete_skill",
            Action::ShowSkill => "show_skill",
            Action::CreateSkill => "create_skill",
            Action::AddCodeword => "add_codeword",
            Action::EditCodeword => "edit_codeword",
            Action::RemoveCodeword => "remove_codeword",
            Action::Stats => "stats",
            Action::ExportSkill => "export_skill",
        };
        let path = self.config.stats_path();
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut v: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|_| serde_json::json!({"codewords": {}}));
        let count = v["codewords"][key].as_u64().unwrap_or(0) + 1;
        v["codewords"][key] = serde_json::json!(count);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default());
    }

    fn build_stats_report(&self) -> String {
        let mut lines = vec!["━━━ 使用统计 & 进化报告 ━━━\n".to_string()];

        // Inventory
        let proto_count = self.available_skills.iter().filter(|s| s.is_protocol).count();
        let know_count = self.available_skills.iter().filter(|s| !s.is_protocol).count();
        lines.push(format!("📦 技能积累：协议 {} 个 · 知识 {} 个", proto_count, know_count));

        // Per-skill evolution
        let mut skill_stats: Vec<(String, u32, u32, usize)> = self.available_skills
            .iter()
            .map(|s| {
                let (conv, distill, projects) = self.skill_store.skill_evolution_info(s);
                (s.name.clone(), conv, distill, projects)
            })
            .filter(|(_, conv, distill, _)| *conv > 0 || *distill > 0)
            .collect();
        skill_stats.sort_by(|a, b| (b.1 + b.2 * 3).cmp(&(a.1 + a.2 * 3)));

        if !skill_stats.is_empty() {
            let total_conv: u32 = skill_stats.iter().map(|s| s.1).sum();
            let total_distill: u32 = skill_stats.iter().map(|s| s.2).sum();
            lines.push(format!("📈 累计 {} 次对话 · {} 次蒸馏\n", total_conv, total_distill));
            lines.push("技能进化排行：".to_string());
            for (name, conv, distill, projects) in &skill_stats {
                let mut parts = Vec::new();
                if *conv > 0 { parts.push(format!("对话×{conv}")); }
                if *distill > 0 { parts.push(format!("蒸馏×{distill}")); }
                if *projects > 0 { parts.push(format!("{projects}个项目")); }
                lines.push(format!("  @{name}  —  {}", parts.join(" · ")));
            }
        } else {
            lines.push("（暂无进化数据，开始使用后将显示统计）".to_string());
        }

        // Skill combos from episodic logs
        let combos = skill_combos_from_logs(&self.config.logs_dir());
        if !combos.is_empty() {
            lines.push("\n常用技能组合：".to_string());
            for (combo, count) in combos.iter().take(5) {
                let display = combo.split('+')
                    .map(|s| format!("@{s}"))
                    .collect::<Vec<_>>()
                    .join(" + ");
                lines.push(format!("  {display}  ×{count}"));
            }
        }

        // Codeword usage
        let stats_path = self.config.stats_path();
        if let Ok(text) = fs::read_to_string(&stats_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(cw_obj) = v["codewords"].as_object() {
                    let mut cw_stats: Vec<(String, u64)> = cw_obj
                        .iter()
                        .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
                        .collect();
                    cw_stats.sort_by(|a, b| b.1.cmp(&a.1));
                    if !cw_stats.is_empty() {
                        lines.push("\n🔑 暗号使用频率：".to_string());
                        let labels: &[(&str, &str)] = &[
                            ("save_skill", "记下来 / 保存技能"),
                            ("list_skills", "看看家底"),
                            ("start_flywheel", "启动飞轮 / 复盘"),
                            ("load_domain", "加载技能"),
                            ("list_codewords", "暗号大全"),
                            ("delete_skill", "删除技能"),
                            ("show_skill", "查看技能"),
                            ("create_skill", "新建协议"),
                            ("add_codeword", "新增暗号"),
                            ("stats", "使用统计"),
                            ("export_skill", "导出技能"),
                        ];
                        for (key, count) in &cw_stats {
                            let label = labels.iter()
                                .find(|(k, _)| k == key)
                                .map(|(_, l)| *l)
                                .unwrap_or(key.as_str());
                            lines.push(format!("  「{label}」×{count}"));
                        }
                    }
                }
            }
        }

        lines.push("\n━━━━━━━━━━━━━━".to_string());
        lines.join("\n")
    }
}

// ── Startup helpers ───────────────────────────────────────────────────────────

fn check_distill_needed(distill_dir: &std::path::Path) -> Vec<String> {
    fs::read_dir(distill_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_str() != Some(".gitkeep"))
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

// ── JSON skill draft extraction ───────────────────────────────────────────────

/// Find a balanced `{}` block starting from the first `{`, respecting string literals.
fn extract_json_block(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let rest = &text[start..];
    for (i, c) in rest.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(rest[..=i].to_string());
                    }
                }
                '"' => in_string = true,
                _ => {}
            }
        }
    }
    None
}

fn extract_skill_draft(text: &str) -> Option<SkillDraft> {
    // Try ```json ... ``` block first, then bare JSON
    let json_str = if let Some(s) = text.find("```json") {
        let after = &text[s + 7..];
        let end = after.find("```")?;
        after[..end].trim().to_string()
    } else if text.find('{').is_some() {
        extract_json_block(text)?
    } else {
        return None;
    };

    let v: serde_json::Value = serde_json::from_str(&json_str).ok()?;

    let domain = v["domain"].as_str()?.trim().to_lowercase().replace(' ', "-");
    let name = v["name"].as_str()?.trim().to_lowercase().replace(' ', "-");
    let summary = v["summary"].as_str().unwrap_or("").trim().to_string();
    let content = v["content"].as_str()?.trim().to_string();
    let tags: Vec<String> = v["tags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(|t| t.trim().to_string())
                .collect()
        })
        .unwrap_or_default();

    if domain.is_empty() || name.is_empty() || content.is_empty() {
        return None;
    }

    Some(SkillDraft { domain, name, summary, tags, content })
}

// ── Miscellaneous helpers ─────────────────────────────────────────────────────

fn count_assumptions(text: &str) -> u32 {
    text.lines()
        .filter(|l| {
            let l = l.trim().to_lowercase();
            l.starts_with("□ 假设")
                || l.starts_with("- 假设")
                || l.contains("假设：")
                || l.starts_with("assume ")
                || l.starts_with("assumption:")
        })
        .count() as u32
}

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", chars[..max_chars].iter().collect::<String>())
    }
}

fn prev_char_boundary(s: &str, mut pos: usize) -> usize {
    if pos == 0 { return 0; }
    pos -= 1;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Compact evolution tag for skill lists: " [×12|蒸3]" or empty if no history.
fn evolution_tag(conv: u32, distill: u32) -> String {
    if conv == 0 && distill == 0 {
        return String::new();
    }
    let mut parts = Vec::new();
    if conv > 0 { parts.push(format!("×{conv}")); }
    if distill > 0 { parts.push(format!("蒸{distill}")); }
    format!(" [{}]", parts.join("|"))
}

/// Scan episodic logs for group_chat events and count skill-pair combos.
fn skill_combos_from_logs(logs_dir: &std::path::Path) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let entries = match fs::read_dir(logs_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("jsonl") { continue; }
        let content = fs::read_to_string(&path).unwrap_or_default();
        for line in content.lines() {
            if line.trim().is_empty() { continue; }
            let v: serde_json::Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
            if v["type"] != "group_chat" { continue; }
            if let Some(arr) = v["skills"].as_array() {
                let mut names: Vec<&str> = arr.iter().filter_map(|s| s.as_str()).collect();
                if names.len() >= 2 {
                    names.sort_unstable();
                    *counts.entry(names.join("+")).or_default() += 1;
                }
            }
        }
    }
    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}

// ── Mode display helpers ──────────────────────────────────────────────────────

impl AppMode {
    pub fn input_prompt(&self) -> &'static str {
        match self {
            AppMode::Chat => " 输入消息  ↑↓ 历史  Ctrl+Q 退出 ",
            AppMode::AutoSave => " 推断中... ",
            AppMode::ConfirmSave(_) => " y 确认保存  n 取消 ",
            AppMode::ConfirmDelete { .. } => " y 确认删除  n 取消 ",
            AppMode::Distilling { .. } => " 蒸馏中... ",
            AppMode::ConfirmDistill { .. } => " y 应用更新  n 取消 ",
            AppMode::DistillProject { .. } => " Ring 3 蒸馏中... ",
            AppMode::ConfirmDistillProject { .. } => " y 保存蒸馏结果  n 取消 ",
            AppMode::GroupChatAnalyzing => " Ring 1 分析中... ",
            AppMode::GroupChatSynthesizing => " Ring 1 综合中... ",
            AppMode::ConfirmCreateSkill => " y 确认创建  n 取消 ",
            AppMode::CreateSkill(CreateStage::AskName) => " Skill 名称 ",
            AppMode::CreateSkill(CreateStage::AskDescription { .. }) => " 一句话描述 ",
            AppMode::CreateSkill(CreateStage::AskTriggers { .. }) => " 触发词，逗号分隔 ",
            AppMode::AddCodeword(CwStage::AskTrigger) => " 触发词 ",
            AppMode::AddCodeword(CwStage::AskAction { .. }) => " 动作编号 (1-5) ",
            AppMode::AddCodeword(CwStage::RemoveTrigger) => " 输入要删除的触发词 ",
        }
    }
}

/// Render a simple ASCII progress bar: ████░░░░░░
fn bar_for(total: usize, filled: usize) -> String {
    let mut out = String::with_capacity(total);
    for i in 0..total {
        out.push(if i < filled { '█' } else { '░' });
    }
    out
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_for() {
        assert_eq!(bar_for(10, 0), "░░░░░░░░░░");
        assert_eq!(bar_for(10, 5), "█████░░░░░");
        assert_eq!(bar_for(10, 10), "██████████");
        assert_eq!(bar_for(10, 15), "██████████"); // clamped to total
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello…");
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_prev_char_boundary() {
        assert_eq!(prev_char_boundary("hello", 0), 0);
        assert_eq!(prev_char_boundary("hello", 1), 0);
        assert_eq!(prev_char_boundary("hello", 5), 4);
        // Test with multi-byte UTF-8 character
        let s = "你好";
        assert_eq!(prev_char_boundary(s, s.len()), 3);
    }

    #[test]
    fn test_evolution_tag() {
        assert_eq!(evolution_tag(0, 0), "");
        assert_eq!(evolution_tag(5, 0), " [×5]");
        assert_eq!(evolution_tag(0, 3), " [蒸3]");
        assert_eq!(evolution_tag(5, 3), " [×5|蒸3]");
    }
}
