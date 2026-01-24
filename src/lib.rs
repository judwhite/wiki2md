pub mod convert;
pub mod wiki;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub fn run(raw_title: &str) -> Result<(), Box<dyn Error>> {
    let article_id = sanitize_article_id(raw_title);

    let bucket = lower_first_letter_bucket(&article_id);

    let wiki_dir = PathBuf::from("docs").join("wiki").join(&bucket);
    let md_dir = PathBuf::from("docs").join("md").join(&bucket);

    // Ensure cache directories exist (mkdir -p behavior).
    fs::create_dir_all(&wiki_dir)?;
    fs::create_dir_all(&md_dir)?;

    let wiki_path = wiki_dir.join(format!("{}.wiki", article_id));
    let md_path = md_dir.join(format!("{}.md", article_id));

    // 1) CHECK: Does ./docs/md/{bucket}/{article_id}.md exist?
    if md_path.exists() {
        let content = fs::read_to_string(&md_path)?;
        println!("{}", content);
        return Ok(());
    }

    // 2) CHECK: Does ./docs/wiki/{bucket}/{article_id}.wiki exist? Fetch if not.
    if !wiki_path.exists() {
        // Use the original title for fetching (MediaWiki accepts spaces or underscores,
        // but the raw title is what the user intended).
        wiki::fetch_and_save(raw_title.trim(), wiki_path.to_string_lossy().as_ref())?;
    }

    // 3) CONVERT: Read Wiki -> Convert -> Save MD -> Print
    let wiki_content = fs::read_to_string(&wiki_path)?;
    let md_content = convert::wiki_to_markdown(&wiki_content);

    fs::write(&md_path, &md_content)?;
    println!("{}", md_content);

    Ok(())
}

/// Produces a stable on-disk identifier used for filenames.
/// Currently this matches the original behavior (spaces -> underscores) with
/// a small amount of path safety.
fn sanitize_article_id(raw_title: &str) -> String {
    let mut id = raw_title.trim().replace(' ', "_");

    // Avoid accidental directory traversal or nested paths.
    id = id.replace('/', "_").replace('\\', "_");

    // Avoid empty ids.
    if id.is_empty() {
        id = "Untitled".to_string();
    }

    id
}

fn lower_first_letter_bucket(article_id: &str) -> String {
    let first = article_id.chars().next().unwrap_or('x');
    first.to_lowercase().collect()
}
