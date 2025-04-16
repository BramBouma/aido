use clap::{Parser, ValueEnum};
use genai::chat::{ChatMessage, ChatRequest};
use genai::Client;
use serde::Deserialize;
use std::{env, fs, path::PathBuf, io::{self, Write}};
use std::process::{Command, exit};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;
use terminal_size::terminal_size;
use dirs::config_dir;

/// CLI argument definitions
#[derive(Parser)]
#[command(name = "aido", author, version, about = "AI‑powered one‑liner for your shell")]
struct Cli {
    /// The question/prompt to send
    #[arg(required = true)]
    prompt: Vec<String>,

    /// Which model to call (overrides config)
    #[arg(long)]
    model: Option<String>,

    /// Shell to generate commands for
    #[arg(long, value_enum, default_value_t = Shell::PowerShell)]
    shell: Shell,

    /// Print the generated command without executing it
    #[arg(long)]
    dry_run: bool,
}

/// Supported shells for execution
#[derive(Copy, Clone, ValueEnum)]
enum Shell {
    Bash,
    PowerShell,
}

/// Configuration loaded from file
#[derive(Deserialize)]
struct Config {
    models: std::collections::HashMap<String, String>,
    api_keys: std::collections::HashMap<String, String>,
    default_model: String,
    streaming: bool,
    system_prompt: String,
}

/// Returns path to config.json (XDG/AppData)
fn get_config_path() -> PathBuf {
    let mut dir = config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("aido");
    fs::create_dir_all(&dir).ok();
    dir.push("config.json");
    dir
}

/// Ensure a default config exists
fn ensure_config_exists() -> Result<(), Box<dyn std::error::Error>> {
    let path = get_config_path();
    if !path.exists() {
        let default = r#"{
  "models": { "gemini": "gemini-2.0-flash" },
  "api_keys": { "GEMINI_API_KEY": "" },
  "default_model": "gemini-2.0-flash",
  "streaming": true,
  "system_prompt": "Answer in one sentence"
}"#;
        fs::write(path, default)?;
    }
    Ok(())
}

/// Load config from disk
fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let data = fs::read_to_string(get_config_path())?;
    let cfg: Config = serde_json::from_str(&data)?;
    Ok(cfg)
}

/// Syntax-highlight code for display
fn print_highlighted_code(code: &str, ext: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ps.find_syntax_by_extension(ext).ok_or("Unknown syntax")?;
    let mut highlighter = HighlightLines::new(syntax, &ts.themes["base16-ocean.dark"]);
    let width = terminal_size().map(|(w, _)| w.0 as usize).unwrap_or(80);
    let max = width.saturating_sub(4);
    println!("╭{}╮", "─".repeat(max + 2));
    for line in code.lines() {
        let mut buf = String::new();
        let mut len = 0;
        for word in line.split_whitespace() {
            if len + word.len() + 1 <= max {
                if !buf.is_empty() { buf.push(' '); len += 1; }
                buf.push_str(word); len += word.len();
            } else {
                let ranges = highlighter.highlight_line(&buf, &ps)?;
                println!("│ {}{} │", as_24_bit_terminal_escaped(&ranges, false), " ".repeat(max - buf.len()));
                buf = word.to_string(); len = word.len();
            }
        }
        let ranges = highlighter.highlight_line(&buf, &ps)?;
        println!("│ {}{} │", as_24_bit_terminal_escaped(&ranges, false), " ".repeat(max - buf.len()));
    }
    println!("╰{}╯", "─".repeat(max + 2));
    print!("\x1b[0m");
    Ok(())
}

/// Strip out any ``` fences from the model’s output
fn clean_answer(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    // If starts with ```...<newline>, drop that first line
    if s.starts_with("```") {
        if let Some(pos) = s.find('\n') {
            s = s[pos+1..].to_string();
        }
    }
    // If ends with trailing fence, drop it
    if s.ends_with("```") {
        if let Some(pos) = s.rfind("```") {
            s = s[..pos].to_string();
        }
    }
    s.trim().to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1) ensure API key
    if env::var("GEMINI_API_KEY").ok().filter(|v| !v.is_empty()).is_none() {
        eprintln!("Error: GEMINI_API_KEY not set. Please set it in your environment.");
        exit(1);
    }

    // 2) parse args + load config
    let cli = Cli::parse();
    let prompt = cli.prompt.join(" ");
    ensure_config_exists()?;
    let cfg = load_config()?;
    let model = cli.model.unwrap_or(cfg.default_model);

    // 3) prepare LLM client and initial message history
    let client = Client::default();
    let mut messages = {
        let shell_name = match cli.shell { Shell::Bash => "bash", Shell::PowerShell => "PowerShell" };
        vec![
            ChatMessage::system(format!(
                "Give a {} one-liner to answer the question. The command will run on {} {}. Do not use a code block or backticks.",
                shell_name,
                env::consts::OS,
                env::consts::ARCH
            )),
            ChatMessage::user(prompt.clone()),
        ]
    };

    // 4) interactive preview → refine → accept loop
    loop {
        let chat_req = ChatRequest::new(messages.clone());
        let chat_res = client.exec_chat(&model, chat_req, None).await?;
        let raw = chat_res.content_text_as_str().unwrap_or("NO ANSWER");
        let answer = clean_answer(raw);

        // show highlighted preview
        let ext = if matches!(cli.shell, Shell::PowerShell) { "ps1" } else { "sh" };
        print_highlighted_code(&answer, ext)
            .unwrap_or_else(|_| println!("{answer}"));

        // prompt for refinement
        println!("Type to refine, Enter to accept, Ctrl+C to bail");
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        if input.trim().is_empty() {
            // accepted!
            if cli.dry_run {
                println!("{answer}");
                return Ok(());
            }

            // execute in the chosen shell
            let mut cmd = match cli.shell {
                Shell::Bash => {
                    let mut c = Command::new("bash");
                    c.arg("-c").arg(&answer);
                    c
                }
                Shell::PowerShell => {
                    let mut c = Command::new("powershell");
                    c.arg("-NoProfile").arg("-Command").arg(&answer);
                    c
                }
            };
            let status = cmd.spawn()?.wait()?;
            if !status.success() {
                eprintln!("Command failed with status: {}", status.code().unwrap_or(1));
            }
            return Ok(());
        }

        // otherwise, refine and loop again
        messages.push(ChatMessage::user(input.trim().to_string()));
    }
}
