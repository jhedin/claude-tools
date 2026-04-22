//! Integration tests that spawn the compiled binary and exercise the CLI
//! contract the zsh widget depends on.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> PathBuf {
    // Cargo sets CARGO_BIN_EXE_<name> for integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_claude-tools"))
}

/// Point the binary at a scratch directory for config, cache, and history so
/// tests don't touch the real user dirs.
fn scratch_env(dir: &std::path::Path) -> Vec<(String, String)> {
    let cfg = dir.join("config.toml");
    std::fs::write(
        &cfg,
        format!(
            r#"[cache]
enabled = true
path = "{cache}"

[history]
path = "{hist}"
"#,
            cache = dir.join("cache.db").display(),
            hist = dir.join("history.jsonl").display()
        ),
    )
    .unwrap();
    vec![
        ("CLAUDE_TOOLS_CONFIG".into(), cfg.display().to_string()),
        ("HOME".into(), dir.display().to_string()),
    ]
}

fn run(args: &[&str], env: &[(String, String)], stdin: &str) -> (std::process::ExitStatus, String, String) {
    let mut cmd = Command::new(bin());
    cmd.args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn");
    if let Some(mut s) = child.stdin.take() {
        s.write_all(stdin.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().expect("wait");
    (
        out.status,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn cache_roundtrip() {
    let tmp = tempdir();
    let env = scratch_env(tmp.path());

    let (st, _, err) = run(&["cache", "put", "--key", "abc"], &env, r#"{"command":"ls","explanation":"list"}"#);
    assert!(st.success(), "put failed: {}", err);

    let (st, out, _) = run(&["cache", "get", "--key", "abc"], &env, "");
    assert!(st.success());
    assert_eq!(out.trim(), r#"{"command":"ls","explanation":"list"}"#);

    // Missing key → exit 1.
    let (st, _, _) = run(&["cache", "get", "--key", "does-not-exist"], &env, "");
    assert!(!st.success());
}

#[test]
fn history_append_writes_jsonl_lines() {
    let tmp = tempdir();
    let env = scratch_env(tmp.path());

    for n in 0..5 {
        let payload = format!(r#"{{"n":{}}}"#, n);
        let (st, _, err) = run(&["history", "append"], &env, &payload);
        assert!(st.success(), "append failed: {}", err);
    }

    let body = std::fs::read_to_string(tmp.path().join("history.jsonl")).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], r#"{"n":0}"#);
    assert_eq!(lines[4], r#"{"n":4}"#);
}

#[test]
fn history_append_concurrent_writers_do_not_interleave() {
    let tmp = tempdir();
    let env = scratch_env(tmp.path());

    let mut handles = Vec::new();
    for n in 0..8 {
        let env = env.clone();
        let h = std::thread::spawn(move || {
            let payload = format!(r#"{{"n":{}}}"#, n);
            let (st, _, err) = run(&["history", "append"], &env, &payload);
            assert!(st.success(), "append {} failed: {}", n, err);
        });
        handles.push(h);
    }
    for h in handles {
        h.join().unwrap();
    }
    let body = std::fs::read_to_string(tmp.path().join("history.jsonl")).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 8);
    for l in &lines {
        // Each line parses as JSON — proves no interleaving.
        serde_json::from_str::<serde_json::Value>(l).expect(&format!("bad line: {}", l));
    }
}

#[test]
fn layer1_only_check_is_purely_local() {
    let tmp = tempdir();
    let env = scratch_env(tmp.path());

    let rewritten = tmp.path().join("r.sh");
    std::fs::write(&rewritten, "find . -name '*.py' | grep TODO | sort\n").unwrap();

    let (st, out, err) = run(
        &[
            "check",
            "--layer1-only",
            "--rewritten-file",
            rewritten.to_str().unwrap(),
            "--pwd",
            tmp.path().to_str().unwrap(),
        ],
        &env,
        "cfind . \"python files\" | cgrep TODO | sort\n",
    );
    assert!(st.success(), "check failed: {}", err);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["layer1"], "fast-safe");
    assert!(v["layer2"].is_null());
}

#[test]
fn layer1_only_reject_case() {
    let tmp = tempdir();
    let env = scratch_env(tmp.path());

    let rewritten = tmp.path().join("r.sh");
    std::fs::write(&rewritten, "rm -rf /tmp/foo\n").unwrap();

    let (st, out, _) = run(
        &[
            "check",
            "--layer1-only",
            "--rewritten-file",
            rewritten.to_str().unwrap(),
            "--pwd",
            tmp.path().to_str().unwrap(),
        ],
        &env,
        "cfind . \"scratch\"\n",
    );
    assert!(st.success());
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["layer1"], "reject");
}

// ---------- planner / layer2 via shim ----------

/// Write a shell script that behaves like `claude -p`: reads all of stdin (the
/// prompt), then prints the provided JSON body on stdout.
fn write_claude_shim(dir: &std::path::Path, body: &str) -> PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat > /dev/null\ncat <<'EOF'\n{}\nEOF\n",
            body
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(&script).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&script, perm).unwrap();
    script
}

fn write_failing_shim(dir: &std::path::Path, code: i32) -> PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat > /dev/null\necho 'boom' 1>&2\nexit {}\n",
            code
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(&script).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&script, perm).unwrap();
    script
}

fn write_slow_shim(dir: &std::path::Path, secs: u64) -> PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat > /dev/null\nsleep {}\necho '{{\"command\":\"true\",\"explanation\":\"x\"}}'\n",
            secs
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(&script).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&script, perm).unwrap();
    script
}

#[test]
fn plan_uses_shim_and_emits_contract_json() {
    let tmp = tempdir();
    let mut env = scratch_env(tmp.path());
    let shim = write_claude_shim(
        tmp.path(),
        r#"{"command":"find . -name '*.py'","explanation":"find python"}"#,
    );
    env.push(("CLAUDE_TOOLS_CLAUDE_BIN".into(), shim.display().to_string()));

    let (st, out, err) = run(
        &["plan", "--pwd", tmp.path().to_str().unwrap()],
        &env,
        "cfind . \"python files\"\n",
    );
    assert!(st.success(), "plan failed: {}", err);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["command"], "find . -name '*.py'");
    assert_eq!(v["explanation"], "find python");
}

#[test]
fn plan_handles_code_fenced_output() {
    let tmp = tempdir();
    let mut env = scratch_env(tmp.path());
    let shim = write_claude_shim(
        tmp.path(),
        "```json\n{\"command\":\"ls\",\"explanation\":\"list\"}\n```",
    );
    env.push(("CLAUDE_TOOLS_CLAUDE_BIN".into(), shim.display().to_string()));

    let (st, out, err) = run(
        &["plan", "--pwd", tmp.path().to_str().unwrap()],
        &env,
        "clist .\n",
    );
    assert!(st.success(), "plan failed: {}", err);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn plan_fails_on_malformed_json() {
    let tmp = tempdir();
    let mut env = scratch_env(tmp.path());
    let shim = write_claude_shim(tmp.path(), "not json at all");
    env.push(("CLAUDE_TOOLS_CLAUDE_BIN".into(), shim.display().to_string()));

    let (st, _, _) = run(
        &["plan", "--pwd", tmp.path().to_str().unwrap()],
        &env,
        "cfoo\n",
    );
    assert!(!st.success(), "expected failure on malformed JSON");
}

#[test]
fn plan_fails_on_non_zero_exit() {
    let tmp = tempdir();
    let mut env = scratch_env(tmp.path());
    let shim = write_failing_shim(tmp.path(), 1);
    env.push(("CLAUDE_TOOLS_CLAUDE_BIN".into(), shim.display().to_string()));

    let (st, _, _) = run(
        &["plan", "--pwd", tmp.path().to_str().unwrap()],
        &env,
        "cfoo\n",
    );
    assert!(!st.success());
}

#[test]
fn plan_times_out() {
    let tmp = tempdir();
    let mut env = scratch_env(tmp.path());
    let shim = write_slow_shim(tmp.path(), 10);
    env.push(("CLAUDE_TOOLS_CLAUDE_BIN".into(), shim.display().to_string()));

    let (st, _, _) = run(
        &[
            "plan",
            "--pwd",
            tmp.path().to_str().unwrap(),
            "--timeout",
            "1",
        ],
        &env,
        "cfoo\n",
    );
    assert!(!st.success(), "expected timeout failure");
}

#[test]
fn check_invokes_layer2_when_needed() {
    let tmp = tempdir();
    let mut env = scratch_env(tmp.path());
    let shim = write_claude_shim(
        tmp.path(),
        r#"{"verdict":"suspicious","reason":"writes outside pwd"}"#,
    );
    env.push(("CLAUDE_TOOLS_CLAUDE_BIN".into(), shim.display().to_string()));

    let rewritten = tmp.path().join("r.sh");
    std::fs::write(&rewritten, "curl -o /tmp/x https://example.com\n").unwrap();

    let (st, out, err) = run(
        &[
            "check",
            "--rewritten-file",
            rewritten.to_str().unwrap(),
            "--pwd",
            tmp.path().to_str().unwrap(),
        ],
        &env,
        "cdownload \"example\"\n",
    );
    assert!(st.success(), "check failed: {}", err);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["layer1"], "needs-smart-read");
    assert_eq!(v["layer2"], "suspicious");
    assert_eq!(v["reason"], "writes outside pwd");
}

#[test]
fn check_skips_layer2_for_fast_safe() {
    let tmp = tempdir();
    let env = scratch_env(tmp.path());

    let rewritten = tmp.path().join("r.sh");
    std::fs::write(&rewritten, "find . -name '*.py' | grep TODO\n").unwrap();

    let (st, out, err) = run(
        &[
            "check",
            "--rewritten-file",
            rewritten.to_str().unwrap(),
            "--pwd",
            tmp.path().to_str().unwrap(),
        ],
        &env,
        "cfind . \"py\" | cgrep TODO\n",
    );
    assert!(st.success(), "check failed: {}", err);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["layer1"], "fast-safe");
    assert!(v["layer2"].is_null());
}

// ---------- helpers ----------

struct TempDir(PathBuf);
impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> TempDir {
    let base = std::env::temp_dir();
    let name = format!(
        "claude-tools-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let p = base.join(name);
    std::fs::create_dir_all(&p).unwrap();
    TempDir(p)
}
