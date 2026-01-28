use std::fs;

use tempfile::tempdir;

use wiki2md::frontmatter::{normalize_tag, split_yaml_frontmatter};
use wiki2md::render::RenderOptions;
use wiki2md::{WriteOptions, regenerate_all_in_dirs};

fn is_yyyy_mm_dd(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

#[test]
fn normalize_tag_rules() {
    // lowercase, allowed charset, starts with a letter
    let samples = [
        normalize_tag("Level 1").unwrap(),
        normalize_tag("Alpha/Beta").unwrap(),
        normalize_tag("1984").unwrap(),
        normalize_tag("42").unwrap(),
        normalize_tag("  123abc").unwrap(),
    ];

    for t in &samples {
        assert!(!t.is_empty());
        assert!(t.len() <= 50, "tag too long: {t}");
        assert!(
            t.chars().next().unwrap().is_ascii_lowercase(),
            "bad start: {t}"
        );
        assert!(
            t.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'),
            "bad chars: {t}"
        );
        assert!(!t.chars().all(|c| c.is_ascii_digit()), "fully numeric: {t}");
        assert!(!t.contains('/'), "nested tag not allowed: {t}");
    }

    assert_eq!(normalize_tag("Level 1"), Some("level_1".to_string()));
    assert_eq!(normalize_tag("Alpha/Beta"), Some("alpha_beta".to_string()));

    // diacritics / latin-script transliteration
    assert_eq!(
        normalize_tag("Salvador Dalí"),
        Some("salvador_dali".to_string())
    );
    assert_eq!(normalize_tag("François"), Some("francois".to_string()));
    assert_eq!(normalize_tag("Gödel"), Some("godel".to_string()));
    assert_eq!(normalize_tag("Łódź"), Some("lodz".to_string()));
    assert_eq!(normalize_tag("Straße"), Some("strasse".to_string()));
    assert_eq!(normalize_tag("Smørrebrød"), Some("smorrebrod".to_string()));

    // fully numeric must not remain numeric
    assert_eq!(normalize_tag("1984"), Some("y1984".to_string()));
    assert_eq!(normalize_tag("42"), Some("n42".to_string()));

    // enforce max length
    let long = "a".repeat(200);
    let t = normalize_tag(&long).unwrap();
    assert!(t.len() <= 50);

    // never produce slash nesting (we'll handle nesting later)
    assert!(!normalize_tag("x/y").unwrap().contains('/'));
}

#[test]
fn generates_frontmatter_when_missing_and_extracts_tags() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let wiki_path = root
        .join("docs")
        .join("wiki")
        .join("b")
        .join("Barend_Swets.wiki");
    fs::create_dir_all(wiki_path.parent().unwrap()).unwrap();
    fs::write(
        &wiki_path,
        "'''[[Main Page|Home]] * [[Level 1]] * [[Level 2]] * Barend Swets'''\n\n".to_string()
            + "Some body.\n\n"
            + "'''[[Level 2|Up one level]]'''\n"
            + "[[Category:Thing 1]]\n"
            + "[[Category:Thing 2]]\n",
    )
    .unwrap();

    let wiki_root = root.join("docs").join("wiki");
    let md_root = root.join("docs").join("md");
    regenerate_all_in_dirs(
        &wiki_root,
        &md_root,
        &RenderOptions::default(),
        &WriteOptions::default(),
    )
    .unwrap();

    let md_path = md_root.join("b").join("Barend Swets.md");
    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.starts_with("---\nwiki2md:\n"), "{md}");
    assert!(md.contains("article_id: Barend_Swets"), "{md}");
    assert!(
        md.contains("source_url: https://www.chessprogramming.org/Barend_Swets"),
        "{md}"
    );
    assert!(md.contains("generated_by: wiki2md"), "{md}");
    assert!(md.contains("schema_version: 1"), "{md}");
    assert!(md.contains("aliases:\n  - \"Barend Swets\""), "{md}");
    assert!(!md.contains("display_title:"), "{md}");

    // do not emit `summary` unless it was already present and preserved.
    assert!(!md.contains("\nsummary:"), "{md}");

    // tags extracted from breadcrumb + bottom categories
    assert!(md.contains("- level_1"), "{md}");
    assert!(md.contains("- level_2"), "{md}");
    assert!(md.contains("- thing_1"), "{md}");
    assert!(md.contains("- thing_2"), "{md}");

    // last_fetched_date exists and is date-ish
    let (_, body) = split_yaml_frontmatter(&md).expect("frontmatter");
    assert!(body.contains("# Barend Swets\n"), "{md}");
    assert!(body.contains("Some body."), "{md}");

    // internal links must use wikilinks style and aliases instead of linking to physical files
    assert!(body.contains("[[Level 1]]"), "{md}");
    assert!(!body.contains("](../"), "{md}");
    let date_line = md
        .lines()
        .find(|l| l.trim_start().starts_with("last_fetched_date:"))
        .expect("last_fetched_date line");
    let date = date_line.split(':').nth(1).unwrap().trim();
    assert!(is_yyyy_mm_dd(date), "bad date: {date}");
}

#[test]
fn preserves_existing_frontmatter_verbatim_by_default() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let wiki_path = root
        .join("docs")
        .join("wiki")
        .join("t")
        .join("Test_Page.wiki");
    fs::create_dir_all(wiki_path.parent().unwrap()).unwrap();
    fs::write(&wiki_path, "=Title=\nBody\n[[Category:Thing 1]]\n").unwrap();

    let md_path = root.join("docs").join("md").join("t").join("Test Page.md");
    fs::create_dir_all(md_path.parent().unwrap()).unwrap();
    let existing_fm = "---\ncustom: 123\nsummary: \"keep me\"\n---\n";
    fs::write(&md_path, format!("{}\nOLD BODY\n", existing_fm)).unwrap();

    let wiki_root = root.join("docs").join("wiki");
    let md_root = root.join("docs").join("md");
    regenerate_all_in_dirs(
        &wiki_root,
        &md_root,
        &RenderOptions::default(),
        &WriteOptions::default(),
    )
    .unwrap();

    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.starts_with(existing_fm), "{md}");
    assert!(!md.contains("OLD BODY"), "{md}");
}

#[test]
fn regenerate_frontmatter_flag_regenerates_but_preserves_summary_and_extras() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let wiki_path = root
        .join("docs")
        .join("wiki")
        .join("t")
        .join("Test_Page.wiki");
    fs::create_dir_all(wiki_path.parent().unwrap()).unwrap();
    fs::write(
        &wiki_path,
        "'''[[Main Page|Home]] * [[Level 1]] * Test Page'''\n\n[[Category:1984]]\n",
    )
    .unwrap();

    let md_path = root.join("docs").join("md").join("t").join("Test Page.md");
    fs::create_dir_all(md_path.parent().unwrap()).unwrap();
    fs::write(
        &md_path,
        "---\ncustom: 123\ncssclass: foo\nsummary: \"keep\"\n---\n\nOLD BODY\n",
    )
    .unwrap();

    let write_opts = WriteOptions {
        regenerate_frontmatter: true,
    };
    let wiki_root = root.join("docs").join("wiki");
    let md_root = root.join("docs").join("md");
    regenerate_all_in_dirs(&wiki_root, &md_root, &RenderOptions::default(), &write_opts).unwrap();

    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.starts_with("---\nwiki2md:\n"), "{md}");
    // preserve summary
    assert!(md.contains("summary: \"keep\""), "{md}");
    // preserve extra keys
    assert!(md.contains("cssclass:"), "{md}");
    // numeric tag normalization
    assert!(md.contains("- y1984"), "{md}");
}
