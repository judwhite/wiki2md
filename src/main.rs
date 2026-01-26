use clap::Parser;
use wiki2md::{regenerate_all, run};

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
}

fn main() {
    let args = Cli::parse();

    if args.regenerate_all {
        if let Err(e) = regenerate_all() {
            eprintln!("Error regenerating all files: {}", e);
            std::process::exit(1);
        }
    } else {
        let title = args.title.as_ref().unwrap();
        if let Err(e) = run(title, false) {
            eprintln!("Error processing '{}': {}", title, e);
            std::process::exit(1);
        }
    }
}
