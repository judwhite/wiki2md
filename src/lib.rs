pub mod convert;
pub mod wiki;

use std::error::Error;
use std::fs;
use std::path::Path;

pub fn run(raw_title: &str) -> Result<(), Box<dyn Error>> {
    // Sanitize the title for filenames (e.g. "Move Generation" -> "Move_Generation")
    let clean_title = raw_title.trim().replace(" ", "_");

    let md_filename = format!("{}.md", clean_title);
    let wiki_filename = format!("{}.wiki", clean_title);

    // 1. CHECK: Does {title}.md exist?
    if Path::new(&md_filename).exists() {
        let content = fs::read_to_string(&md_filename)?;
        println!("{}", content);
        return Ok(());
    }

    // 2. CHECK: Does {title}.wiki exist? Fetch if not.
    if !Path::new(&wiki_filename).exists() {
        eprintln!("Cache miss for '{}'.", clean_title);
        wiki::fetch_and_save(&clean_title, &wiki_filename)?;
    }

    // 3. CONVERT: Read Wiki -> Convert -> Save MD -> Print
    let wiki_content = fs::read_to_string(&wiki_filename)?;
    let md_content = convert::wiki_to_markdown(&wiki_content);

    fs::write(&md_filename, &md_content)?;
    println!("{}", md_content);

    Ok(())
}
