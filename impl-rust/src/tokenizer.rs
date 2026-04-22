use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub op: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    pub program: String,
    pub args: Vec<String>,
    pub redirects: Vec<Redirect>,
}

impl Stage {
    pub fn has_flag(&self, f: &str) -> bool {
        self.args.iter().any(|a| a == f)
    }

    pub fn has_arg_prefix(&self, p: &str) -> bool {
        self.args.iter().any(|a| a.starts_with(p))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedPipeline {
    pub stages: Vec<Stage>,
    /// Separators between stages: "|", "&&", ";", "||".
    pub separators: Vec<String>,
    pub has_command_substitution: bool,
    pub has_backticks: bool,
    /// If the raw source could not be parsed into tokens, this is true and
    /// callers should treat the command as opaque.
    pub unparseable: bool,
}

impl ParsedPipeline {
    pub fn has_compound(&self) -> bool {
        self.separators
            .iter()
            .any(|s| s == "&&" || s == "||" || s == ";")
    }

    pub fn programs(&self) -> HashSet<&str> {
        self.stages.iter().map(|s| s.program.as_str()).collect()
    }
}

/// Parse a shell command line into a ParsedPipeline.
///
/// Best effort: splits on `|`, `&&`, `;`, `||` outside of quotes and `$(...)` /
/// backticks, then tokenizes each stage with `shlex`. Redirect operators
/// (`>`, `>>`, `<`, `2>`, `&>`) are pulled out of the arg list so callers can
/// reason about destinations separately.
pub fn parse(src: &str) -> ParsedPipeline {
    let mut out = ParsedPipeline::default();
    out.has_command_substitution = scan_command_substitution(src);
    out.has_backticks = scan_backticks(src);

    let (stage_srcs, separators) = split_top_level(src);
    out.separators = separators;

    for s in stage_srcs {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            out.unparseable = true;
            continue;
        }
        let tokens = match shlex::split(trimmed) {
            Some(t) if !t.is_empty() => t,
            _ => {
                out.unparseable = true;
                continue;
            }
        };
        let (program, args, redirects) = classify_tokens(tokens);
        out.stages.push(Stage {
            program,
            args,
            redirects,
        });
    }

    out
}

fn classify_tokens(tokens: Vec<String>) -> (String, Vec<String>, Vec<Redirect>) {
    let mut program = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut redirects: Vec<Redirect> = Vec::new();
    let mut iter = tokens.into_iter().peekable();

    // Skip simple `VAR=value` env prefix(es) to find the real program name.
    while let Some(tok) = iter.peek() {
        if is_env_assignment(tok) {
            iter.next();
        } else {
            break;
        }
    }
    if let Some(first) = iter.next() {
        program = first;
    }

    while let Some(tok) = iter.next() {
        if let Some(op) = redirect_op(&tok) {
            let target = iter.next().unwrap_or_default();
            redirects.push(Redirect { op, target });
        } else if let Some((op, target)) = split_glued_redirect(&tok) {
            redirects.push(Redirect { op, target });
        } else {
            args.push(tok);
        }
    }

    (program, args, redirects)
}

fn is_env_assignment(tok: &str) -> bool {
    if let Some(eq) = tok.find('=') {
        if eq == 0 {
            return false;
        }
        let name = &tok[..eq];
        name.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false)
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

fn redirect_op(tok: &str) -> Option<String> {
    matches!(tok, ">" | ">>" | "<" | "2>" | "2>>" | "&>" | "&>>" | "<<<").then(|| tok.to_string())
}

fn split_glued_redirect(tok: &str) -> Option<(String, String)> {
    for op in [">>", "2>>", "&>>", "2>", "&>", ">", "<<<", "<"] {
        if let Some(rest) = tok.strip_prefix(op) {
            if !rest.is_empty() {
                return Some((op.to_string(), rest.to_string()));
            }
        }
    }
    None
}

fn scan_command_substitution(src: &str) -> bool {
    // Looking for an unescaped `$(` outside of single quotes.
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if c == b'\'' && !in_double {
            in_single = !in_single;
        } else if c == b'"' && !in_single {
            in_double = !in_double;
        } else if !in_single && c == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            return true;
        }
        i += 1;
    }
    false
}

fn scan_backticks(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if c == b'\'' {
            in_single = !in_single;
        } else if !in_single && c == b'`' {
            return true;
        }
        i += 1;
    }
    false
}

/// Split the command line into stages on `|`, `&&`, `;`, `||` at the top level
/// (outside of quotes, `$(...)`, backticks, `${...}`). Returns (stages,
/// separators) where separators.len() == stages.len() - 1.
fn split_top_level(src: &str) -> (Vec<String>, Vec<String>) {
    let mut stages: Vec<String> = Vec::new();
    let mut seps: Vec<String> = Vec::new();
    let bytes = src.as_bytes();
    let mut cur = String::new();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut paren_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut in_backtick = false;

    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            cur.push(c as char);
            cur.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        if in_single {
            cur.push(c as char);
            if c == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if c == b'\'' {
            in_single = true;
            cur.push(c as char);
            i += 1;
            continue;
        }
        if c == b'"' {
            in_double = !in_double;
            cur.push(c as char);
            i += 1;
            continue;
        }
        if in_double {
            cur.push(c as char);
            i += 1;
            continue;
        }
        if c == b'`' {
            in_backtick = !in_backtick;
            cur.push(c as char);
            i += 1;
            continue;
        }
        if in_backtick {
            cur.push(c as char);
            i += 1;
            continue;
        }
        if c == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            cur.push('$');
            cur.push('(');
            paren_depth += 1;
            i += 2;
            continue;
        }
        if c == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            cur.push('$');
            cur.push('{');
            brace_depth += 1;
            i += 2;
            continue;
        }
        if paren_depth > 0 {
            if c == b'(' {
                paren_depth += 1;
            } else if c == b')' {
                paren_depth -= 1;
            }
            cur.push(c as char);
            i += 1;
            continue;
        }
        if brace_depth > 0 {
            if c == b'{' {
                brace_depth += 1;
            } else if c == b'}' {
                brace_depth -= 1;
            }
            cur.push(c as char);
            i += 1;
            continue;
        }

        // Top level — split on operators.
        if c == b'&' && i + 1 < bytes.len() && bytes[i + 1] == b'&' {
            stages.push(std::mem::take(&mut cur));
            seps.push("&&".to_string());
            i += 2;
            continue;
        }
        if c == b'|' && i + 1 < bytes.len() && bytes[i + 1] == b'|' {
            stages.push(std::mem::take(&mut cur));
            seps.push("||".to_string());
            i += 2;
            continue;
        }
        if c == b'|' {
            stages.push(std::mem::take(&mut cur));
            seps.push("|".to_string());
            i += 1;
            continue;
        }
        if c == b';' {
            stages.push(std::mem::take(&mut cur));
            seps.push(";".to_string());
            i += 1;
            continue;
        }

        cur.push(c as char);
        i += 1;
    }

    stages.push(cur);
    (stages, seps)
}
