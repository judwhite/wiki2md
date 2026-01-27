use clap::Parser;
use wiki2md::render::RenderOptions;
use wiki2md::{WriteOptions, regenerate_all_with_options, run_with_options};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The title of the page (e.g., "Perft" or "Move Generation").
    /// Required unless --regenerate-all is used.
    #[arg(required_unless_present = "regenerate_all")]
    title: Option<String>,

    /// Regenerate all .md files from existing .wiki files in ./docs/wiki
    #[arg(long, short = 'r')]
    regenerate_all: bool,

    /// Center wikitable captions and tables using an HTML wrapper.
    #[arg(long, default_value_t = false)]
    center_tables: bool,

    /// Regenerate YAML frontmatter during regeneration.
    #[arg(long, default_value_t = false)]
    regenerate_frontmatter: bool,
}

fn main() {
    let args = Cli::parse();

    let render_opts = RenderOptions {
        center_tables_and_captions: args.center_tables,
        ..Default::default()
    };

    let write_opts = WriteOptions {
        regenerate_frontmatter: args.regenerate_frontmatter,
    };

    if args.regenerate_all {
        if let Err(e) = regenerate_all_with_options(&render_opts, &write_opts) {
            eprintln!("Error regenerating all files: {}", e);
            std::process::exit(1);
        }
    } else {
        let title = args.title.as_ref().unwrap();
        if let Err(e) = run_with_options(title, false, &render_opts, &write_opts) {
            eprintln!("Error processing '{}': {}", title, e);
            std::process::exit(1);
        }
    }
}
