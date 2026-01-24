use clap::Parser;
use wiki2md::run;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The title of the page (e.g., "Perft" or "Move Generation")
    title: String,
}

fn main() {
    let args = Cli::parse();

    if let Err(e) = run(&args.title) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
