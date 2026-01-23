use regex::{Captures, Regex};
use std::fs;
use std::io::{self};

fn main() -> io::Result<()> {
    let content = fs::read_to_string("perft.wiki").unwrap_or_else(|_| {
        String::from("=Perft=\n<pre>Code example</pre>")
    });

    let converted = convert_wiki_to_markdown(&content);
    println!("{}", converted);
    Ok(())
}

fn convert_wiki_to_markdown(input: &str) -> String {
    let mut text = input.to_string();

    // code blocks
    text = text.replace("<pre>", "\n```c\n");
    text = text.replace("</pre>", "\n```\n");

    // convert <span id> to <a name>
    // convert this early so the header regex below sees the <a name...>
    let re_span = Regex::new(r#"<span\s+id="([^"]+)"></span>"#).unwrap();
    text = re_span.replace_all(&text, |caps: &Captures| {
        format!(r#"<a name="{}"></a>"#, &caps[1])
    }).to_string();

    // heading level 3
    let re_h2 = Regex::new(r"(?m)^\s*==(.+?)==\s*$").unwrap();
    text = re_h2.replace_all(&text, "\n### $1\n").to_string();

    // heading level 2
    let re_h1 = Regex::new(r"(?m)^=(.+?)=\s*$").unwrap();
    text = re_h1.replace_all(&text, "\n## $1\n").to_string();

    // extract anchor from header
    // matches: `### <a name="..."></a> Title`
    // replaces with: `<a name="..."></a>\n### Title`
    let re_extract = Regex::new(r#"(?m)^(#{2,3})\s*(<a name="[^"]+"></a>)\s*(.*)$"#).unwrap();
    text = re_extract.replace_all(&text, "$2\n$1 $3").to_string();

    // wiki links: [[target|label]] or [[target]]
    let re_wiki_link = Regex::new(r"\[\[(?P<inner>.*?)]]").unwrap();
    text = re_wiki_link.replace_all(&text, |caps: &Captures| {
        let inner = &caps["inner"];
        let (target_raw, label_raw) = match inner.split_once('|') {
            Some((t, l)) => (t, l),
            None => (inner, inner),
        };
        let target = target_raw.trim().replace(" ", "_");
        let label = label_raw.trim();
        format!("[{}]({})", label, target)
    }).to_string();

    // external Links
    let re_ext_link = Regex::new(r"\[(?P<url>https?://\S+)\s+(?P<label>[^]]+)]").unwrap();
    text = re_ext_link.replace_all(&text, "[$label]($url)").to_string();

    // change ''' to ** (bold)
    let re_bold = Regex::new(r"'''(.*?)'''").unwrap();
    text = re_bold.replace_all(&text, "**$1**").to_string();

    // change instances of "perft(d+)" to "`perft(d+)`".
    let re_perft = Regex::new(r"(?i)(perft\(\d+\))").unwrap();
    text = re_perft.replace_all(&text, "`$1`").to_string();

    // fix double backticks around perft specifically
    text = text.replace("``perft", "`perft");
    text = text.replace(")``", ")`");

    // remove <br/>
    let re_br = Regex::new(r"(?i)<\s*br\s*/?\s*>").unwrap();
    text = re_br.replace_all(&text, "\n").to_string();

    // find: `>` followed by whitespace `\s+` followed by punctuation `[.:,;]`
    // We capture ONLY the punctuation in group 1 ($1)
    let re_space_after_close_tag = Regex::new(r">\s+([.:,;])").unwrap();
    text = re_space_after_close_tag.replace_all(&text, ">$1").to_string();

    // blockquotes
    // match lines starting with a single space, followed immediately by a non-space character.
    // this captures " I believe..." but ignores "  if (depth == 0)"
    let re_quote = Regex::new(r"(?m)^ (?P<content>[^ ].*)$").unwrap();
    text = re_quote.replace_all(&text, "> $content").to_string();

    // blockquotes line spacing
    text = process_quote_spacing(&text);

    // remove superfluous newlines
    text = text.replace("\n\n\n", "\n\n");
    text = text.replace("\n\n\n", "\n\n"); // Duplicate call to catch odd overlaps
    text = text.replace("```c\n\n", "```c\n");
    text = text.replace("\n\n```\n", "\n```\n");

    // change '' to "
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
                // Rule 1: Entering a quote block. Ensure a blank line before.
                if !new_text.is_empty() && !new_text.ends_with("\n\n") {
                    if new_text.ends_with('\n') {
                        new_text.push('\n');
                    } else {
                        new_text.push_str("\n\n");
                    }
                }
            } else {
                // Rule 2: Inside a quote block. Insert spacer between quote lines.
                new_text.push_str(">\n");
            }
        } else {
            // Rule 3: Leaving a quote block. Ensure a blank line after.
            if was_in_quote && !line.trim().is_empty() {
                if !new_text.ends_with("\n\n") {
                    if new_text.ends_with('\n') {
                        new_text.push('\n');
                    } else {
                        new_text.push_str("\n\n");
                    }
                }
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
    fn test_wiki_links() {
        let input = "See [[Initial Position|initial position]] and [[C]].";
        let output = convert_wiki_to_markdown(input);
        assert_eq!(output, "See [initial position](Initial_Position) and [C](C).");
    }

    #[test]
    fn test_headers() {
        let input = "=Perft Function=\n==Bulk Counting==";
        let output = convert_wiki_to_markdown(input);
        assert_eq!(output, "## Perft Function\n\n### Bulk Counting");
    }
    
    #[test]
    fn test_span_to_anchor() {
        let input = r#"Header <span id="Bulk"></span>"#;
        let output = convert_wiki_to_markdown(input);
        assert_eq!(output, r#"Header <a name="Bulk"></a>"#);
    }

    #[test]
    fn test_code_block_formatting() {
        let input = "<pre>int main() { return 0; }</pre>";
        let output = convert_wiki_to_markdown(input);

        // We expect a valid Markdown code block with 3 backticks
        assert!(output.contains("```"), "Output code block fence is broken: found '{}'", output.trim());
    }

    #[test]
    fn test_quote_spacing_logic() {
        // Input has:
        // 1. Text immediately touching the quote (Rule 1 violation)
        // 2. Two quotes touching each other (Rule 2 violation)
        // 3. Text immediately touching the end of a quote (Rule 3 violation)
        let input = r#"
Preceding Text
 I believe...
 I carried...
Following Text
"#;

        let output = convert_wiki_to_markdown(input);

        let expected_fragment = r#"
Preceding Text

> I believe...
>
> I carried...

Following Text
"#;

        // We trim both to ignore file-start/end whitespace differences
        assert!(output.contains(expected_fragment.trim()),
                "Spacing logic failed! Output:\n{}", output);
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
        let output = convert_wiki_to_markdown(input);

        // 1. Check that the quote was converted
        assert!(output.contains("> I am a quote"), "Failed to convert single-space indent to quote.");

        // 2. CRITICAL: Check that the code was NOT converted
        // It should still look like "  if (depth..." not "> if (depth..."
        assert!(output.contains("  if (depth == 0)"), "Code indentation was corrupted!");
        assert!(!output.contains("> if (depth == 0)"), "Code was incorrectly turned into a quote!");
    }

    #[test]
    fn test_header_anchor_extraction() {
        // Input: Wiki style where anchor is inside the equals signs
        let input = r#"==<span id="Bulk"></span>Bulk-counting=="#;

        let output = convert_wiki_to_markdown(input);

        // Expected: Anchor moved to its own line BEFORE the header
        let expected = r#"<a name="Bulk"></a>
### Bulk-counting"#;

        assert_eq!(output.trim(), expected, "Failed to extract and move anchor tag from header.");
    }

    #[test]
    fn test_formatting_and_perft() {
        let input = "The '''perft(5)''' result is `perft(6)`";
        let output = convert_wiki_to_markdown(input);

        // 1. ''' -> **
        // 2. perft(5) -> `perft(5)`
        // 3. `perft(6)` -> `perft(6)` (No double backticks like ``perft(6)``)
        assert_eq!(output, "The **`perft(5)`** result is `perft(6)`");
    }

    #[test]
    fn test_punctuation_cleanup() {
        // Wiki often leaves a space before punctuation in generated content
        let input = "Link > . Next > , End > ;";
        let output = convert_wiki_to_markdown(input);

        assert_eq!(output, "Link >. Next >, End >;", "Failed to remove whitespace before punctuation.");
    }
}
