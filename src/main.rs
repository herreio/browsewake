use clap::Parser;
use std::fs::File;
use std::io::{self, Write};

use browsewake::model::BrowserKind;
use browsewake::output::{self, Format};

#[derive(Parser)]
#[command(
    name = "browsewake",
    about = "Export browser tabs and per-tab navigation history"
)]
struct Cli {
    /// Browsers to export (firefox, chrome, safari). Default: all installed.
    #[arg(value_parser = parse_browser)]
    browsers: Vec<BrowserKind>,

    /// Output format: json, text, csv
    #[arg(short, long, default_value = "json")]
    format: String,

    /// Skip per-tab navigation history
    #[arg(long)]
    no_history: bool,

    /// Compact JSON (no pretty-printing)
    #[arg(long)]
    compact: bool,

    /// Write to file instead of stdout
    #[arg(short, long)]
    output: Option<String>,
}

fn parse_browser(s: &str) -> Result<BrowserKind, String> {
    s.parse()
}

fn main() {
    let cli = Cli::parse();

    let format = match cli.format.as_str() {
        "json" if cli.compact => Format::JsonCompact,
        "json" => Format::Json,
        "text" => Format::Text,
        "csv" => Format::Csv,
        other => {
            eprintln!("error: unknown format '{other}'. Use json, text, or csv.");
            std::process::exit(1);
        }
    };

    let include_history = !cli.no_history;

    let export = match browsewake::export_browsers(&cli.browsers, include_history) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let mut writer: Box<dyn Write> = if let Some(ref path) = cli.output {
        match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("error: cannot write to {path}: {e}");
                std::process::exit(1);
            }
        }
    } else {
        Box::new(io::stdout().lock())
    };

    if let Err(e) = output::write_export(&mut writer, &export, &format) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
