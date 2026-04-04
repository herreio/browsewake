use crate::browser::BrowserSource;
use crate::browser::paths::chrome_profile_dirs;
use crate::browser::snss;
use crate::error::Result;
use crate::model::{BrowserKind, BrowserWindows};

pub struct Chrome;

impl BrowserSource for Chrome {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Chrome
    }

    fn available(&self) -> bool {
        chrome_profile_dirs().is_ok()
    }

    fn export_tabs(&self, deep_history: bool) -> Result<BrowserWindows> {
        let profiles = chrome_profile_dirs()?;
        let windows = snss::read_chromium_sessions(&profiles, "Chrome", deep_history)?;
        Ok(BrowserWindows {
            browser: BrowserKind::Chrome,
            windows,
        })
    }
}
