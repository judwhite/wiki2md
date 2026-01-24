pub mod convert;
pub mod ast;
pub mod wiki;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

/// Single file mode: Fetch if needed, then convert.
pub fn run(raw_title: &str) -> Result<(), Box<dyn Error>> {
    let article_id = sanitize_article_id(raw_title);
    let bucket = lower_first_letter_bucket(&article_id);

    let wiki_dir = PathBuf::from("docs").join("wiki").join(&bucket);
    let md_dir = PathBuf::from("docs").join("md").join(&bucket);

    // Ensure directories exist
    fs::create_dir_all(&wiki_dir)?;
    fs::create_dir_all(&md_dir)?;

    let wiki_path = wiki_dir.join(format!("{}.wiki", article_id));
    let md_path = md_dir.join(format!("{}.md", article_id));

    // does ./docs/md/{bucket}/{article_id}.md exist?
    if md_path.exists() {
        let content = fs::read_to_string(&md_path)?;
        println!("{}", content);
        return Ok(());
    }

    // does ./docs/wiki/{bucket}/{article_id}.wiki exist? fetch if not.
    if !wiki_path.exists() {
        wiki::fetch_and_save(raw_title.trim(), wiki_path.to_string_lossy().as_ref())?;
    }

    // read wikitext -> convert -> save .md
    let md_content = convert_and_save(&wiki_path, &md_path)?;
    println!("{}", md_content);

    Ok(())
}

/// Bulk mode: Walk ./docs/wiki and regenerate all corresponding .md files.
pub fn regenerate_all() -> Result<(), Box<dyn Error>> {
    let start_time = Instant::now();
    let wiki_root = PathBuf::from("docs").join("wiki");

    if !wiki_root.exists() {
        return Err(format!("Wiki source directory not found: {}", wiki_root.display()).into());
    }

    let mut count = 0;
    let mut entries: Vec<_> = WalkDir::new(&wiki_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by_key(|e| e.path().to_owned());
    let total = entries.len();

    for entry in entries {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "wiki") {
            // determine relative path structure to maintain the same structure in the md/ directory.
            let relative = path.strip_prefix(&wiki_root)?;

            // construct target path: docs/md/<relative_with_md_extension>
            let md_root = PathBuf::from("docs").join("md");
            let mut md_path = md_root.join(relative);
            md_path.set_extension("md");

            // ensure the parent and bucket directory exists for the target md file
            let bucket_dir = md_path.parent().unwrap();
            fs::create_dir_all(bucket_dir)?;

            match convert_and_save(path, &md_path) {
                Ok(_) => {
                    count += 1;
                    let elapsed = start_time.elapsed();
                    let total_ms = elapsed.as_millis();
                    let mins = total_ms / 60_000;
                    let secs = (total_ms % 60_000) / 1_000;
                    let ms = total_ms % 1_000;
                    eprintln!(
                        "[{:>4}/{:>4}] [{:02}:{:02}.{:03}] Regenerated: {:?}",
                        count, total, mins, secs, ms, md_path
                    );
                }
                Err(e) => eprintln!("Failed to process {:?}: {}", path, e),
            }
        }
    }

    let total_elapsed = start_time.elapsed();
    let total_secs = total_elapsed.as_secs_f64();
    let avg_str = if count > 0 {
        format!("{:.3}s", total_secs / count as f64)
    } else {
        "-".to_string()
    };

    eprintln!(
        "Done. Regenerated {} files in {:.3}s (avg {}/doc).",
        count, total_secs, avg_str
    );
    Ok(())
}

fn convert_and_save(wiki_path: &Path, md_path: &Path) -> Result<String, Box<dyn Error>> {
    let wiki_content = fs::read_to_string(wiki_path)?;
    let md_content = convert::wiki_to_markdown(&wiki_content);
    fs::write(md_path, &md_content)?;
    Ok(md_content)
}

fn sanitize_article_id(raw_title: &str) -> String {
    let mut id = raw_title.trim().replace(' ', "_");
    id = id.replace(['/', '\\'], "_");
    if id.is_empty() {
        id = "Untitled".to_string();
    }
    id
}

fn lower_first_letter_bucket(article_id: &str) -> String {
    let first = article_id.chars().next().unwrap_or('x');
    first.to_lowercase().collect()
}
