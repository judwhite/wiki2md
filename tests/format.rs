use std::fs;
use std::path::PathBuf;
use wiki2md::{parse, render};

fn base_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
}

#[test]
fn test_wikitable_text_align() {
    let in_path = base_dir().join("001-in-wikitable-text-align.wiki");
    let want_path = base_dir().join("001-want-wikitable-text-align.txt");

    let want_bytes = fs::read(&want_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", want_path.display()));
    let want_md = String::from_utf8_lossy(&want_bytes).into_owned();

    let in_bytes =
        fs::read(&in_path).unwrap_or_else(|e| panic!("failed to read {}: {e}", in_path.display()));
    let in_str = String::from_utf8_lossy(&in_bytes).into_owned();
    let ast = parse::parse_wiki(&in_str);
    let actual_md = render::render_doc(&ast.document);

    let out_path = base_dir().join("001-out-wikitable-text-align.txt");
    if !actual_md.eq(&want_md) {
        fs::write(&out_path, &actual_md)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));
    } else if out_path.exists() {
        fs::remove_file(&out_path)
            .unwrap_or_else(|e| panic!("failed to remove {}: {e}", out_path.display()));
    }

    assert_eq!(actual_md, want_md);
}

#[test]
fn test_table_centering_option_wraps_caption_and_table() {
    // verify that when `center_tables_and_captions` is enabled, the renderer
    // wraps the caption + table in a centering HTML container.
    let src = "{| class=\"wikitable\"\n|+ Caption\n|-\n! H1\n! H2\n|-\n! A\n| B\n|}\n";

    let ast = parse::parse_wiki(src);

    let mut opts = render::RenderOptions::default();
    opts.center_tables_and_captions = true;

    let md = render::render_doc_with_options(&ast.document, &opts);

    assert!(
        md.starts_with("<div style=\"display:flex; flex-direction:column; align-items:center;\">"),
        "expected centering wrapper, got:\n{md}"
    );
    assert!(md.contains("Caption"), "{}", md.to_string());
    assert!(md.contains("| H1 | H2 |"), "{}", md.to_string());
    assert!(md.ends_with("</div>"), "{}", md.to_string());
}
