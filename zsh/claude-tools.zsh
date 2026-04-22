# claude-tools zsh widget
#
# Source from ~/.zshrc:
#
#   export CLAUDE_TOOLS_BIN=~/.local/bin/claude-tools
#   source ~/.local/share/claude-tools/claude-tools.zsh
#
# The widget intercepts `accept-line` and rewrites `c`-prefixed buffers (e.g.
# `cfind . "python files" | cgrep TODO`) into real Unix pipelines by calling
# `$CLAUDE_TOOLS_BIN`. Falls back to the original buffer on any planner
# failure.

: ${CLAUDE_TOOLS_BIN:=claude-tools}

# ---------- alias generation ----------
#
# Pull the configured tool list from the binary so the widget stays in sync
# with ~/.config/claude-tools/config.toml. Each alias is a no-op function
# whose body prints a helpful error — the widget matches on the token name,
# not on alias expansion, so these only fire outside the widget (scripts,
# subshells).

_claude_tools_install_aliases() {
  emulate -L zsh
  local tool
  local -a tools
  tools=("${(@f)$("$CLAUDE_TOOLS_BIN" config aliases 2>/dev/null)}")
  (( ${#tools} )) || tools=(find grep jq awk sed)
  for tool in $tools; do
    eval "c${tool}() { print -u2 -- 'claude-tools: c${tool} only works via the zsh widget (interactive Enter). This shell has \$CLAUDE_TOOLS_BIN=\${CLAUDE_TOOLS_BIN}; source claude-tools.zsh to enable.'; return 127; }"
  done
  typeset -g _CLAUDE_TOOLS_PREFIXES=""
  for tool in $tools; do
    _CLAUDE_TOOLS_PREFIXES+="c${tool} "
  done
}

_claude_tools_install_aliases

# ---------- helpers ----------

# Return 0 if $1 contains any configured c-prefixed tool, with or without a
# `c?` / `cc` modifier. Substring match is intentional — false positives just
# mean the planner sees a buffer it'll pass through unchanged.
_claude_tools_buffer_has_trigger() {
  emulate -L zsh
  local buf=$1 tok name
  for tok in ${=_CLAUDE_TOOLS_PREFIXES}; do
    name=${tok#c}
    if [[ $buf == *c${name}* || $buf == *c\?${name}* || $buf == *cc${name}* ]]; then
      return 0
    fi
  done
  return 1
}

# sha256 of a string via `shasum` or `sha256sum`.
_claude_tools_sha256() {
  if command -v shasum >/dev/null 2>&1; then
    print -rn -- "$1" | shasum -a 256 | awk '{print $1}'
  else
    print -rn -- "$1" | sha256sum | awk '{print $1}'
  fi
}

# Detect & strip leading `c?` / `cc` modifiers on any c-prefixed token.
# Echoes "<mode>\n<new_buffer>". Mode: normal | dry | confirm.
_claude_tools_extract_mode() {
  emulate -L zsh
  local buf=$1
  # Look for the first occurrence of c? or cc adjacent to a known tool name.
  local mode="normal"
  local tok name stripped=$buf
  for tok in ${=_CLAUDE_TOOLS_PREFIXES}; do
    name=${tok#c}
    if [[ $buf == *c\?${name}* ]]; then
      mode="dry"
      stripped=${buf//c\?${name}/c${name}}
      break
    elif [[ $buf == *cc${name}* ]]; then
      mode="confirm"
      stripped=${buf//cc${name}/c${name}}
      break
    fi
  done
  print -r -- "$mode"
  print -r -- "$stripped"
}

# Show the explanation line below the command (dim gray `#` comment).
_claude_tools_show_explanation() {
  print -P -- "%F{8}# ${1}%f"
}

_claude_tools_warn() {
  print -P -- "%F{yellow}⚠ ${1}%f" 1>&2
}

_claude_tools_error() {
  print -P -- "%F{red}✖ ${1}%f" 1>&2
}

# Read "yes" / "no" from the terminal. Default N.
_claude_tools_confirm() {
  local prompt=$1 reply
  print -n -- "${prompt} [y/N] "
  read -k1 reply
  print
  [[ $reply == y || $reply == Y ]]
}

# Write the error log entry (mirrors the binary's log path).
_claude_tools_log_error() {
  local dir
  dir=${XDG_DATA_HOME:-$HOME/.local/share}/claude-tools
  mkdir -p "$dir" 2>/dev/null
  print -r -- "$(date -Iseconds) $1" >> "$dir/errors.log"
}

# ---------- the widget itself ----------

claude-tools-accept-line() {
  emulate -L zsh
  local original=$BUFFER

  # Fast path: no trigger token → hand off to default accept-line.
  if ! _claude_tools_buffer_has_trigger "$original"; then
    zle .accept-line
    return
  fi

  # Extract c?/cc modifiers.
  local info mode planned
  info=$(_claude_tools_extract_mode "$original")
  mode=${info%%$'\n'*}
  planned=${info#*$'\n'}

  # Cache key = sha256(buffer + cwd). Use the *planned* (modifier-stripped)
  # buffer so c?/cc doesn't affect cache hits.
  local key
  key=$(_claude_tools_sha256 "${planned}${PWD}")

  # Cache lookup.
  local cached
  cached=$("$CLAUDE_TOOLS_BIN" cache get --key "$key" 2>/dev/null)

  local rewritten explanation layer1 layer2 reason
  if [[ -n $cached ]]; then
    rewritten=$(print -r -- "$cached" | _claude_tools_json_field command)
    explanation=$(print -r -- "$cached" | _claude_tools_json_field explanation)
    layer1=$(print -r -- "$cached" | _claude_tools_json_field layer1)
    layer2=$(print -r -- "$cached" | _claude_tools_json_field layer2)
    reason=$(print -r -- "$cached" | _claude_tools_json_field reason)
  else
    # Plan.
    local plan_json
    plan_json=$(print -r -- "$planned" | "$CLAUDE_TOOLS_BIN" plan --pwd "$PWD" 2>>"${XDG_DATA_HOME:-$HOME/.local/share}/claude-tools/errors.log")
    if [[ -z $plan_json ]]; then
      _claude_tools_warn "planner unavailable — submitting buffer unmodified"
      zle .accept-line
      return
    fi
    rewritten=$(print -r -- "$plan_json" | _claude_tools_json_field command)
    explanation=$(print -r -- "$plan_json" | _claude_tools_json_field explanation)
    if [[ -z $rewritten ]]; then
      _claude_tools_warn "planner returned empty command — submitting buffer unmodified"
      zle .accept-line
      return
    fi

    # Check.
    local rewritten_file=$(mktemp -t claude-tools-rewritten.XXXXXX)
    print -r -- "$rewritten" > "$rewritten_file"
    local check_json
    check_json=$(print -r -- "$planned" | "$CLAUDE_TOOLS_BIN" check --rewritten-file "$rewritten_file" --pwd "$PWD" 2>>"${XDG_DATA_HOME:-$HOME/.local/share}/claude-tools/errors.log")
    rm -f "$rewritten_file"
    if [[ -z $check_json ]]; then
      # Fail closed per spec: treat as suspicious.
      layer1="needs-smart-read"
      layer2="suspicious"
      reason="safety checker unavailable; fail-closed"
    else
      layer1=$(print -r -- "$check_json" | _claude_tools_json_field layer1)
      layer2=$(print -r -- "$check_json" | _claude_tools_json_field layer2)
      reason=$(print -r -- "$check_json" | _claude_tools_json_field reason)
    fi

    # Persist to cache: combined plan+check JSON.
    local combined
    combined=$(printf '{"command":%s,"explanation":%s,"layer1":%s,"layer2":%s,"reason":%s}' \
      "$(_claude_tools_json_encode "$rewritten")" \
      "$(_claude_tools_json_encode "$explanation")" \
      "$(_claude_tools_json_encode "$layer1")" \
      "$(_claude_tools_json_encode "$layer2")" \
      "$(_claude_tools_json_encode "$reason")")
    print -r -- "$combined" | "$CLAUDE_TOOLS_BIN" cache put --key "$key" 2>/dev/null
  fi

  # Dry run: just show, don't execute.
  if [[ $mode == dry ]]; then
    zle -I
    print -r -- "$rewritten"
    [[ -n $explanation ]] && _claude_tools_show_explanation "$explanation"
    [[ -n $reason && $layer1 != "fast-safe" ]] && print -P -- "%F{8}# safety: ${layer1}/${layer2} — ${reason}%f"
    # Two history entries: original then rewritten (most recent).
    print -s -- "$original"
    print -s -- "$rewritten"
    _claude_tools_append_history "$original" "$rewritten" "$explanation" \
      "$layer1" "$layer2" "$reason" "false"
    BUFFER=""
    zle .accept-line
    return
  fi

  # Decide execute vs. confirm.
  local need_confirm=0
  case "${layer1}:${layer2}" in
    fast-safe:*) need_confirm=0 ;;
    needs-smart-read:safe) need_confirm=0 ;;
    needs-smart-read:suspicious) need_confirm=1 ;;
    needs-smart-read:dangerous) need_confirm=1 ;;
    reject:*) need_confirm=1 ;;
    *) need_confirm=1 ;;
  esac
  [[ $mode == confirm ]] && need_confirm=1

  # Display the rewritten command line in place (replace the buffer so it
  # shows where the user's input was).
  zle -I
  print -r -- "$rewritten"
  [[ -n $explanation ]] && _claude_tools_show_explanation "$explanation"

  local executed="false"
  if (( need_confirm )); then
    local color=yellow
    [[ $layer2 == dangerous || $layer1 == reject ]] && color=red
    [[ -n $reason ]] && print -P -- "%F{${color}}⚠ ${layer1}/${layer2:-n/a}: ${reason}%f"
    if _claude_tools_confirm "Execute?"; then
      executed="true"
    else
      _claude_tools_warn "rejected — re-opening for editing"
      BUFFER="$rewritten"
      CURSOR=${#BUFFER}
      zle .redisplay
      _claude_tools_append_history "$original" "$rewritten" "$explanation" \
        "$layer1" "$layer2" "$reason" "false"
      return
    fi
  else
    executed="true"
  fi

  # Two history entries: push the natural-language buffer first (becomes
  # Up-arrow × 2), then set BUFFER to rewritten and let accept-line push it
  # as the most recent (Up-arrow × 1).
  print -s -- "$original"
  BUFFER="$rewritten"
  _claude_tools_append_history "$original" "$rewritten" "$explanation" \
    "$layer1" "$layer2" "$reason" "$executed"
  zle .accept-line
}

# Extract a JSON string field from stdin. Uses jq if available, otherwise a
# small awk fallback. The fallback handles the subset of output our binary
# emits (single-line compact JSON, no nested objects at the top level).
_claude_tools_json_field() {
  local field=$1
  if command -v jq >/dev/null 2>&1; then
    jq -r "if .${field} == null then \"\" else .${field} | tostring end" 2>/dev/null
  else
    awk -v f="$field" '
      {
        n = index($0, "\"" f "\":")
        if (n == 0) { next }
        rest = substr($0, n + length(f) + 3)
        # strip leading whitespace
        sub(/^[ \t]+/, "", rest)
        if (substr(rest, 1, 1) == "\"") {
          # string
          s = ""
          for (i = 2; i <= length(rest); i++) {
            c = substr(rest, i, 1)
            if (c == "\\") { s = s substr(rest, i+1, 1); i++; continue }
            if (c == "\"") { break }
            s = s c
          }
          print s
        } else if (substr(rest, 1, 4) == "null") {
          print ""
        } else {
          # number / bool — read until comma or brace
          split(rest, a, /[,}]/)
          print a[1]
        }
      }'
  fi
}

# Encode a shell string as a JSON string (with surrounding quotes).
_claude_tools_json_encode() {
  local s=$1
  if command -v jq >/dev/null 2>&1; then
    print -rn -- "$s" | jq -Rsc .
  else
    # Minimal escaper: backslash, quote, newline, tab.
    local out=${s//\\/\\\\}
    out=${out//\"/\\\"}
    out=${out//$'\n'/\\n}
    out=${out//$'\t'/\\t}
    print -rn -- "\"$out\""
  fi
}

_claude_tools_append_history() {
  local original=$1 rewritten=$2 explanation=$3 layer1=$4 layer2=$5 reason=$6 executed=$7
  local payload
  payload=$(printf '{"timestamp":"%s","cwd":%s,"original_buffer":%s,"rewritten":%s,"explanation":%s,"layer1_verdict":%s,"layer2_verdict":%s,"layer2_reason":%s,"executed":%s}' \
    "$(date -Iseconds)" \
    "$(_claude_tools_json_encode "$PWD")" \
    "$(_claude_tools_json_encode "$original")" \
    "$(_claude_tools_json_encode "$rewritten")" \
    "$(_claude_tools_json_encode "$explanation")" \
    "$(_claude_tools_json_encode "$layer1")" \
    "$(_claude_tools_json_encode "$layer2")" \
    "$(_claude_tools_json_encode "$reason")" \
    "$executed")
  print -r -- "$payload" | "$CLAUDE_TOOLS_BIN" history append 2>/dev/null
}

# ---------- bind ----------

zle -N claude-tools-accept-line
bindkey '^M' claude-tools-accept-line
bindkey '^J' claude-tools-accept-line
