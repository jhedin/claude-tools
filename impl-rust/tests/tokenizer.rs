use claude_tools::tokenizer;

fn parse(s: &str) -> tokenizer::ParsedPipeline {
    tokenizer::parse(s)
}

#[test]
fn simple_command() {
    let p = parse("ls -la");
    assert_eq!(p.stages.len(), 1);
    assert_eq!(p.stages[0].program, "ls");
    assert_eq!(p.stages[0].args, vec!["-la"]);
    assert!(p.separators.is_empty());
}

#[test]
fn pipe_split() {
    let p = parse("find . | grep TODO | sort | uniq -c");
    assert_eq!(p.stages.len(), 4);
    assert_eq!(p.stages[0].program, "find");
    assert_eq!(p.stages[1].program, "grep");
    assert_eq!(p.stages[3].program, "uniq");
    assert_eq!(p.separators, vec!["|", "|", "|"]);
}

#[test]
fn quoted_pipe_does_not_split() {
    let p = parse("echo \"a|b\"");
    assert_eq!(p.stages.len(), 1);
    assert_eq!(p.stages[0].program, "echo");
    assert_eq!(p.stages[0].args, vec!["a|b"]);
}

#[test]
fn single_quoted_pipe_does_not_split() {
    let p = parse("echo 'a|b|c'");
    assert_eq!(p.stages.len(), 1);
    assert_eq!(p.stages[0].args, vec!["a|b|c"]);
}

#[test]
fn command_substitution_detected() {
    let p = parse("echo $(date)");
    assert!(p.has_command_substitution);
    // Substitution lives inside one stage.
    assert_eq!(p.stages.len(), 1);
}

#[test]
fn pipe_inside_substitution_does_not_split_stages() {
    let p = parse("echo $(echo foo | tr a-z A-Z)");
    assert_eq!(p.stages.len(), 1);
    assert!(p.has_command_substitution);
}

#[test]
fn backticks_detected() {
    let p = parse("echo `date`");
    assert!(p.has_backticks);
}

#[test]
fn compound_and() {
    let p = parse("make && echo done");
    assert_eq!(p.stages.len(), 2);
    assert_eq!(p.separators, vec!["&&"]);
    assert!(p.has_compound());
}

#[test]
fn semicolon_compound() {
    let p = parse("cd /tmp; ls");
    assert_eq!(p.stages.len(), 2);
    assert_eq!(p.separators, vec![";"]);
}

#[test]
fn or_compound() {
    let p = parse("test -f x || touch x");
    assert_eq!(p.stages.len(), 2);
    assert_eq!(p.separators, vec!["||"]);
}

#[test]
fn redirect_to_file() {
    let p = parse("cmd > /tmp/out");
    assert_eq!(p.stages[0].program, "cmd");
    assert_eq!(p.stages[0].redirects.len(), 1);
    assert_eq!(p.stages[0].redirects[0].op, ">");
    assert_eq!(p.stages[0].redirects[0].target, "/tmp/out");
}

#[test]
fn stderr_redirect_and_pipe() {
    let p = parse("cmd 2>&1 | tee log");
    assert_eq!(p.stages.len(), 2);
    assert_eq!(p.stages[1].program, "tee");
    assert_eq!(p.stages[1].args, vec!["log"]);
}

#[test]
fn append_redirect() {
    let p = parse("echo hi >> log.txt");
    assert_eq!(p.stages[0].redirects[0].op, ">>");
    assert_eq!(p.stages[0].redirects[0].target, "log.txt");
}

#[test]
fn find_delete_preserved_as_flag() {
    let p = parse("find . -name '*.py' -delete");
    assert_eq!(p.stages[0].program, "find");
    assert!(p.stages[0].has_flag("-delete"));
}

#[test]
fn sed_inplace_flag_detected() {
    let p = parse("sed -i 's/foo/bar/' file");
    assert_eq!(p.stages[0].program, "sed");
    assert!(p.stages[0].has_flag("-i"));
}

#[test]
fn env_prefix_skipped_to_find_program() {
    let p = parse("LANG=C grep foo bar");
    assert_eq!(p.stages[0].program, "grep");
    assert_eq!(p.stages[0].args, vec!["foo", "bar"]);
}

#[test]
fn glued_redirect_splits() {
    let p = parse("cmd >/tmp/out");
    assert_eq!(p.stages[0].redirects.len(), 1);
    assert_eq!(p.stages[0].redirects[0].op, ">");
    assert_eq!(p.stages[0].redirects[0].target, "/tmp/out");
}
