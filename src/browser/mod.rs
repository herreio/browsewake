pub mod brave;
pub mod chrome;
pub mod firefox;
pub mod paths;
#[cfg(target_os = "macos")]
pub mod safari;
pub mod snss;

use crate::error::Result;
use crate::model::{BrowserKind, BrowserWindows};

pub trait BrowserSource {
    fn kind(&self) -> BrowserKind;
    fn available(&self) -> bool;
    fn export_tabs(&self) -> Result<BrowserWindows>;
}

/// Returns browser sources for the requested browsers, or all available if none specified.
pub fn get_sources(requested: &[BrowserKind]) -> Vec<Box<dyn BrowserSource>> {
    let all: Vec<Box<dyn BrowserSource>> = {
        let mut v: Vec<Box<dyn BrowserSource>> = vec![
            Box::new(firefox::Firefox),
            Box::new(chrome::Chrome),
            Box::new(brave::Brave),
        ];
        #[cfg(target_os = "macos")]
        v.push(Box::new(safari::Safari));
        v
    };

    if requested.is_empty() {
        all.into_iter().filter(|s| s.available()).collect()
    } else {
        all.into_iter()
            .filter(|s| requested.contains(&s.kind()))
            .collect()
    }
}
