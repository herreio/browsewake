use crate::error::{BrowseWakeError, Result};
use std::path::PathBuf;

/// Returns all Firefox profile directories for the current OS.
pub fn firefox_profile_dirs() -> Result<Vec<PathBuf>> {
    let base = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine home directory".into()))?
            .join("Library/Application Support/Firefox/Profiles")
    } else if cfg!(target_os = "linux") {
        dirs::home_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine home directory".into()))?
            .join(".mozilla/firefox")
    } else if cfg!(target_os = "windows") {
        dirs::data_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine appdata directory".into()))?
            .join(r"Mozilla\Firefox\Profiles")
    } else {
        return Err(BrowseWakeError::Unsupported("firefox".into()));
    };

    glob_subdirs(&base)
}

/// Returns the Chrome user data directory for the current OS.
fn chrome_user_data_dir() -> Result<PathBuf> {
    let path = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine home directory".into()))?
            .join("Library/Application Support/Google/Chrome")
    } else if cfg!(target_os = "linux") {
        dirs::config_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine config directory".into()))?
            .join("google-chrome")
    } else if cfg!(target_os = "windows") {
        dirs::data_local_dir()
            .ok_or_else(|| {
                BrowseWakeError::Other("cannot determine local appdata directory".into())
            })?
            .join(r"Google\Chrome\User Data")
    } else {
        return Err(BrowseWakeError::Unsupported("chrome".into()));
    };

    if path.is_dir() {
        Ok(path)
    } else {
        Err(BrowseWakeError::NoProfile("chrome".into()))
    }
}

/// Returns Chrome profile directories (Default, Profile 1, Profile 2, etc.)
pub fn chrome_profile_dirs() -> Result<Vec<PathBuf>> {
    let user_data = chrome_user_data_dir()?;
    let mut profiles = Vec::new();

    // Check "Default" profile
    let default_profile = user_data.join("Default");
    if default_profile.is_dir() {
        profiles.push(default_profile);
    }

    // Check numbered profiles (Profile 1, Profile 2, ...)
    let pattern = user_data.join("Profile *").to_string_lossy().to_string();
    if let Ok(entries) = glob::glob(&pattern) {
        for entry in entries.flatten() {
            if entry.is_dir() {
                profiles.push(entry);
            }
        }
    }

    if profiles.is_empty() {
        Err(BrowseWakeError::NoProfile("chrome".into()))
    } else {
        Ok(profiles)
    }
}

/// Returns the Brave user data directory for the current OS.
fn brave_user_data_dir() -> Result<PathBuf> {
    let path = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine home directory".into()))?
            .join("Library/Application Support/BraveSoftware/Brave-Browser")
    } else if cfg!(target_os = "linux") {
        dirs::config_dir()
            .ok_or_else(|| BrowseWakeError::Other("cannot determine config directory".into()))?
            .join("BraveSoftware/Brave-Browser")
    } else if cfg!(target_os = "windows") {
        dirs::data_local_dir()
            .ok_or_else(|| {
                BrowseWakeError::Other("cannot determine local appdata directory".into())
            })?
            .join(r"BraveSoftware\Brave-Browser\User Data")
    } else {
        return Err(BrowseWakeError::Unsupported("brave".into()));
    };

    if path.is_dir() {
        Ok(path)
    } else {
        Err(BrowseWakeError::NoProfile("brave".into()))
    }
}

/// Returns Brave profile directories (Default, Profile 1, Profile 2, etc.)
pub fn brave_profile_dirs() -> Result<Vec<PathBuf>> {
    let user_data = brave_user_data_dir()?;
    let mut profiles = Vec::new();

    let default_profile = user_data.join("Default");
    if default_profile.is_dir() {
        profiles.push(default_profile);
    }

    let pattern = user_data.join("Profile *").to_string_lossy().to_string();
    if let Ok(entries) = glob::glob(&pattern) {
        for entry in entries.flatten() {
            if entry.is_dir() {
                profiles.push(entry);
            }
        }
    }

    if profiles.is_empty() {
        Err(BrowseWakeError::NoProfile("brave".into()))
    } else {
        Ok(profiles)
    }
}

/// Returns the Safari data directory (macOS only).
#[cfg(target_os = "macos")]
pub fn safari_data_dir() -> Result<PathBuf> {
    let path = dirs::home_dir()
        .ok_or_else(|| BrowseWakeError::Other("cannot determine home directory".into()))?
        .join("Library/Safari");

    if path.is_dir() {
        Ok(path)
    } else {
        Err(BrowseWakeError::NoProfile("safari".into()))
    }
}

fn glob_subdirs(base: &std::path::Path) -> Result<Vec<PathBuf>> {
    let pattern = base.join("*").to_string_lossy().to_string();
    let mut dirs = Vec::new();
    if let Ok(entries) = glob::glob(&pattern) {
        for entry in entries.flatten() {
            if entry.is_dir() {
                dirs.push(entry);
            }
        }
    }
    if dirs.is_empty() {
        Err(BrowseWakeError::NoProfile("browser".into()))
    } else {
        Ok(dirs)
    }
}
