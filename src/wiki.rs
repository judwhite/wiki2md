use reqwest::Url;
use scraper::{Html, Selector};
use std::error::Error;
use std::fs;

fn build_edit_url(title: &str) -> Result<Url, Box<dyn Error>> {
    let mut url = Url::parse("https://www.chessprogramming.org/index.php")?;
    url.query_pairs_mut()
        .append_pair("title", title)
        .append_pair("action", "edit");
    Ok(url)
}

fn extract_wiki_text_from_edit_html(html_body: &str) -> Result<String, Box<dyn Error>> {
    let document = Html::parse_document(html_body);

    // MediaWiki edit pages typically keep the article content in a textarea with this id.
    // fall back to the first textarea if the structure changes.
    let selector_primary = Selector::parse("textarea#wpTextbox1")?;
    let selector_fallback = Selector::parse("textarea")?;

    let textarea = document
        .select(&selector_primary)
        .next()
        .or_else(|| document.select(&selector_fallback).next())
        .ok_or("Could not find <textarea> element. Is the page valid?")?;

    // for <textarea>, the content is HTML-escaped in the response.
    let textarea_content = textarea.inner_html();
    Ok(html_escape::decode_html_entities(&textarea_content).to_string())
}

/// Fetches the raw Wiki markup from the Edit page and saves it to a file.
pub fn fetch_and_save(title: &str, filename: &str) -> Result<(), Box<dyn Error>> {
    let url = build_edit_url(title)?;

    let resp = reqwest::blocking::get(url.clone())?;

    if !resp.status().is_success() {
        return Err(format!("Request failed: {} (URL: {})", resp.status(), url).into());
    }

    let html_body = resp.text()?;
    let decoded_wiki = extract_wiki_text_from_edit_html(&html_body)?;

    fs::write(filename, decoded_wiki)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_edit_url_encodes_title_and_sets_action() {
        let url = build_edit_url("C++ and Friends").unwrap();
        let pairs: std::collections::HashMap<String, String> =
            url.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("title").unwrap(), "C++ and Friends");
        assert_eq!(pairs.get("action").unwrap(), "edit");
    }

    #[test]
    fn extract_prefers_wp_textbox_1_and_decodes_entities() {
        let html = r#"
            <html>
              <body>
                <textarea id="other">ignore me</textarea>
                <textarea id="wpTextbox1">Line1 &amp; Line2 &lt;tag&gt;</textarea>
              </body>
            </html>
        "#;

        let out = extract_wiki_text_from_edit_html(html).unwrap();
        assert_eq!(out, "Line1 & Line2 <tag>");
    }
}
