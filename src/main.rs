use genai::chat::{ChatMessage, ChatRequest};
use genai::Client;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;
use terminal_size::terminal_size;

fn ensure_config_exists() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").expect("Could not find HOME directory");
    let config_path = PathBuf::from(home).join(".aido");

    if !config_path.exists() {
        let default_config = r#"{
            "models": {
                "openai": "gpt-4o-mini",
                "anthropic": "claude-3-haiku-20240307", 
                "cohere": "command-light",
                "gemini": "gemini-1.5-flash-latest",
                "groq": "llama3-8b-8192",
                "ollama": "gemma:2b",
                "xai": "grok-beta",
                "deepseek": "deepseek-chat"
            },
            "api_keys": {
                "OPENAI_API_KEY": "",
                "ANTHROPIC_API_KEY": "",
                "COHERE_API_KEY": "",
                "GEMINI_API_KEY": "",
                "GROQ_API_KEY": "",
                "XAI_API_KEY": "",
                "DEEPSEEK_API_KEY": ""
            },
            "default_model": "gpt-4o-mini",
            "streaming": true,
            "system_prompt": "Answer in one sentence"
        }"#;
        fs::write(config_path, default_config)?;
    }
    Ok(())
}

fn print_highlighted_code(code: &str, language: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let syntax = ps
        .find_syntax_by_extension(language)
        .ok_or("Language syntax not found")?;

    let mut highlighter = HighlightLines::new(syntax, &ts.themes["base16-ocean.dark"]);

    // Get terminal width, default to 80 if can't determine
    let term_width = match terminal_size() {
        None => 80,
        Some(size) => size.0 .0 as usize,
    };
    // Max content width is terminal width minus borders and padding (4 chars: '│ ' and ' │')
    let max_content_width = term_width.saturating_sub(4);

    // Print top border
    println!("╭{}╮", "─".repeat(max_content_width + 2));

    for line in code.lines() {
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut current_length = 0;

        // Split line into chunks that fit within max_content_width
        for word in line.split_whitespace() {
            if current_length + word.len() + 1 <= max_content_width {
                if !current_chunk.is_empty() {
                    current_chunk.push(' ');
                    current_length += 1;
                }
                current_chunk.push_str(word);
                current_length += word.len();
            } else {
                if !current_chunk.is_empty() {
                    chunks.push(current_chunk);
                }
                current_chunk = word.to_string();
                current_length = word.len();
            }
        }
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        // If line is empty, add an empty chunk
        if chunks.is_empty() {
            chunks.push(String::new());
        }

        // Print each chunk with highlighting
        for chunk in chunks {
            print!("│ ");

            let ranges = highlighter.highlight_line(&chunk, &ps)?;
            print!("{}", as_24_bit_terminal_escaped(&ranges[..], false));

            // Pad to max_content_width
            let visible_length = chunk.chars().count();
            if visible_length < max_content_width {
                print!("{}", " ".repeat(max_content_width - visible_length));
            }
            println!("\x1b[0m │");
        }
    }

    // Print bottom border
    println!("╰{}╯", "─".repeat(max_content_width + 2));
    print!("\x1b[0m"); // Reset terminal colors
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ensure_config_exists()?;

    let args: Vec<String> = std::env::args().collect();
    let question = args[1..].join(" ");
    if question.is_empty() {
        eprintln!("Error: Please provide a question as command line arguments");
        std::process::exit(1);
    }
    let mut messages = vec![
        ChatMessage::system(
            format!(
                "Give a bash one-liner to answer the question. The command will run on {} {}. Do not use a code block or leading/trailing backticks.",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        ),
        ChatMessage::user(question.clone()),
    ];

    loop {
        let chat_req = ChatRequest::new(messages.clone());
        let client = Client::default();
        let model = "gpt-4o";
        let chat_res = client.exec_chat(model, chat_req.clone(), None).await?;
        let answer = chat_res.content_text_as_str().unwrap_or("NO ANSWER");
        let _ = print_highlighted_code(&answer, "bash");

        println!("Type to refine, Enter to accept, Ctrl+C to bail");
        print!("> ");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        if input.trim().is_empty() {
            // Run the answer as a bash command in the current shell
            let status = std::process::Command::new("bash")
                .arg("-c")
                .arg(answer)
                .spawn()?
                .wait()?;
            if status.success() {
                println!("");
            } else {
                println!("FAILED with status: {}", status.code().unwrap_or(1));
            }
            break;
        }
        messages.push(ChatMessage::user(input.trim().to_string()));
    }
    Ok(())
}
