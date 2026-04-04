use crate::browser::BrowserSource;
use crate::browser::paths::brave_profile_dirs;
use crate::browser::snss;
use crate::error::Result;
use crate::model::{BrowserKind, BrowserWindows};

pub struct Brave;

impl BrowserSource for Brave {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Brave
    }

    fn available(&self) -> bool {
        brave_profile_dirs().is_ok()
    }

    fn export_tabs(&self, deep_history: bool) -> Result<BrowserWindows> {
        let profiles = brave_profile_dirs()?;
        let windows = snss::read_chromium_sessions(&profiles, "Brave", deep_history)?;
        Ok(BrowserWindows {
            browser: BrowserKind::Brave,
            windows,
        })
    }
}
