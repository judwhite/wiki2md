use assert_cmd::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn prints_existing_md_from_cache() {
    let dir = tempdir().unwrap();

    // Cache layout: ./docs/md/{lower_first_letter}/{article_id}.md
    let md_path = dir.path().join("docs").join("md").join("p").join("Perft.md");
    fs::create_dir_all(md_path.parent().unwrap()).unwrap();
    fs::write(&md_path, "cached markdown").unwrap();

    let mut cmd = cargo_bin_cmd!("wiki2md");
    cmd.current_dir(dir.path()).arg("Perft");

    // println! adds a trailing newline.
    cmd.assert()
        .success()
        .stdout(predicate::eq("cached markdown\n"));
}

#[test]
fn generates_md_from_existing_wiki_cache() {
    let dir = tempdir().unwrap();

    // Provide a .wiki cache so the tool does not try to hit the network.
    let wiki_path = dir
        .path()
        .join("docs")
        .join("wiki")
        .join("t")
        .join("Test_Page.wiki");
    fs::create_dir_all(wiki_path.parent().unwrap()).unwrap();
    fs::write(&wiki_path, "=Title=\nSee [[Other Page|link]].\n").unwrap();

    let mut cmd = cargo_bin_cmd!("wiki2md");
    cmd.current_dir(dir.path()).arg("Test Page");

    cmd.assert()
        .success()
        .stdout(predicate::eq("## Title\n\nSee [link](../o/Other_Page.md).\n"));

    // It should have written the markdown cache.
    let md_path = dir
        .path()
        .join("docs")
        .join("md")
        .join("t")
        .join("Test_Page.md");
    let md = fs::read_to_string(&md_path).unwrap();
    assert_eq!(md, "## Title\n\nSee [link](../o/Other_Page.md).");
}
