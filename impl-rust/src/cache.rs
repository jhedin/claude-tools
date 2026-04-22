use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

fn open(path: &Path) -> Result<Connection> {
    config::ensure_parent(path)?;
    let conn = Connection::open(path).with_context(|| format!("opening cache {}", path.display()))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS rewrites (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );",
    )?;
    Ok(conn)
}

pub fn get(path: &Path, key: &str) -> Result<Option<String>> {
    let conn = open(path)?;
    let mut stmt = conn.prepare("SELECT value FROM rewrites WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        let v: String = row.get(0)?;
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

pub fn put(path: &Path, key: &str, value: &str) -> Result<()> {
    let conn = open(path)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO rewrites(key, value, created_at) VALUES(?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, created_at=excluded.created_at",
        params![key, value, now],
    )?;
    Ok(())
}

/// Subcommand: `cache get --key <k>`. Prints value on stdout; exit 1 on miss.
pub fn get_stdout(key: &str) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    if !cfg.cache.enabled {
        std::process::exit(1);
    }
    let path = config::cache_path(&cfg);
    match get(&path, key)? {
        Some(v) => {
            print!("{v}");
            Ok(())
        }
        None => std::process::exit(1),
    }
}

/// Subcommand: `cache put --key <k>`. Value JSON on stdin.
pub fn put_stdin(key: &str) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    if !cfg.cache.enabled {
        return Ok(());
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let path = config::cache_path(&cfg);
    put(&path, key, buf.trim_end())?;
    Ok(())
}
