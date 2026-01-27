pub mod ast;
pub mod frontmatter;
pub mod parse;
pub mod render;
pub mod wiki;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

/// Options controlling how Markdown files are written on disk.
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteOptions {
    /// If true, regenerate YAML frontmatter even when the destination `.md`
    /// already contains a frontmatter block.
    pub regenerate_frontmatter: bool,
}

/// Single file mode: Fetch if needed, then convert.
pub fn run(raw_title: &str, write_json: bool) -> Result<(), Box<dyn Error>> {
    run_with_options(
        raw_title,
        write_json,
        &render::RenderOptions::default(),
        &WriteOptions::default(),
    )
}

/// Single file mode: like [`run`], but allows callers to customize Markdown rendering.
pub fn run_with_render_options(
    raw_title: &str,
    write_json: bool,
    render_opts: &render::RenderOptions,
) -> Result<(), Box<dyn Error>> {
    run_with_options(raw_title, write_json, render_opts, &WriteOptions::default())
}

/// Single file mode: like [`run_with_render_options`], but also controls how
/// Markdown files are written (frontmatter preservation, etc.).
pub fn run_with_options(
    raw_title: &str,
    write_json: bool,
    render_opts: &render::RenderOptions,
    write_opts: &WriteOptions,
) -> Result<(), Box<dyn Error>> {
    let article_id = sanitize_article_id(raw_title);
    let bucket = lower_first_letter_bucket(&article_id);

    let wiki_dir = PathBuf::from("docs").join("wiki").join(&bucket);
    let json_dir = PathBuf::from("docs").join("json").join(&bucket);
    let md_dir = PathBuf::from("docs").join("md").join(&bucket);

    // ensure directories exist
    fs::create_dir_all(&wiki_dir)?;
    fs::create_dir_all(&md_dir)?;

    if write_json {
        fs::create_dir_all(&json_dir)?;
    }

    let wiki_path = wiki_dir.join(format!("{}.wiki", article_id));
    let json_path = json_dir.join(format!("{}.json", article_id));
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

    // parse wikitext into ast
    let ast = parse_file(&wiki_path)?;

    match write_json {
        true => {
            // write .json
            write_json_ast_for_wiki(&article_id, &wiki_path, &ast, &json_path)?;

            // write .md
            let md_content = render_markdown_from_json(
                &article_id,
                &wiki_path,
                &json_path,
                &md_path,
                render_opts,
                write_opts,
            )?;
            println!("{}", md_content);
        }
        false => {
            let md_body = render::render_doc_with_options(&ast.document, render_opts);
            let md_content = write_markdown_file(
                &md_path,
                &wiki_path,
                &article_id,
                &ast.document,
                &md_body,
                write_opts,
                render_opts,
            )?;
            println!("{}", md_content);
        }
    }

    Ok(())
}

/// Bulk mode: Walk ./docs/wiki and regenerate all corresponding .md files.
pub fn regenerate_all() -> Result<(), Box<dyn Error>> {
    regenerate_all_with_options(&render::RenderOptions::default(), &WriteOptions::default())
}

/// Bulk mode: like [`regenerate_all`], but allows callers to customize Markdown rendering.
pub fn regenerate_all_with_render_options(
    render_opts: &render::RenderOptions,
) -> Result<(), Box<dyn Error>> {
    regenerate_all_with_options(render_opts, &WriteOptions::default())
}

/// Bulk mode: like [`regenerate_all_with_render_options`], but also controls how
/// Markdown files are written (frontmatter preservation, etc.).
pub fn regenerate_all_with_options(
    render_opts: &render::RenderOptions,
    write_opts: &WriteOptions,
) -> Result<(), Box<dyn Error>> {
    let wiki_root = PathBuf::from("docs").join("wiki");
    let md_root = PathBuf::from("docs").join("md");
    regenerate_all_in_dirs(&wiki_root, &md_root, render_opts, write_opts)
}

/// Bulk mode: Walk the provided wiki root directory and regenerate all corresponding Markdown files
/// under the provided md root directory.
pub fn regenerate_all_in_dirs(
    wiki_root: &Path,
    md_root: &Path,
    render_opts: &render::RenderOptions,
    write_opts: &WriteOptions,
) -> Result<(), Box<dyn Error>> {
    let start_time = Instant::now();

    if !wiki_root.exists() {
        return Err(format!("Wiki source directory not found: {}", wiki_root.display()).into());
    }

    let mut entries: Vec<_> = WalkDir::new(wiki_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "wiki")
        })
        .collect();

    entries.sort_by(|a, b| a.path().cmp(b.path()));

    let total = entries.len();
    let mut count = 0;

    for entry in entries {
        let path = entry.path();
        // determine relative path structure to maintain the same structure in the md/ directory.
        let relative = path.strip_prefix(wiki_root)?;

        let mut md_path = md_root.join(relative);
        md_path.set_extension("md");

        // ensure the parent and bucket directory exists for the target .md file
        if let Some(parent) = md_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let article_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();

        let ast = parse_file(path)?;
        let md_body = render::render_doc_with_options(&ast.document, render_opts);
        let _full_md = write_markdown_file(
            &md_path,
            path,
            &article_id,
            &ast.document,
            &md_body,
            write_opts,
            render_opts,
        )?;

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

fn parse_file(wiki_path: &Path) -> Result<parse::ParseOutput, Box<dyn Error>> {
    let bytes = fs::read(wiki_path)?;

    // if we ever encounter invalid UTF-8, fallback to lossy conversion
    let wiki_content = String::from_utf8(bytes.clone())
        .unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).to_string());

    Ok(parse::parse_wiki(&wiki_content))
}

fn write_json_ast_for_wiki(
    article_id: &str,
    wiki_path: &Path,
    parse_out: &parse::ParseOutput,
    json_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let ast_file = ast::AstFile {
        schema_version: ast::SCHEMA_VERSION,
        parser: ast::ParserInfo {
            name: ast::PARSER_NAME.to_string(),
            version: ast::PARSER_VERSION.to_string(),
        },
        span_encoding: ast::SpanEncoding::default(),
        article_id: article_id.to_string(),
        source: ast::SourceInfo {
            path: Some(wiki_path.to_string_lossy().to_string()),
            byte_len: parse_out.byte_len as u64,
        },
        diagnostics: parse_out.diagnostics.clone(),
        document: parse_out.document.clone(),
    };

    // prettify JSON so it's easy to inspect / diff.
    let json = serde_json::to_string_pretty(&ast_file)?;
    fs::write(json_path, json)?;
    Ok(())
}

fn render_markdown_from_json(
    article_id: &str,
    wiki_path: &Path,
    json_path: &Path,
    md_path: &Path,
    render_opts: &render::RenderOptions,
    write_opts: &WriteOptions,
) -> Result<String, Box<dyn Error>> {
    let json_text = fs::read_to_string(json_path)?;
    let ast_file: ast::AstFile = serde_json::from_str(&json_text)?;
    let md_body = render::render_doc_with_options(&ast_file.document, render_opts);
    let full = write_markdown_file(
        md_path,
        wiki_path,
        article_id,
        &ast_file.document,
        &md_body,
        write_opts,
        render_opts,
    )?;
    Ok(full)
}

fn write_markdown_file(
    md_path: &Path,
    wiki_path: &Path,
    article_id: &str,
    doc: &ast::Document,
    md_body: &str,
    write_opts: &WriteOptions,
    render_opts: &render::RenderOptions,
) -> Result<String, Box<dyn Error>> {
    let existing = if md_path.exists() {
        Some(fs::read_to_string(md_path)?)
    } else {
        None
    };

    let mut frontmatter_text: Option<String> = None;

    if let Some(existing_text) = existing.as_deref()
        && let Some((fm, _)) = frontmatter::split_yaml_frontmatter(existing_text)
        && !write_opts.regenerate_frontmatter
    {
        frontmatter_text = Some(fm);
    }

    if frontmatter_text.is_none() {
        let mut fm = frontmatter::build_frontmatter(
            article_id,
            wiki_path,
            doc,
            &render_opts.mediawiki_base_url,
        )?;

        // when explicitly regenerating frontmatter, preserve user-authored summary and any
        // unknown top-level YAML keys.
        if write_opts.regenerate_frontmatter
            && let Some(existing_text) = existing.as_deref()
        {
            frontmatter::merge_existing_frontmatter_for_regeneration(&mut fm, existing_text);
        }

        frontmatter_text = Some(fm.to_yaml_string());
    }

    let mut out = String::new();
    if let Some(fm) = frontmatter_text {
        out.push_str(&fm);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        // blank line after frontmatter for readability.
        out.push('\n');
    }

    // avoid leading blank lines in the body to keep output stable.
    let body = md_body.trim_start_matches(['\n', '\r']);
    out.push_str(body);

    fs::write(md_path, &out)?;
    Ok(out)
}

pub(crate) fn sanitize_article_id(raw_title: &str) -> String {
    let mut id = raw_title.trim().replace(' ', "_");
    id = id.replace(['/', '\\'], "_");
    if id.is_empty() {
        id = "Untitled".to_string();
    }
    id
}

pub(crate) fn lower_first_letter_bucket(article_id: &str) -> String {
    let first = article_id.chars().next().unwrap_or('x');
    first.to_lowercase().collect()
}
