use scraper::{Html, Selector};
use std::error::Error;
use std::fs;

/// Fetches the raw Wiki markup from the Edit page and saves it to a file.
pub fn fetch_and_save(title: &str, filename: &str) -> Result<(), Box<dyn Error>> {
    // Construct URL
    let url = format!(
        "https://www.chessprogramming.org/index.php?title={}&action=edit",
        title
    );

    println!("Fetching from: {}", url);
    let resp = reqwest::blocking::get(&url)?;

    if !resp.status().is_success() {
        return Err(format!("Request failed: {} (URL: {})", resp.status(), url).into());
    }

    let html_body = resp.text()?;

    // Parse HTML to find <textarea>
    let document = Html::parse_document(&html_body);
    let selector = Selector::parse("textarea").unwrap();

    let textarea_content = document
        .select(&selector)
        .next()
        .ok_or("Could not find <textarea> element. Is the page valid?")?
        .inner_html();

    // Decode HTML entities (e.g. &lt; -> <)
    let decoded_wiki = html_escape::decode_html_entities(&textarea_content).to_string();

    fs::write(filename, decoded_wiki)?;
    println!("Saved raw wiki data to {}", filename);

    Ok(())
}
