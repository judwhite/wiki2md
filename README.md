# wiki2md

**wiki2md** converts [MediaWiki markup](https://www.mediawiki.org/wiki/Help:Formatting) to [GitHub Flavored Markdown](https://github.github.com/gfm/), because portability is awesome, and why not? If you want a mature project that does this [Pandoc](https://github.com/jgm/pandoc) is the most well-known.

As of this commit, the code fetches articles from [chessprogramming.org](https://www.chessprogramming.org) and turns them into `.json` and `.md` files.

Need Rust? 🦀 [https://rustup.rs](https://rustup.rs)

### Run it <small>⚙️ ⚙️ ⚙️</small>

```bash
$ git clone https://github.com/judwhite/wiki2md
$ cd wiki2md
$ cargo build && cargo test && cargo clippy
$ cargo build --release && target/release/wiki2md Ken_Thompson
```

This will create three files relative to the current working directory:

1. `./docs/wiki/k/Ken_Thompson.wiki` - Raw Wikitext from https://www.chessprogramming.org/index.php?title=Ken_Thompson&action=edit
2. `./docs/json/k/Ken_Thompson.json` - `.json` AST representation of the article, because trying to go straight to `.md` using Regex was killing me. 🫠
3. `./docs/md/k/Ken_Thompson.md` - `.md` output, transformed from the `.json` AST.

If you want to regenerate all of the `.md` files (because you changed something about the rendering, for example), run with `--regenerate-all`:

```bash
$ cargo build --release && target/release/wiki2md --regenerate-all
    Finished `release` profile [optimized] target(s) in 0.15s
[   1/1157] [00:00.007] Regenerated: "docs/md/0/0x88.md"
[   2/1157] [00:00.012] Regenerated: "docs/md/1/10th_Computer_Olympiad.md"
[   3/1157] [00:00.013] Regenerated: "docs/md/1/10x12_Board.md"
...
[1155/1157] [00:02.551] Regenerated: "docs/md/z/Zobrist_Hashing.md"
[1156/1157] [00:02.551] Regenerated: "docs/md/z/Zugzwang.md"
[1157/1157] [00:02.552] Regenerated: "docs/md/é/Zwischenzug.md"
Done. Regenerated 1157 files in 2.553s (avg 0.002s/doc).
```

### Fuzzing <small>🧨</small>

The parser is intentionally tolerant, but it must *never* panic, hang, or produce out-of-bounds spans.

This repo includes an AFL++-style fuzz target:

- See `fuzz/afl/README.md` for instructions.
