use assert_cmd::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn prints_existing_md_from_cache() {
    let dir = tempdir().unwrap();

    // cache layout: ./docs/md/{lower_first_letter}/{article title}.md
    // (underscores from the article_id are converted to spaces)
    let md_path = dir
        .path()
        .join("docs")
        .join("md")
        .join("p")
        .join("Perft.md");
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

    // provide a .wiki cache so the tool does not try to hit the network.
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

    cmd.assert().success().stdout(
        predicate::str::starts_with("---\nwiki2md:\n")
            .and(predicate::str::contains("article_id: Test_Page"))
            .and(predicate::str::contains(
                "source_url: https://www.chessprogramming.org/Test_Page",
            ))
            .and(predicate::str::contains("aliases:\n  - \"Test Page\""))
            .and(predicate::str::contains("tags: []"))
            // file title heading
            .and(predicate::str::contains("# Test Page"))
            // headings from the AST are demoted (H1 -> H2)
            .and(predicate::str::contains("## Title"))
            // internal links use aliases; no need for referencing the physical file.
            .and(predicate::str::contains("See [[Other Page|link]]."))
            // ensure we do not emit the removed display_title field
            .and(predicate::str::contains("display_title:").not()),
    );

    // it should have written the .md cache.
    let md_path = dir
        .path()
        .join("docs")
        .join("md")
        .join("t")
        .join("Test Page.md");
    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.starts_with("---\nwiki2md:\n"), "{md}");
    assert!(md.contains("# Test Page"), "{md}");
    assert!(md.contains("## Title"), "{md}");
    assert!(md.contains("See [[Other Page|link]]."), "{md}");
}

#[test]
fn regenerate_frontmatter_flag_overwrites_existing_frontmatter() {
    let dir = tempdir().unwrap();

    // provide a .wiki cache.
    let wiki_path = dir
        .path()
        .join("docs")
        .join("wiki")
        .join("t")
        .join("Test_Page.wiki");
    fs::create_dir_all(wiki_path.parent().unwrap()).unwrap();
    fs::write(
        &wiki_path,
        "'''[[Main Page|Home]] * [[Level 1]] * Test Page'''\n\n[[Category:Thing 1]]\n",
    )
    .unwrap();

    // existing `.md` with frontmatter.
    let md_path = dir
        .path()
        .join("docs")
        .join("md")
        .join("t")
        .join("Test Page.md");
    fs::create_dir_all(md_path.parent().unwrap()).unwrap();
    fs::write(
        &md_path,
        "---\ncustom: 123\nsummary: \"keep\"\n---\n\nOLD BODY\n",
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("wiki2md");
    cmd.current_dir(dir.path())
        .arg("--regenerate-all")
        .arg("--regenerate-frontmatter");

    cmd.assert().success();

    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.starts_with("---\nwiki2md:\n"), "{md}");
    assert!(md.contains("summary: \"keep\""), "{md}");
}
