#/!bin/bash

# Run all hangs and see which are actually slow
for f in fuzz/afl/out/default/hangs/*; do
    echo "=== $f ==="
    time timeout 10s cargo afl run --features afl_fuzz --bin wiki2md_afl_parse < "$f"
done
