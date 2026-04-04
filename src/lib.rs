pub mod browser;
pub mod error;
pub mod model;
pub mod output;

use error::Result;
use model::{BrowserKind, BrowserWindows, Export};

pub fn export_browsers(
    requested: &[BrowserKind],
    include_history: bool,
    deep_history: bool,
) -> Result<Export> {
    let sources = browser::get_sources(requested);

    if sources.is_empty() {
        eprintln!("warning: no browsers found");
    }

    let mut browsers: Vec<BrowserWindows> = Vec::new();

    for source in &sources {
        match source.export_tabs(deep_history) {
            Ok(mut bw) => {
                if !include_history {
                    for window in &mut bw.windows {
                        for tab in &mut window.tabs {
                            tab.history.clear();
                            tab.current_index = None;
                        }
                    }
                }
                browsers.push(bw);
            }
            Err(e) => {
                eprintln!("warning: failed to export {}: {e}", source.kind());
            }
        }
    }

    Ok(Export { browsers })
}
