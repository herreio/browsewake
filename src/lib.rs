pub mod browser;
pub mod error;
pub mod model;
pub mod output;

use error::{BrowseWakeError, Result};
use model::{BrowserKind, BrowserWindows, Export};

fn validate_requested_browsers(
    requested: &[BrowserKind],
    present: &[BrowserKind],
    available: &[BrowserKind],
) -> Result<()> {
    if requested.is_empty() {
        return Ok(());
    }

    for kind in requested {
        if !present.contains(kind) {
            return Err(BrowseWakeError::Unsupported(kind.to_string()));
        }
        if !available.contains(kind) {
            return Err(BrowseWakeError::NoProfile(kind.to_string()));
        }
    }

    Ok(())
}

pub fn export_browsers(
    requested: &[BrowserKind],
    include_history: bool,
    deep_history: bool,
) -> Result<Export> {
    let sources = browser::get_sources(requested);
    let present: Vec<BrowserKind> = sources.iter().map(|s| s.kind()).collect();
    let available: Vec<BrowserKind> = sources
        .iter()
        .filter(|s| s.available())
        .map(|s| s.kind())
        .collect();

    validate_requested_browsers(requested, &present, &available)?;

    if requested.is_empty() && sources.is_empty() {
        eprintln!("warning: no browsers found");
    }

    let mut browsers: Vec<BrowserWindows> = Vec::new();

    for source in &sources {
        if requested.is_empty() && !source.available() {
            continue;
        }

        match source.export_tabs(deep_history) {
            Ok(mut bw) => {
                if !include_history {
                    for window in &mut bw.windows {
                        for tab in &mut window.tabs {
                            tab.history.clear();
                            tab.current_index = None;
                            tab.deep_history.clear();
                        }
                    }
                }
                browsers.push(bw);
            }
            Err(e) => {
                if requested.contains(&source.kind()) {
                    return Err(e);
                }
                eprintln!("warning: failed to export {}: {e}", source.kind());
            }
        }
    }

    Ok(Export { browsers })
}

#[cfg(test)]
mod tests {
    use super::validate_requested_browsers;
    use crate::error::BrowseWakeError;
    use crate::model::BrowserKind;

    #[test]
    fn explicit_requested_browser_must_be_supported() {
        let err = validate_requested_browsers(
            &[BrowserKind::Safari],
            &[
                BrowserKind::Firefox,
                BrowserKind::Chrome,
                BrowserKind::Brave,
            ],
            &[
                BrowserKind::Firefox,
                BrowserKind::Chrome,
                BrowserKind::Brave,
            ],
        )
        .unwrap_err();

        assert!(matches!(err, BrowseWakeError::Unsupported(browser) if browser == "safari"));
    }

    #[test]
    fn explicit_requested_browser_must_be_available() {
        let err = validate_requested_browsers(&[BrowserKind::Chrome], &[BrowserKind::Chrome], &[])
            .unwrap_err();

        assert!(matches!(err, BrowseWakeError::NoProfile(browser) if browser == "chrome"));
    }
}
