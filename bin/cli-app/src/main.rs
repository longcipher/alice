//! Alice CLI binary.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use clap::Parser;
use cli_app::{bootstrap, config, memory_context};

/// Alice - minimal hexagonal AI agent CLI.
#[derive(Debug, Parser)]
#[command(name = "alice", version, about = "Alice CLI agent")]
struct Cli {
    /// Path to configuration file.
    #[arg(short, long, default_value = "alice.toml")]
    config: String,

    /// Run one prompt and exit.
    #[arg(long)]
    once: Option<String>,
}

async fn run_repl(context: &bootstrap::AliceRuntimeContext) -> eyre::Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let session_id = "alice-session";

    eprintln!("Alice ready (model: {})", context.default_model);
    eprintln!("Type /quit to exit.\n");

    loop {
        eprint!("> ");
        let flush_result = tokio::io::AsyncWriteExt::flush(&mut tokio::io::stderr()).await;
        if flush_result.is_err() {
            break;
        }

        let line = lines.next_line().await?;
        let Some(line) = line else {
            break;
        };

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("/quit") || input.eq_ignore_ascii_case("/exit") {
            break;
        }

        match memory_context::run_turn_with_memory(context, session_id, input).await {
            Ok(response) => println!("{}", response.content),
            Err(error) => eprintln!("error: {error}"),
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let cli = Cli::parse();
    let config = config::load_config(&cli.config)?;
    let context = bootstrap::build_runtime(&config).await?;

    if let Some(input) = cli.once.as_deref() {
        let response = memory_context::run_turn_with_memory(&context, "alice-once", input).await?;
        println!("{}", response.content);
        return Ok(());
    }

    run_repl(&context).await
}
