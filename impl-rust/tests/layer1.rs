use claude_tools::layer1::{self, Layer1Verdict};
use claude_tools::tokenizer;
use std::path::PathBuf;

fn classify(cmd: &str) -> Layer1Verdict {
    classify_in(cmd, "/home/testuser/project")
}

fn classify_in(cmd: &str, pwd: &str) -> Layer1Verdict {
    let p = tokenizer::parse(cmd);
    layer1::classify(&p, &PathBuf::from(pwd))
}

// ---------- fast-safe cases ----------

#[test]
fn plain_find_is_fast_safe() {
    assert_eq!(
        classify("find . -name '*.py' -mtime -7"),
        Layer1Verdict::FastSafe
    );
}

#[test]
fn grep_pipeline_fast_safe() {
    assert_eq!(
        classify("find . -name '*.py' | grep -l TODO | sort | uniq -c"),
        Layer1Verdict::FastSafe
    );
}

#[test]
fn jq_is_fast_safe() {
    assert_eq!(classify("jq '.items[]' data.json"), Layer1Verdict::FastSafe);
}

#[test]
fn awk_is_fast_safe() {
    assert_eq!(classify("awk '{print $1}' file"), Layer1Verdict::FastSafe);
}

#[test]
fn sed_without_inplace_is_fast_safe() {
    assert_eq!(
        classify("sed 's/foo/bar/' file"),
        Layer1Verdict::FastSafe
    );
}

#[test]
fn git_log_is_fast_safe() {
    assert_eq!(classify("git log --oneline -20"), Layer1Verdict::FastSafe);
}

#[test]
fn git_diff_is_fast_safe() {
    assert_eq!(classify("git diff HEAD~1"), Layer1Verdict::FastSafe);
}

#[test]
fn redirect_to_tmp_is_fast_safe() {
    assert_eq!(classify("echo hi > /tmp/out"), Layer1Verdict::FastSafe);
}

#[test]
fn redirect_relative_in_pwd_is_fast_safe() {
    assert_eq!(classify("echo hi > out.log"), Layer1Verdict::FastSafe);
}

// ---------- needs-smart-read ----------

#[test]
fn xargs_always_needs_smart_read() {
    assert_eq!(
        classify("find . -name '*.py' | xargs grep TODO"),
        Layer1Verdict::NeedsSmartRead
    );
}

#[test]
fn tee_always_needs_smart_read() {
    assert_eq!(classify("echo hi | tee out"), Layer1Verdict::NeedsSmartRead);
}

#[test]
fn compound_and_needs_smart_read() {
    assert_eq!(classify("ls && pwd"), Layer1Verdict::NeedsSmartRead);
}

#[test]
fn compound_semicolon_needs_smart_read() {
    assert_eq!(classify("ls ; pwd"), Layer1Verdict::NeedsSmartRead);
}

#[test]
fn command_substitution_needs_smart_read() {
    assert_eq!(classify("echo $(date)"), Layer1Verdict::NeedsSmartRead);
}

#[test]
fn backticks_need_smart_read() {
    assert_eq!(classify("echo `date`"), Layer1Verdict::NeedsSmartRead);
}

#[test]
fn unknown_tool_needs_smart_read() {
    assert_eq!(
        classify("unknowntool --foo bar"),
        Layer1Verdict::NeedsSmartRead
    );
}

#[test]
fn redirect_outside_pwd_needs_smart_read() {
    assert_eq!(
        classify_in("echo hi > /etc/motd", "/home/u/p"),
        Layer1Verdict::NeedsSmartRead
    );
}

#[test]
fn curl_output_flag_needs_smart_read() {
    assert_eq!(
        classify("curl -o /tmp/foo https://example.com"),
        Layer1Verdict::NeedsSmartRead
    );
}

#[test]
fn find_exec_not_rm_needs_smart_read() {
    assert_eq!(
        classify("find . -name '*.log' -exec cat {} \\;"),
        Layer1Verdict::NeedsSmartRead
    );
}

// ---------- reject ----------

#[test]
fn rm_rejects() {
    assert_eq!(classify("rm -rf foo"), Layer1Verdict::Reject);
}

#[test]
fn sed_inplace_rejects() {
    assert_eq!(
        classify("sed -i 's/foo/bar/' file"),
        Layer1Verdict::Reject
    );
}

#[test]
fn perl_inplace_rejects() {
    assert_eq!(
        classify("perl -i -pe 's/foo/bar/' file"),
        Layer1Verdict::Reject
    );
}

#[test]
fn find_delete_rejects() {
    assert_eq!(classify("find . -delete"), Layer1Verdict::Reject);
}

#[test]
fn find_exec_rm_rejects() {
    assert_eq!(
        classify("find . -name '*.tmp' -exec rm {} \\;"),
        Layer1Verdict::Reject
    );
}

#[test]
fn chmod_rejects() {
    assert_eq!(classify("chmod 755 script.sh"), Layer1Verdict::Reject);
}

#[test]
fn chown_rejects() {
    assert_eq!(classify("chown user:user f"), Layer1Verdict::Reject);
}

#[test]
fn sudo_rejects() {
    assert_eq!(classify("sudo ls /root"), Layer1Verdict::Reject);
}

#[test]
fn eval_rejects() {
    assert_eq!(classify("eval 'echo hi'"), Layer1Verdict::Reject);
}

#[test]
fn git_checkout_rejects() {
    assert_eq!(classify("git checkout main"), Layer1Verdict::Reject);
}

#[test]
fn git_reset_rejects() {
    assert_eq!(classify("git reset --hard HEAD~1"), Layer1Verdict::Reject);
}

#[test]
fn git_clean_rejects() {
    assert_eq!(classify("git clean -fd"), Layer1Verdict::Reject);
}

#[test]
fn git_push_force_rejects() {
    assert_eq!(
        classify("git push --force origin main"),
        Layer1Verdict::Reject
    );
}

#[test]
fn git_stash_drop_rejects() {
    assert_eq!(classify("git stash drop"), Layer1Verdict::Reject);
}

#[test]
fn docker_rm_rejects() {
    assert_eq!(classify("docker rm -f mycontainer"), Layer1Verdict::Reject);
}

#[test]
fn docker_run_rejects() {
    assert_eq!(classify("docker run -it bash"), Layer1Verdict::Reject);
}

#[test]
fn kubectl_delete_rejects() {
    assert_eq!(classify("kubectl delete pod foo"), Layer1Verdict::Reject);
}

#[test]
fn npm_install_rejects() {
    assert_eq!(classify("npm install express"), Layer1Verdict::Reject);
}

#[test]
fn pip_install_rejects() {
    assert_eq!(classify("pip install requests"), Layer1Verdict::Reject);
}

#[test]
fn apt_install_rejects() {
    assert_eq!(classify("apt install foo"), Layer1Verdict::Reject);
}

#[test]
fn cargo_install_rejects() {
    assert_eq!(classify("cargo install ripgrep"), Layer1Verdict::Reject);
}

#[test]
fn make_clean_rejects() {
    assert_eq!(classify("make clean"), Layer1Verdict::Reject);
}

#[test]
fn mv_rejects() {
    assert_eq!(classify("mv a b"), Layer1Verdict::Reject);
}

#[test]
fn dot_source_rejects() {
    assert_eq!(classify(". ./env.sh"), Layer1Verdict::Reject);
}

// ---------- worst-case combining ----------

#[test]
fn reject_dominates_in_pipeline() {
    // rm in its own stage is a hard reject even behind a fast-safe stage.
    assert_eq!(classify("find . | rm"), Layer1Verdict::Reject);
}

#[test]
fn smart_read_dominates_safe() {
    // xargs always forces Layer 2 per spec, regardless of what it feeds.
    assert_eq!(
        classify("find . | xargs grep foo"),
        Layer1Verdict::NeedsSmartRead
    );
}

#[test]
fn xargs_rm_still_needs_smart_read_at_layer1() {
    // Spec: "xargs — invocation always forces Layer 2 regardless of what it
    // feeds." So `find . | xargs rm` lands at needs-smart-read; Layer 2 is
    // where the "rm" shows up and escalates to dangerous.
    assert_eq!(
        classify("find . | xargs rm"),
        Layer1Verdict::NeedsSmartRead
    );
}
