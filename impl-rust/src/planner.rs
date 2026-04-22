use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::config;
use crate::history;

const SYSTEM_PROMPT: &str = "You translate natural-language shell wrappers \
(like cfind, cgrep, cjq, cawk, csed) into ONE real shell pipeline using \
standard Unix tools. The user gives you a command-line buffer they typed; \
your job is to rewrite it so it can run directly in a POSIX shell.

Rules:
- Output STRICT JSON with exactly two keys: \"command\" (the rewritten \
shell pipeline, one line, no leading `$`) and \"explanation\" (one short \
sentence, no trailing period required).
- The command MUST be a runnable pipeline of standard tools — find, grep, \
rg, jq, awk, sed (no -i), sort, uniq, cut, tr, wc, head, tail, xargs, \
curl, etc.
- Preserve any non-wrapped portion of the pipeline verbatim.
- Do not add explanations, markdown, or code fences. JSON only.";

#[derive(Debug, Serialize, Deserialize)]
pub struct PlanOutput {
    pub command: String,
    pub explanation: String,
}

pub fn run(pwd: &Path, timeout_override: Option<u64>) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    let buffer = buffer.trim_end_matches('\n').to_string();
    if buffer.is_empty() {
        return Err(anyhow!("empty buffer"));
    }
    let timeout = timeout_override.unwrap_or(cfg.planner.timeout_seconds);

    let user_message = format!(
        "Current working directory: {}\n\nBuffer:\n{}\n\nReturn JSON only.",
        pwd.display(),
        buffer
    );

    let raw = call_claude(&cfg.planner.model, SYSTEM_PROMPT, &user_message, timeout)
        .context("calling planner")?;
    let plan = parse_plan(&raw).context("parsing planner output")?;
    let out = serde_json::to_string(&plan)?;
    println!("{}", out);
    Ok(())
}

fn parse_plan(raw: &str) -> Result<PlanOutput> {
    let trimmed = strip_code_fence(raw.trim());
    let plan: PlanOutput = serde_json::from_str(trimmed)
        .with_context(|| format!("malformed planner JSON: {}", raw))?;
    if plan.command.trim().is_empty() {
        return Err(anyhow!("planner returned empty command"));
    }
    Ok(plan)
}

fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

/// Invoke `claude -p --output-format text --model <model>` with the given
/// system and user message. The user message is sent on stdin. Returns the
/// model's stdout with surrounding whitespace trimmed.
///
/// A watchdog thread kills the child if the timeout is exceeded.
pub fn call_claude(model: &str, system: &str, user: &str, timeout_secs: u64) -> Result<String> {
    let claude_bin = std::env::var("CLAUDE_TOOLS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let mut child = Command::new(&claude_bin)
        .args([
            "-p",
            "--model",
            model,
            "--output-format",
            "text",
            "--system-prompt",
            system,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning {}", claude_bin))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(user.as_bytes())?;
        // Closing stdin signals end-of-prompt.
    }

    let (tx, rx) = mpsc::channel();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    thread::spawn(move || {
        let mut out = String::new();
        let mut err = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = stderr.read_to_string(&mut err);
        let _ = tx.send((out, err));
    });

    // Wait with timeout.
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (out, err) = rx.recv().unwrap_or_default();
                if !status.success() {
                    history::log_error(&format!(
                        "claude -p failed: status={:?} stderr={}",
                        status.code(),
                        err.trim()
                    ));
                    return Err(anyhow!(
                        "claude -p exited with status {:?}: {}",
                        status.code(),
                        err.trim()
                    ));
                }
                return Ok(out);
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(timeout_secs) {
                    let _ = child.kill();
                    let _ = child.wait();
                    history::log_error(&format!(
                        "claude -p timed out after {}s",
                        timeout_secs
                    ));
                    return Err(anyhow!("claude -p timed out after {}s", timeout_secs));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
}
