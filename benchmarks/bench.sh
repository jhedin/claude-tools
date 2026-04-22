#!/usr/bin/env bash
# Benchmark stub. When sibling impls land under impl-go/ and impl-python/,
# extend this to compare cold-start, warm-path, and cache-hit timings across
# binaries and write results to benchmarks/results.md.
#
# For now: measure cold-start and a layer1-only check against the Rust build.

set -euo pipefail

impl_rust_bin=${IMPL_RUST_BIN:-$(cd "$(dirname "$0")/.." && pwd)/impl-rust/target/release/claude-tools}

if [ ! -x "$impl_rust_bin" ]; then
  echo "build the release binary first: (cd impl-rust && cargo build --release)" >&2
  exit 1
fi

fixture_dir=$(mktemp -d)
trap 'rm -rf "$fixture_dir"' EXIT
cat > "$fixture_dir/plan.sh" <<'EOF'
find . -name '*.py' | grep -l TODO | sort | uniq -c
EOF

echo "# Benchmarks — $(date -Iseconds)"
echo
echo "## Cold start (help)"
for _ in 1 2 3; do
  /usr/bin/time -f '%e s' -- "$impl_rust_bin" --version >/dev/null
done

echo "## Layer 1 classification (fast-safe pipeline)"
for _ in 1 2 3; do
  /usr/bin/time -f '%e s' -- "$impl_rust_bin" check --layer1-only \
    --rewritten-file "$fixture_dir/plan.sh" --pwd "$PWD" \
    < /dev/null >/dev/null
done
