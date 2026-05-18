/// Slash-command parser and file-reference extractor.
///
/// `/command`  — executed immediately, bypasses LLM.
/// `#./path`   — file content injected into the API message context.

// ── Slash commands ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SlashCommand {
    Help,
    Clear,
    Skills,
    Load(Option<String>),
    Save,
    Show(Option<String>),
    Delete(Option<String>),
    Export(Option<String>),
    Distill,
    Unknown(String),
}

pub struct CommandSpec {
    pub name: &'static str,
    pub args: &'static str,
    pub desc: &'static str,
}

pub const ALL: &[CommandSpec] = &[
    CommandSpec { name: "help",    args: "",         desc: "显示此帮助" },
    CommandSpec { name: "clear",   args: "",         desc: "清空对话" },
    CommandSpec { name: "skills",  args: "",         desc: "列出所有 skill" },
    CommandSpec { name: "load",    args: "<skill>",  desc: "加载指定 skill/领域" },
    CommandSpec { name: "save",    args: "",         desc: "将对话保存为 skill" },
    CommandSpec { name: "show",    args: "<d/name>", desc: "查看 skill 内容" },
    CommandSpec { name: "delete",  args: "<d/name>", desc: "删除 skill" },
    CommandSpec { name: "export",  args: "<d/name>", desc: "导出 skill（含上下文提示）" },
    CommandSpec { name: "distill", args: "",         desc: "启动蒸馏飞轮" },
];

/// Parse a slash command from `input` (must start with `/`).
/// Returns `None` if `input` does not start with `/`.
pub fn parse(input: &str) -> Option<SlashCommand> {
    if !input.starts_with('/') {
        return None;
    }
    let rest = &input[1..];
    let mut parts = rest.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").trim().to_lowercase();
    let arg = parts
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some(match cmd.as_str() {
        "help" | "h" => SlashCommand::Help,
        "clear" => SlashCommand::Clear,
        "skills" | "ls" | "list" => SlashCommand::Skills,
        "load" => SlashCommand::Load(arg),
        "save" => SlashCommand::Save,
        "show" => SlashCommand::Show(arg),
        "delete" | "del" | "rm" => SlashCommand::Delete(arg),
        "export" => SlashCommand::Export(arg),
        "distill" => SlashCommand::Distill,
        other => SlashCommand::Unknown(other.to_string()),
    })
}

/// Return all `CommandSpec`s whose name starts with `prefix`.
pub fn completions(prefix: &str) -> Vec<&'static CommandSpec> {
    ALL.iter().filter(|c| c.name.starts_with(prefix)).collect()
}

// ── File references ───────────────────────────────────────────────────────────

/// Parse `#path` tokens from `message`, read each file, and return:
/// - `display`: message with `#path` replaced by `[📎 path]`
/// - `refs`: list of `(path, file_content)` for readable files
///
/// A token qualifies as a file path when it starts with `.`, `/`, `~`,
/// or contains `/` (ruling out plain `#hashtags`).
pub fn extract_file_refs(message: &str) -> (String, Vec<(String, String)>) {
    let mut refs: Vec<(String, String)> = Vec::new();
    let mut display = String::with_capacity(message.len());
    let chars: Vec<char> = message.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '#' {
            display.push(chars[i]);
            i += 1;
            continue;
        }

        // Collect the token after '#' up to the next whitespace
        let start = i + 1;
        let mut end = start;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        let token: String = chars[start..end].iter().collect();

        let is_path = !token.is_empty()
            && (token.starts_with('.')
                || token.starts_with('/')
                || token.starts_with('~')
                || token.contains('/'));

        if is_path {
            let resolved = resolve_tilde(&token);
            match std::fs::read_to_string(&resolved) {
                Ok(content) => {
                    refs.push((token.clone(), content));
                    display.push_str(&format!("[📎 {token}]"));
                }
                Err(_) => {
                    // Unreadable path — keep original
                    display.push('#');
                    display.push_str(&token);
                }
            }
        } else {
            // Plain hashtag — keep as-is
            display.push('#');
            display.push_str(&token);
        }

        i = end;
    }

    (display, refs)
}

fn resolve_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}{rest}");
        }
    }
    path.to_string()
}

/// Build the file-context block appended to the API user message.
pub fn file_context_block(refs: &[(String, String)]) -> String {
    if refs.is_empty() {
        return String::new();
    }
    let mut block = String::from("\n\n--- 附件文件内容 ---\n");
    for (path, content) in refs {
        block.push_str(&format!("\n### `{path}`\n```\n{content}\n```\n"));
    }
    block
}
