use regex::{Captures, Regex};

/// Converts MediaWiki markup to Markdown.
pub fn wiki_to_markdown(input: &str) -> String {
    let mut text = input.to_string();

    // code blocks
    let re_pre_open = Regex::new(r"(?i)<pre[^>]*>").unwrap();
    text = re_pre_open.replace_all(&text, "\n```c\n").to_string();
    let re_pre_close = Regex::new(r"(?i)</pre>").unwrap();
    text = re_pre_close.replace_all(&text, "\n```\n").to_string();

    // convert <span id> to <a name>
    // convert this early so the header regex below sees the <a name...>
    let re_span = Regex::new(r#"<span\s+id="([^"]+)"></span>"#).unwrap();
    text = re_span
        .replace_all(&text, |caps: &Captures| {
            format!(r#"<a name="{}"></a>"#, &caps[1])
        })
        .to_string();

    // headings
    let re_h4 = Regex::new(r"(?m)^\s*====(.+?)====\s*$").unwrap();
    text = re_h4.replace_all(&text, "\n##### $1\n").to_string();

    let re_h3 = Regex::new(r"(?m)^\s*===(.+?)===\s*$").unwrap();
    text = re_h3.replace_all(&text, "\n#### $1\n").to_string();

    let re_h2 = Regex::new(r"(?m)^\s*==(.+?)==\s*$").unwrap();
    text = re_h2.replace_all(&text, "\n### $1\n").to_string();

    let re_h1 = Regex::new(r"(?m)^\s*=(.+?)=\s*$").unwrap();
    text = re_h1.replace_all(&text, "\n## $1\n").to_string();

    // extract anchor from header
    // matches: `### <a name="..."></a> Title`
    // replaces with: `<a name="..."></a>\n### Title`
    let re_anchor = Regex::new(r#"(?m)^(#{2,6})\s*(<a name="[^"]+"></a>)\s*(.*)$"#).unwrap();
    text = re_anchor.replace_all(&text, "$2\n$1 $3").to_string();

    // wiki links: [[target|label]] or [[target]]
    let re_wiki_link = Regex::new(r"\[\[(?P<inner>.*?)]]").unwrap();
    text = re_wiki_link
        .replace_all(&text, |caps: &Captures| {
            let inner = &caps["inner"];
            let (target_raw, label_raw) = match inner.split_once('|') {
                Some((t, l)) => (t, l),
                None => (inner, inner),
            };
            let target = target_raw.trim().replace(" ", "_");
            let label = label_raw.trim();

            // point to the generated local Markdown file.
            // keep pure in-page anchors (e.g. [[#Section|label]]) unchanged.
            if target.starts_with('#') {
                return format!("[{}]({})", label, target);
            }

            let (page, anchor) = match target.split_once('#') {
                Some((p, a)) => (p, Some(a)),
                None => (target.as_str(), None),
            };

            let mut href = format!("{}.md", page);
            if let Some(a) = anchor {
                href.push('#');
                href.push_str(a);
            }

            format!("[{}]({})", label, href)
        })
        .to_string();

    // external Links
    let re_ext_link = Regex::new(r"\[(?P<url>https?://\S+)\s+(?P<label>[^]]+)]").unwrap();
    text = re_ext_link.replace_all(&text, "[$label]($url)").to_string();

    // bold
    let re_bold = Regex::new(r"'''(.*?)'''").unwrap();
    text = re_bold.replace_all(&text, "**$1**").to_string();

    // perft(#)
    let re_perft = Regex::new(r"(?i)(perft\(\d+\))").unwrap();
    text = re_perft.replace_all(&text, "`$1`").to_string();

    text = text.replace("``perft", "`perft");
    text = text.replace(")``", ")`");

    // remove <br/>
    let re_br = Regex::new(r"(?i)<\s*br\s*/?\s*>").unwrap();
    text = re_br.replace_all(&text, "\n").to_string();

    // fix space before punctuation
    let re_space_after_close_tag = Regex::new(r">\s+([.:,;])").unwrap();
    text = re_space_after_close_tag
        .replace_all(&text, ">$1")
        .to_string();

    // blockquotes
    let re_quote = Regex::new(r"(?m)^ (?P<content>[^ ].*)$").unwrap();
    text = re_quote.replace_all(&text, "> $content").to_string();

    // blockquotes line spacing
    text = process_quote_spacing(&text);

    // remove superfluous newlines
    text = text.replace("\n\n\n", "\n\n");
    text = text.replace("\n\n\n", "\n\n");
    text = text.replace("```c\n\n", "```c\n");
    text = text.replace("\n\n```\n", "\n```\n");

    text = text.replace("''", "\"");

    // change breadcrumbs to top-level heading
    let re_nav = Regex::new(r"\*\*\[Home]\(Main_Page\).* \* (.+)\*\*").unwrap();
    text = re_nav.replace_all(&text, "# $1").to_string();

    text.trim().to_string()
}

fn process_quote_spacing(input: &str) -> String {
    let mut new_text = String::with_capacity(input.len());
    let mut was_in_quote = false;

    for line in input.lines() {
        let is_quote = line.starts_with("> ");

        if is_quote {
            if !was_in_quote {
                if !new_text.is_empty() && !new_text.ends_with("\n\n") {
                    if new_text.ends_with('\n') {
                        new_text.push('\n');
                    } else {
                        new_text.push_str("\n\n");
                    }
                }
            } else {
                new_text.push_str(">\n");
            }
        } else if was_in_quote && !line.trim().is_empty() && !new_text.ends_with("\n\n") {
            if new_text.ends_with('\n') {
                new_text.push('\n');
            } else {
                new_text.push_str("\n\n");
            }
        }

        new_text.push_str(line);
        new_text.push('\n');
        was_in_quote = is_quote;
    }

    new_text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_anchor_extraction() {
        let input = r#"==<span id="Bulk"></span>Bulk-counting=="#;
        let output = wiki_to_markdown(input);
        let expected = r#"<a name="Bulk"></a>
### Bulk-counting"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_wiki_links() {
        let input = "See [[Initial Position|initial position]] and [[C]].";
        let output = wiki_to_markdown(input);
        let expected = "See [initial position](Initial_Position.md) and [C](C.md).";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_headers() {
        let input = "=Perft Function=\n==Bulk Counting==";
        let output = wiki_to_markdown(input);
        let expected = "## Perft Function\n\n### Bulk Counting";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_span_to_anchor() {
        let input = r#"Header <span id="Bulk"></span>"#;
        let output = wiki_to_markdown(input);
        let expected = r#"Header <a name="Bulk"></a>"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_code_block_formatting() {
        let input = "<pre>int main() { return 0; }</pre>";
        let output = wiki_to_markdown(input);
        let expected = "```c\nint main() { return 0; }\n```";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_code_block_with_attributes_and_case() {
        let input = r#"<PRE class="code">int x = 1;</PRE>"#;
        let output = wiki_to_markdown(input);
        let expected = "```c\nint x = 1;\n```";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_deeper_headings() {
        let input = "===Level 3===\n====Level 4====";
        let output = wiki_to_markdown(input);
        let expected = "#### Level 3\n\n##### Level 4";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_wiki_link_with_anchor() {
        let input = "See [[Perft Results#Initial Position|init]].";
        let output = wiki_to_markdown(input);
        let expected = "See [init](Perft_Results.md#Initial_Position).";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_quote_spacing_logic() {
        let input = r#"
Preceding Text
 I believe...
 I carried...
Following Text
"#;

        let output = wiki_to_markdown(input);

        let expected = r#"Preceding Text

> I believe...
>
> I carried...

Following Text"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_blockquotes_vs_code_indentation() {
        let input = r#"
 I am a quote.
```c
  if (depth == 0)
    return 1;

```

"#;
        let output = wiki_to_markdown(input);
        let expected = r#"> I am a quote.

```c
  if (depth == 0)
    return 1;
```"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_formatting_and_perft() {
        let input = "The '''perft(5)''' result is `perft(6)`";
        let output = wiki_to_markdown(input);
        let expected = "The **`perft(5)`** result is `perft(6)`";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_punctuation_cleanup() {
        let input = "</ref> . <ref> , </ref> ;";
        let output = wiki_to_markdown(input);
        let expected = "</ref>. <ref>, </ref>;";

        assert_eq!(output, expected);
    }
}
