use anyhow::Result;
use clap::{Parser, Subcommand};
use claude_tools::{cache, config, history, layer2, planner};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "claude-tools", version, about = "zsh wrapper engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Rewrite buffer (read from stdin) into a real shell pipeline.
    Plan {
        #[arg(long)]
        pwd: PathBuf,
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Safety-check a rewritten command. Original buffer on stdin.
    Check {
        #[arg(long)]
        rewritten_file: PathBuf,
        #[arg(long)]
        pwd: PathBuf,
        #[arg(long)]
        timeout: Option<u64>,
        /// Only run Layer 1; do not call the LLM. For manual eyeballing.
        #[arg(long, hide = true)]
        layer1_only: bool,
    },
    /// Cache operations.
    Cache {
        #[command(subcommand)]
        op: CacheOp,
    },
    /// History operations.
    History {
        #[command(subcommand)]
        op: HistoryOp,
    },
    /// Config introspection (used by the zsh widget).
    Config {
        #[command(subcommand)]
        op: ConfigOp,
    },
}

#[derive(Subcommand)]
enum ConfigOp {
    /// Print configured alias tool names, one per line.
    Aliases,
}

#[derive(Subcommand)]
enum CacheOp {
    /// Get cached JSON value by key. Exit 1 on miss.
    Get {
        #[arg(long)]
        key: String,
    },
    /// Put value (JSON on stdin) by key.
    Put {
        #[arg(long)]
        key: String,
    },
}

#[derive(Subcommand)]
enum HistoryOp {
    /// Append a JSON line (from stdin) to the sidecar log.
    Append,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Plan { pwd, timeout } => planner::run(&pwd, timeout),
        Cmd::Check {
            rewritten_file,
            pwd,
            timeout,
            layer1_only,
        } => layer2::run_check(&rewritten_file, &pwd, timeout, layer1_only),
        Cmd::Cache { op } => match op {
            CacheOp::Get { key } => cache::get_stdout(&key),
            CacheOp::Put { key } => cache::put_stdin(&key),
        },
        Cmd::History { op } => match op {
            HistoryOp::Append => history::append_stdin(),
        },
        Cmd::Config { op } => match op {
            ConfigOp::Aliases => {
                let cfg = config::load().unwrap_or_default();
                for t in &cfg.aliases.tools {
                    println!("{}", t);
                }
                Ok(())
            }
        },
    }
}
