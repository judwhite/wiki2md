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
    if !actual_md.eq(&want_md)  {
        fs::write(&out_path, &actual_md).unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));
    } else {
        if fs::exists(&out_path).is_ok().eq(&true) {
            fs::remove_file(&out_path).unwrap_or_else(|e| panic!("failed to remove {}: {e}", out_path.display()));
        }
    }

    assert_eq!(actual_md, want_md);
}
