#!/usr/bin/env bash
# Measure handwritten production code independently for every workspace crate.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $(rustc --version) != *nightly* ]]; then
  echo "Coverage requires the Nix development shell's nightly compiler." >&2
  exit 1
fi

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
export CARGO_LLVM_COV_TARGET_DIR="$PWD/target/coverage-build"
reports="$PWD/target/coverage"
mkdir -p "$reports"

# A full cleanup also removes binaries produced by older Cargo artifact layouts.
cargo llvm-cov clean
cargo llvm-cov --workspace --all-features --locked --no-report

crates=(detritus-client detritus-protocol detritus-server)
failed=0
for crate in "${crates[@]}"; do
  ignore='(/tests(/|\.rs$)|/examples/|/build\.rs$|/target/)'
  for other in "${crates[@]}"; do
    if [[ "$other" != "$crate" ]]; then
      ignore+="|/crates/$other/"
    fi
  done
  report_args=(--ignore-filename-regex "$ignore")
  if ! cargo llvm-cov report "${report_args[@]}" --json \
    --output-path "$reports/$crate.json" --fail-under-lines 90; then
    failed=1
  fi
  cargo llvm-cov report "${report_args[@]}" --html --output-dir "$reports/$crate"
done
exit "$failed"
