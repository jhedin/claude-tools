#!/usr/bin/env bash
# Install claude-tools.
#
# Builds the Rust binary in release mode, places it at ~/.local/bin/claude-tools,
# copies the zsh widget to ~/.local/share/claude-tools/, and seeds a default
# config at ~/.config/claude-tools/config.toml if none exists.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")" && pwd)
bin_dir=${CLAUDE_TOOLS_INSTALL_BIN_DIR:-$HOME/.local/bin}
share_dir=${CLAUDE_TOOLS_INSTALL_SHARE_DIR:-$HOME/.local/share/claude-tools}
config_dir=${CLAUDE_TOOLS_INSTALL_CONFIG_DIR:-$HOME/.config/claude-tools}

mkdir -p "$bin_dir" "$share_dir" "$config_dir"

echo "==> Building release binary"
(cd "$repo_root/impl-rust" && cargo build --release)

echo "==> Installing binary to $bin_dir/claude-tools"
install -m 0755 "$repo_root/impl-rust/target/release/claude-tools" "$bin_dir/claude-tools"

echo "==> Installing zsh widget to $share_dir/claude-tools.zsh"
install -m 0644 "$repo_root/zsh/claude-tools.zsh" "$share_dir/claude-tools.zsh"

if [ ! -f "$config_dir/config.toml" ]; then
  echo "==> Seeding default config at $config_dir/config.toml"
  install -m 0644 "$repo_root/config/config.toml.example" "$config_dir/config.toml"
else
  echo "==> Leaving existing $config_dir/config.toml untouched"
fi

cat <<MSG

claude-tools installed.

Add this to ~/.zshrc:

  export CLAUDE_TOOLS_BIN=$bin_dir/claude-tools
  source $share_dir/claude-tools.zsh

Open a new shell and try:

  cfind . "python files modified this week"

MSG
