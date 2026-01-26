#!/bin/bash
set -e

BIN=wiki2md_afl_parse

# 1. Build with coverage instrumentation
RUSTFLAGS="-C instrument-coverage" cargo build --features afl_fuzz --bin "$BIN"

# 2. Run corpus through it
export LLVM_PROFILE_FILE="fuzz-%p-%m.profraw"
for f in fuzz/afl/out/default/queue/*; do
    ./target/debug/"$BIN" < "$f" 2>/dev/null || true
done

# 3. Merge profile data
rust-profdata merge -sparse *.profraw -o fuzz.profdata

# 4. Generate HTML report
rust-cov show ./target/debug/"$BIN" \
    --instr-profile=fuzz.profdata \
    --format=html \
    --output-dir=coverage \
    --ignore-filename-regex='/.cargo/|/rustc/'

# 5. Cleanup
rm -f *.profraw

# 6. View report
xdg-open coverage/index.html