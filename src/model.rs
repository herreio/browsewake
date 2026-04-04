use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserKind {
    Firefox,
    Chrome,
    Brave,
    Safari,
}

impl fmt::Display for BrowserKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrowserKind::Firefox => write!(f, "firefox"),
            BrowserKind::Chrome => write!(f, "chrome"),
            BrowserKind::Brave => write!(f, "brave"),
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
            "brave" => Ok(BrowserKind::Brave),
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
pub struct VisitEntry {
    pub url: String,
    pub title: String,
    pub visit_time: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tab {
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<NavEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_index: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deep_history: Vec<VisitEntry>,
    #[serde(skip)]
    pub tab_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserWindows {
    pub browser: BrowserKind,
    pub windows: Vec<Window>,
}

impl BrowserWindows {
    pub fn tab_count(&self) -> usize {
        self.windows.iter().map(|w| w.tabs.len()).sum()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Export {
    pub browsers: Vec<BrowserWindows>,
}
