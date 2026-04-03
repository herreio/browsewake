use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserKind {
    Firefox,
    Chrome,
    Safari,
}

impl fmt::Display for BrowserKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrowserKind::Firefox => write!(f, "firefox"),
            BrowserKind::Chrome => write!(f, "chrome"),
            BrowserKind::Safari => write!(f, "safari"),
        }
    }
}

impl std::str::FromStr for BrowserKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "firefox" => Ok(BrowserKind::Firefox),
            "chrome" => Ok(BrowserKind::Chrome),
            "safari" => Ok(BrowserKind::Safari),
            other => Err(format!("unknown browser: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NavEntry {
    pub url: String,
    pub title: String,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tab {
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<NavEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserTabs {
    pub browser: BrowserKind,
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Export {
    pub browsers: Vec<BrowserTabs>,
}
