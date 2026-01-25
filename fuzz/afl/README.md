# AFL++ fuzzing for wiki2md

This project includes an AFL++-style, stdin-driven fuzz target.

## Setup

Install the Rust AFL tooling (`cargo-afl`) as described by the Rust Fuzz Book. Then install AFL++.

- `cargo-afl` setup instructions: https://rust-fuzz.github.io/book/afl/setup.html
- `cargo afl fuzz ...` usage: https://rust-fuzz.github.io/book/afl/tutorial.html

## Build the fuzz target

```bash
cargo install cargo-afl

# build the fuzz target with AFL instrumentation
cargo afl build --release --features afl_fuzz --bin wiki2md_afl_parse
```

The resulting binary will be at:

```
target/release/wiki2md_afl_parse
```

## Run the fuzzer

AFL++ requires at least one seed input.

```bash
# run from the repo root
mkdir -p fuzz/afl/out

cargo afl fuzz \
  -i fuzz/afl/in \
  -o fuzz/afl/out \
  -x fuzz/afl/dict/wikitext.dict \
  target/release/wiki2md_afl_parse
```

## Reproducing crashes

```bash
# pick any crash file from fuzz/afl/out/.../crashes/
cargo afl run --features afl_fuzz --bin wiki2md_afl_parse < fuzz/afl/out/default/crashes/id:....
```

AFL++ will keep a minimized reproducer in the `crashes/` directory.
