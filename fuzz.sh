#!/bin/bash
set -euo pipefail

# configuration
BIN="wiki2md_afl_parse"
IN_DIR="fuzz/afl/in"
OUT_DIR="fuzz/afl/out"
DICT_FILE="fuzz/afl/dict/wikitext.dict"
TARGET_BIN="target/release/$BIN"
DEBUG_BIN="target/debug/$BIN"

# build the fuzzer (release mode)
build_fuzz() {
    echo "[*] Building fuzzer (Release)..."
    cargo afl build --release --features afl_fuzz --bin "$BIN"
}

# build for coverage (debug mode with instrumentation)
build_cov() {
    echo "[*] Building for coverage (Debug + Instrumentation)..."
    RUSTFLAGS="-C instrument-coverage" cargo build --features afl_fuzz --bin "$BIN"
}

# ensure we have an action
if [ -z "${1:-}" ]; then
    echo "Usage: ./fuzz.sh [new|resume|cov|repro|hangs]"
    exit 1
fi

ACTION="$1"

case "$ACTION" in
    new)
        # nuke and pave
        build_fuzz
        if [ -z "$(ls -A $IN_DIR 2>/dev/null)" ]; then
           echo "Error: '$IN_DIR' is empty or missing."
           exit 1
        fi

        echo "[*] Starting NEW fuzzing session..."
        if [ -d "$OUT_DIR" ]; then
            echo "    -> Cleaning previous session..."
            rm -rf "$OUT_DIR"
        fi
        mkdir -p "$OUT_DIR"

        cargo afl fuzz \
          -i "$IN_DIR" \
          -o "$OUT_DIR" \
          -x "$DICT_FILE" \
          "$TARGET_BIN"
        ;;

    resume)
        # resume existing session
        build_fuzz
        if [ ! -d "$OUT_DIR" ]; then
            echo "Error: No output directory found at $OUT_DIR. Cannot resume."
            exit 1
        fi

        echo "[*] RESUMING fuzzing session..."
        cargo afl fuzz \
          -i- \
          -o "$OUT_DIR" \
          -x "$DICT_FILE" \
          "$TARGET_BIN"
        ;;

    cov|cover|coverage)
        # coverage report
        build_cov

        CORPUS_DIR="$OUT_DIR/default/queue"
        if [ ! -d "$CORPUS_DIR" ]; then
            echo "Error: No corpus found at $CORPUS_DIR. Run 'new' first."
            exit 1
        fi

        echo "[*] Replaying corpus for coverage..."
        export LLVM_PROFILE_FILE="fuzz-%p-%m.profraw"

        # allow failure here because the fuzzer might have found inputs that abort
        set +e
        for f in "$CORPUS_DIR"/*; do
            # run silently, we only care about the profraw generation
            "$DEBUG_BIN" < "$f" >/dev/null 2>&1
        done
        set -e

        echo "[*] Merging profile data..."
        rust-profdata merge -sparse *.profraw -o fuzz.profdata

        echo "[*] Generating HTML report..."
        rust-cov show "$DEBUG_BIN" \
            --instr-profile=fuzz.profdata \
            --format=html \
            --output-dir=coverage \
            --ignore-filename-regex='/.rustup/|/.cargo/|/rustc/'

        echo "[*] Cleanup..."
        rm -f *.profraw

        echo "[*] Done! Opening report..."
        xdg-open coverage/index.html 2>/dev/null || echo "Report available at: coverage/index.html"
        ;;

    repro|repro-crashers)
        # reproduce, minimize, and deduplicate crashes

        # use the debug bin for better stack traces
        echo "[*] Building Debug binary for analysis..."
        cargo build --features afl_fuzz --bin "$BIN"

        CRASH_DIR="$OUT_DIR/default/crashes"
        MIN_DIR="fuzz/repro_minified"
        mkdir -p "$MIN_DIR"

        echo "[*] Analyzing crashes from $CRASH_DIR..."

        # disable instant exit on error, because crashes create errors
        set +e

        count=0
        for f in "$CRASH_DIR"/id*; do
            [ -e "$f" ] || continue

            filename=$(basename "$f")
            echo "--------------------------------------------------"
            echo "Processing: $filename"

            # capture the stack trace
            output=$("$DEBUG_BIN" < "$f" 2>&1)
            # grep for "panicked at" to capture the location line
            panic_loc=$(echo "$output" | grep "panicked at")

            if [ -z "$panic_loc" ]; then
                echo "  -> No panic found (might be a signal/segfault)."
                panic_hash="signal_$(md5sum "$f" | cut -d' ' -f1)"
            else
                echo "  -> Found panic: $panic_loc"
                # hash the panic location to distinguish unique bugs
                panic_hash=$(echo "$panic_loc" | md5sum | cut -d' ' -f1)
            fi

            target_min="$MIN_DIR/${panic_hash}.min"

            if [ -f "$target_min" ]; then
                echo "  -> Skipping: We already have a minified repro for this crash location ($panic_hash)."
            else
                echo "  -> UNIQUE CRASH! Minifying..."
                # use tmin to minimize the file
                cargo afl tmin -i "$f" -o "$target_min" "$TARGET_BIN"
                echo "  -> Saved minified repro to: $target_min"
            fi
            ((count=count+1))
        done
        set -e
        echo "--------------------------------------------------"
        echo "Analysis complete. Check '$MIN_DIR' for unique minified crashers."
        ;;

    hang|hangs|repro-hang|repro-hangs)
        # check hangs
        build_fuzz
        HANGS_DIR="$OUT_DIR/default/hangs"

        echo "[*] Verifying hangs..."
        set +e
        for f in "$HANGS_DIR"/id*; do
            [ -e "$f" ] || continue
            echo -n "Testing $(basename "$f")... "

            # timeout after 4 seconds. if it takes longer, it's a real hang.
            timeout 2s "$TARGET_BIN" < "$f" >/dev/null 2>&1
            exit_code=$?

            if [ $exit_code -eq 124 ]; then
                echo "CONFIRMED (Timed out)"
            else
                echo "False Positive (Exit code: $exit_code)"
            fi
        done
        set -e
        ;;

    *)
        echo "Unknown action: $ACTION"
        echo "Usage: ./fuzz.sh [new|resume|cov|repro|hangs]"
        exit 1
        ;;
esac
