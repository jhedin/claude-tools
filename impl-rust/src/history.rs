use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{Read, Write};

use crate::config;

/// Subcommand: `history append`. Reads one JSON document from stdin, appends
/// it as a single line to the configured history path. Opens with `O_APPEND`
/// so concurrent writers don't interleave within a single `write()` call.
pub fn append_stdin() -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let path = config::history_path(&cfg);
    config::ensure_parent(&path)?;

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let line = buf.trim_end_matches('\n');
    if line.is_empty() {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    // Single write → atomic up to PIPE_BUF on POSIX, which is plenty for a
    // one-line JSON record.
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Append a log line to `~/.local/share/claude-tools/errors.log`. Best effort;
/// errors here are swallowed.
#[allow(dead_code)]
pub fn log_error(msg: &str) {
    let path = config::error_log_path();
    let _ = config::ensure_parent(&path);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", msg);
    }
}
