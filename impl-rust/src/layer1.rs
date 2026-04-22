use crate::tokenizer::{ParsedPipeline, Stage};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer1Verdict {
    FastSafe,
    NeedsSmartRead,
    Reject,
}

impl Layer1Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer1Verdict::FastSafe => "fast-safe",
            Layer1Verdict::NeedsSmartRead => "needs-smart-read",
            Layer1Verdict::Reject => "reject",
        }
    }
}

/// Tools that are safe by default with any flags, per spec §Safety/Layer 1.
const ALLOWED_READ_TOOLS: &[&str] = &[
    "find", "grep", "rg", "ag", "jq", "yq", "awk", "gawk", "sort", "uniq", "cut", "tr", "wc",
    "head", "tail", "ls", "cat", "echo", "printf", "column", "paste", "comm", "diff", "fold",
    "rev", "tac", "seq", "nl", "od", "xxd", "file", "stat", "readlink", "realpath", "basename",
    "dirname", "date", "env", "printenv", "which", "type", "command",
];

/// Tools that write but are allowed by Layer 1 (Layer 2 may still flag).
const WRITE_ISH_ALLOWED: &[&str] = &[
    "curl", "wget", "mkdir", "touch", "docker", "npm", "cargo", "make",
];

/// Git subcommands considered read-only.
const GIT_READ_SUBS: &[&str] = &[
    "log",
    "diff",
    "status",
    "show",
    "blame",
    "ls-files",
    "rev-parse",
];

/// Git subcommands that write state but Layer 1 permits (Layer 2 may flag).
const GIT_WRITE_ALLOWED_SUBS: &[&str] =
    &["commit", "add", "stash", "tag", "branch", "fetch", "pull"];

/// Git subcommands that always reject.
const GIT_REJECT_SUBS: &[&str] = &[
    "checkout",
    "restore",
    "reset",
    "clean",
    "push",
    "rebase",
    "cherry-pick",
    "revert",
    "merge",
];

/// Docker subcommands that are always rejected at Layer 1.
const DOCKER_REJECT_SUBS: &[&str] = &["rm", "rmi", "system", "run", "exec"];

/// kubectl subcommands that are always rejected.
const KUBECTL_REJECT_SUBS: &[&str] = &["apply", "delete", "exec", "patch"];

/// Package managers whose state-changing invocations are rejected.
const PKG_MANAGERS: &[&str] = &["pip", "pip3", "apt", "apt-get", "brew", "yum", "dnf", "gem"];

/// Shell builtins / dangerous constructs that always reject.
const REJECT_PROGRAMS: &[&str] = &[
    "rm", "rmdir", "shred", "dd", "unlink", "chmod", "chown", "chgrp", "setfacl", "sudo", "doas",
    "su", "eval", "exec", "source", ".",
];

/// Programs that always force a Layer 2 read.
const FORCE_SMART_READ_PROGRAMS: &[&str] = &["xargs", "tee"];

/// Classify a parsed pipeline. `pwd` is used only for redirect-target
/// heuristics (writes inside $PWD or /tmp are tolerated, others force
/// Layer 2).
pub fn classify(p: &ParsedPipeline, pwd: &Path) -> Layer1Verdict {
    if p.unparseable {
        return Layer1Verdict::NeedsSmartRead;
    }
    if p.stages.is_empty() {
        return Layer1Verdict::NeedsSmartRead;
    }
    if p.has_command_substitution || p.has_backticks {
        return Layer1Verdict::NeedsSmartRead;
    }
    if p.has_compound() {
        // `&&`, `;`, `||` at top level always force Layer 2.
        return worst(
            Layer1Verdict::NeedsSmartRead,
            classify_stages(&p.stages, pwd),
        );
    }
    classify_stages(&p.stages, pwd)
}

fn classify_stages(stages: &[Stage], pwd: &Path) -> Layer1Verdict {
    let mut worst_v = Layer1Verdict::FastSafe;
    for s in stages {
        let v = classify_stage(s, pwd);
        worst_v = worst(worst_v, v);
    }
    worst_v
}

fn worst(a: Layer1Verdict, b: Layer1Verdict) -> Layer1Verdict {
    use Layer1Verdict::*;
    match (a, b) {
        (Reject, _) | (_, Reject) => Reject,
        (NeedsSmartRead, _) | (_, NeedsSmartRead) => NeedsSmartRead,
        _ => FastSafe,
    }
}

fn classify_stage(s: &Stage, pwd: &Path) -> Layer1Verdict {
    let prog = s.program.as_str();

    // Hard rejects on program name.
    if REJECT_PROGRAMS.contains(&prog) {
        return Layer1Verdict::Reject;
    }
    if FORCE_SMART_READ_PROGRAMS.contains(&prog) {
        return Layer1Verdict::NeedsSmartRead;
    }

    // Special-cased programs with flag-dependent behavior.
    if prog == "sed" || prog == "perl" {
        if s.has_flag("-i") || s.args.iter().any(|a| a.starts_with("-i")) {
            return Layer1Verdict::Reject;
        }
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }
    if prog == "find" {
        if s.has_flag("-delete") {
            return Layer1Verdict::Reject;
        }
        if s.has_flag("-exec") || s.has_flag("-execdir") {
            // -exec with rm → hard reject; anything else needs smart read.
            let joined = s.args.join(" ");
            if joined.contains(" rm ") || joined.contains(" rm\t") || joined.ends_with(" rm") {
                return Layer1Verdict::Reject;
            }
            return Layer1Verdict::NeedsSmartRead;
        }
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }
    if prog == "mv" {
        // Conservative: treat all mv as reject (would overwrite existing dest).
        return Layer1Verdict::Reject;
    }
    if prog == "truncate" {
        // truncate -s 0 is a reject.
        if s.args.iter().any(|a| a == "-s") {
            return Layer1Verdict::Reject;
        }
        return Layer1Verdict::NeedsSmartRead;
    }
    if prog == "git" {
        return git_verdict(s);
    }
    if prog == "docker" {
        if let Some(sub) = s.args.first() {
            if DOCKER_REJECT_SUBS.contains(&sub.as_str()) {
                return Layer1Verdict::Reject;
            }
        }
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }
    if prog == "kubectl" {
        if let Some(sub) = s.args.first() {
            if KUBECTL_REJECT_SUBS.contains(&sub.as_str()) {
                return Layer1Verdict::Reject;
            }
        }
        return Layer1Verdict::NeedsSmartRead;
    }
    if PKG_MANAGERS.contains(&prog) {
        // npm and cargo are special-cased below.
        return pkg_verdict(prog, s);
    }
    if prog == "npm" {
        if let Some(sub) = s.args.first() {
            match sub.as_str() {
                "install" | "i" | "uninstall" | "rm" | "update" => return Layer1Verdict::Reject,
                _ => return Layer1Verdict::FastSafe,
            }
        }
        return Layer1Verdict::FastSafe;
    }
    if prog == "cargo" {
        if let Some(sub) = s.args.first() {
            if sub == "install" || sub == "uninstall" {
                return Layer1Verdict::Reject;
            }
        }
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }

    // `make <target>` — conservative: if any arg looks like a clean target, reject.
    if prog == "make" {
        if s.args.iter().any(|a| a == "clean" || a == "distclean") {
            return Layer1Verdict::Reject;
        }
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }

    if prog == "curl" || prog == "wget" {
        // curl -o / wget -O to anywhere always forces Layer 2.
        let writes_to_disk = s.args.iter().any(|a| a == "-o" || a == "-O");
        if writes_to_disk {
            return Layer1Verdict::NeedsSmartRead;
        }
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }

    if WRITE_ISH_ALLOWED.contains(&prog) {
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }
    if ALLOWED_READ_TOOLS.contains(&prog) {
        return check_redirects(s, pwd, Layer1Verdict::FastSafe);
    }

    // Unknown program → force Layer 2.
    Layer1Verdict::NeedsSmartRead
}

fn git_verdict(s: &Stage) -> Layer1Verdict {
    let sub = match s.args.iter().find(|a| !a.starts_with('-')) {
        Some(a) => a.as_str(),
        None => return Layer1Verdict::NeedsSmartRead,
    };
    if GIT_REJECT_SUBS.contains(&sub) {
        return Layer1Verdict::Reject;
    }
    if sub == "stash" {
        // git stash drop / git stash clear → reject.
        if s.args.iter().any(|a| a == "drop" || a == "clear") {
            return Layer1Verdict::Reject;
        }
    }
    if sub == "branch" && s.args.iter().any(|a| a == "-D" || a == "--delete") {
        return Layer1Verdict::Reject;
    }
    if sub == "push" && s.args.iter().any(|a| a == "--force" || a == "-f") {
        return Layer1Verdict::Reject;
    }
    if GIT_READ_SUBS.contains(&sub) {
        return Layer1Verdict::FastSafe;
    }
    if GIT_WRITE_ALLOWED_SUBS.contains(&sub) {
        return Layer1Verdict::FastSafe;
    }
    Layer1Verdict::NeedsSmartRead
}

fn pkg_verdict(prog: &str, s: &Stage) -> Layer1Verdict {
    let sub = s.args.first().map(String::as_str).unwrap_or("");
    // pip / pip3: install/uninstall reject.
    if prog == "pip" || prog == "pip3" {
        if matches!(sub, "install" | "uninstall") {
            return Layer1Verdict::Reject;
        }
        return Layer1Verdict::FastSafe;
    }
    // apt / apt-get / yum / dnf / brew: install/remove/upgrade reject.
    if matches!(
        sub,
        "install" | "remove" | "upgrade" | "update" | "autoremove" | "purge"
    ) {
        return Layer1Verdict::Reject;
    }
    Layer1Verdict::NeedsSmartRead
}

fn check_redirects(s: &Stage, pwd: &Path, base: Layer1Verdict) -> Layer1Verdict {
    let mut v = base;
    for r in &s.redirects {
        if matches!(r.op.as_str(), ">" | ">>" | "&>" | "&>>" | "2>" | "2>>") {
            if !target_is_safe(&r.target, pwd) {
                v = worst(v, Layer1Verdict::NeedsSmartRead);
            }
        }
    }
    // tee target check — tee is FORCE_SMART_READ so handled above, but if
    // someone added a tee to WRITE_ISH_ALLOWED later, this is defensive.
    v
}

fn target_is_safe(target: &str, pwd: &Path) -> bool {
    if target.starts_with("/tmp/") || target == "/tmp" {
        return true;
    }
    if target.starts_with("/dev/null") || target == "/dev/null" {
        return true;
    }
    if target.starts_with('/') {
        // Absolute path outside /tmp.
        let pwd_str = pwd.to_string_lossy();
        return target.starts_with(pwd_str.as_ref());
    }
    // Relative path → inside PWD by construction.
    true
}
