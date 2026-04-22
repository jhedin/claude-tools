use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

use crate::config;
use crate::layer1::{self, Layer1Verdict};
use crate::planner;
use crate::tokenizer;

const SAFETY_SYSTEM_PROMPT: &str = "You review a shell command that was \
auto-generated from a natural-language request. Answer with strict JSON: \
{\"verdict\": \"safe\" | \"suspicious\" | \"dangerous\", \"reason\": \
\"<one short sentence>\"}. No markdown, no code fences.

Definitions:
- safe: command matches the user's request, no surprises, read-only or \
clearly intentional writes within the working directory.
- suspicious: does what was asked but has unusual side effects, writes to \
unexpected locations, or uses rarely-correct flags.
- dangerous: could destroy data, send network traffic to unexpected hosts, \
or does something materially different from what the user asked for.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer2Verdict {
    pub verdict: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckOutput {
    pub layer1: String,
    pub layer2: Option<String>,
    pub reason: String,
}

pub fn run_check(
    rewritten_file: &Path,
    pwd: &Path,
    timeout_override: Option<u64>,
    layer1_only: bool,
) -> Result<()> {
    let cfg = config::load().unwrap_or_default();

    let mut original = String::new();
    std::io::stdin().read_to_string(&mut original)?;
    let original = original.trim_end_matches('\n').to_string();

    let rewritten = std::fs::read_to_string(rewritten_file)
        .with_context(|| format!("reading {}", rewritten_file.display()))?;
    let rewritten = rewritten.trim_end_matches('\n').to_string();

    let parsed = tokenizer::parse(&rewritten);
    let l1 = layer1::classify(&parsed, pwd);

    let output = if layer1_only || l1 == Layer1Verdict::FastSafe {
        CheckOutput {
            layer1: l1.as_str().to_string(),
            layer2: None,
            reason: String::new(),
        }
    } else {
        let timeout = timeout_override.unwrap_or(cfg.safety.timeout_seconds);
        let verdict = call_layer2(&cfg.safety.model, &original, &rewritten, pwd, timeout)?;
        CheckOutput {
            layer1: l1.as_str().to_string(),
            layer2: Some(verdict.verdict),
            reason: verdict.reason,
        }
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn call_layer2(
    model: &str,
    original: &str,
    rewritten: &str,
    pwd: &Path,
    timeout: u64,
) -> Result<Layer2Verdict> {
    let user = format!(
        "User's original buffer (including c-prefixed wrappers):\n{}\n\n\
Rewritten command the planner produced:\n{}\n\n\
Current working directory: {}\n\n\
Return JSON only.",
        original,
        rewritten,
        pwd.display()
    );
    let raw = planner::call_claude(model, SAFETY_SYSTEM_PROMPT, &user, timeout)
        .context("calling safety checker")?;
    parse_layer2(&raw)
}

fn parse_layer2(raw: &str) -> Result<Layer2Verdict> {
    let trimmed = strip_code_fence(raw.trim());
    let v: Layer2Verdict = serde_json::from_str(trimmed)
        .with_context(|| format!("malformed safety JSON: {}", raw))?;
    match v.verdict.as_str() {
        "safe" | "suspicious" | "dangerous" => Ok(v),
        other => Err(anyhow!("unknown safety verdict: {}", other)),
    }
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
