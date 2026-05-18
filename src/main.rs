mod app;
mod command;
mod config;
mod codeword;
mod project;
mod session;
mod llm;
mod skill;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use app::{App, AppEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let config = config::Config::load()?;

    // CLI subcommands
    if args.len() >= 2 {
        match args[1].as_str() {
            "stats" => {
                print_stats(&config)?;
                return Ok(());
            }
            _ => {
                eprintln!("Unknown command: {}", args[1]);
                eprintln!("Usage: telos [stats]");
                std::process::exit(1);
            }
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::channel::<AppEvent>(256);

    // Terminal events live in a blocking thread — crossterm::event::poll is blocking
    let tx_keys = tx.clone();
    tokio::task::spawn_blocking(move || loop {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if tx_keys.blocking_send(AppEvent::Key(key)).is_err() {
                    break;
                }
            }
        }
    });

    let mut app = App::new(config, tx)?;

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        // Drain all pending events without blocking the render loop
        loop {
            match rx.try_recv() {
                Ok(event) => app.handle_event(event).await,
                Err(_) => break,
            }
        }

        if app.should_quit {
            break;
        }

        tokio::time::sleep(Duration::from_millis(16)).await; // ~60 fps
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn print_stats(config: &config::Config) -> Result<(), Box<dyn std::error::Error>> {
    use skill::store::SkillStore;

    let skill_store = SkillStore::new(config.skills_dir.clone());
    let skills = skill_store.scan_all();

    let days = project::days_since_first_use(&config.skills_dir);
    let skill_count = skills.len();
    let proj_count = project::project_count(&config.projects_dir());
    let quality_events = project::total_quality_events(&config.logs_dir());
    let distill_count = project::total_distillations(&config.skills_dir);

    println!("━━━ Telos 使用统计 & 进化报告 ━━━");
    println!();
    println!("📊 资产总览：");
    println!("  使用天数:    {} 天", days);
    println!("  技能总数:    {} 个", skill_count);
    println!("  项目总数:    {} 个", proj_count);
    println!("  优质反馈:    {} 条", quality_events);
    println!("  蒸馏记录:    {} 次", distill_count);
    println!();

    let proto_count = skills.iter().filter(|s| s.is_protocol).count();
    let know_count = skills.iter().filter(|s| !s.is_protocol).count();
    println!("📦 技能构成：协议 {} 个 · 知识 {} 个", proto_count, know_count);

    // Per-skill evolution
    let mut skill_stats: Vec<(String, u32, u32, usize)> = skills
        .iter()
        .map(|s| {
            let (conv, distill, projects) = skill_store.skill_evolution_info(s);
            (s.name.clone(), conv, distill, projects)
        })
        .filter(|(_, conv, distill, _)| *conv > 0 || *distill > 0)
        .collect();
    skill_stats.sort_by(|a, b| (b.1 + b.2 * 3).cmp(&(a.1 + a.2 * 3)));

    if !skill_stats.is_empty() {
        let total_conv: u32 = skill_stats.iter().map(|s| s.1).sum();
        let total_distill: u32 = skill_stats.iter().map(|s| s.2).sum();
        println!();
        println!("📈 累计 {} 次对话 · {} 次蒸馏", total_conv, total_distill);
        println!("技能进化排行：");
        for (name, conv, distill, projects) in &skill_stats {
            let mut parts = Vec::new();
            if *conv > 0 { parts.push(format!("对话×{conv}")); }
            if *distill > 0 { parts.push(format!("蒸馏×{distill}")); }
            if *projects > 0 { parts.push(format!("{projects}个项目")); }
            println!("  @{}  —  {}", name, parts.join(" · "));
        }
    } else {
        println!("（暂无进化数据，开始使用后将显示统计）");
    }

    // Skill combos from episodic logs
    let combos = skill_combos_from_logs(&config.logs_dir());
    if !combos.is_empty() {
        println!();
        println!("常用技能组合：");
        for (combo, count) in combos.iter().take(5) {
            let display = combo.split('+')
                .map(|s| format!("@{s}"))
                .collect::<Vec<_>>()
                .join(" + ");
            println!("  {}  ×{}", display, count);
        }
    }

    // Codeword usage
    let stats_path = config.stats_path();
    if let Ok(text) = std::fs::read_to_string(&stats_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(cw_obj) = v["codewords"].as_object() {
                let mut cw_stats: Vec<(String, u64)> = cw_obj
                    .iter()
                    .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
                    .collect();
                cw_stats.sort_by(|a, b| b.1.cmp(&a.1));
                if !cw_stats.is_empty() {
                    println!();
                    println!("🔑 暗号使用频率：");
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
                        println!("  「{}」×{}", label, count);
                    }
                }
            }
        }
    }

    println!();
    println!("━━━━━━━━━━━━━━");
    Ok(())
}

fn skill_combos_from_logs(logs_dir: &std::path::Path) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let entries = match std::fs::read_dir(logs_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("jsonl") { continue; }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
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
