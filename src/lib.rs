pub mod browser;
pub mod error;
pub mod model;
pub mod output;

use error::Result;
use model::{BrowserKind, BrowserTabs, Export};

pub fn export_browsers(requested: &[BrowserKind], include_history: bool) -> Result<Export> {
    let sources = browser::get_sources(requested);

    if sources.is_empty() {
        eprintln!("warning: no browsers found");
    }

    let mut browsers: Vec<BrowserTabs> = Vec::new();

    for source in &sources {
        match source.export_tabs() {
            Ok(mut bt) => {
                if !include_history {
                    for tab in &mut bt.tabs {
                        tab.history.clear();
                        tab.current_index = None;
                    }
                }
                browsers.push(bt);
            }
            Err(e) => {
                eprintln!("warning: failed to export {}: {e}", source.kind());
            }
        }
    }

    Ok(Export { browsers })
}
