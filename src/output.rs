use crate::error::Result;
use crate::model::Export;
use std::io::Write;

pub enum Format {
    Json,
    JsonCompact,
    Text,
    Csv,
}

pub fn write_export(w: &mut dyn Write, export: &Export, format: &Format) -> Result<()> {
    match format {
        Format::Json => {
            serde_json::to_writer_pretty(&mut *w, export)?;
            writeln!(w)?;
        }
        Format::JsonCompact => {
            serde_json::to_writer(&mut *w, export)?;
            writeln!(w)?;
        }
        Format::Text => write_text(w, export)?,
        Format::Csv => write_csv(w, export)?,
    }
    Ok(())
}

fn write_text(w: &mut dyn Write, export: &Export) -> Result<()> {
    for bw in &export.browsers {
        writeln!(
            w,
            "=== {} ({} tabs in {} window{}) ===",
            bw.browser,
            bw.tab_count(),
            bw.windows.len(),
            if bw.windows.len() == 1 { "" } else { "s" },
        )?;
        for (wi, window) in bw.windows.iter().enumerate() {
            if bw.windows.len() > 1 {
                writeln!(
                    w,
                    "  --- Window {} ({} tabs) ---",
                    wi + 1,
                    window.tabs.len()
                )?;
            }
            for (i, tab) in window.tabs.iter().enumerate() {
                writeln!(w, "  Tab {}: {}", i + 1, tab.title)?;
                writeln!(w, "    URL: {}", tab.url)?;
                if !tab.history.is_empty() {
                    writeln!(w, "    History ({} entries):", tab.history.len())?;
                    for entry in &tab.history {
                        let marker = if tab.current_index == Some(entry.index) {
                            " <-- current"
                        } else {
                            ""
                        };
                        writeln!(w, "      [{}] {}{}", entry.index, entry.url, marker)?;
                    }
                }
                if !tab.deep_history.is_empty() {
                    writeln!(w, "    Deep History ({} visits):", tab.deep_history.len())?;
                    for visit in &tab.deep_history {
                        let from = visit
                            .from_url
                            .as_deref()
                            .map(|u| format!(" <- {u}"))
                            .unwrap_or_default();
                        writeln!(w, "      {}{}", visit.url, from)?;
                    }
                }
            }
        }
        writeln!(w)?;
    }
    Ok(())
}

fn write_csv(w: &mut dyn Write, export: &Export) -> Result<()> {
    writeln!(
        w,
        "browser,window,tab_index,url,title,history_index,history_url,history_title"
    )?;
    for bw in &export.browsers {
        for (wi, window) in bw.windows.iter().enumerate() {
            for (i, tab) in window.tabs.iter().enumerate() {
                if tab.history.is_empty() {
                    writeln!(
                        w,
                        "{},{},{},{},{},,,",
                        bw.browser,
                        wi + 1,
                        i,
                        csv_escape(&tab.url),
                        csv_escape(&tab.title),
                    )?;
                } else {
                    for entry in &tab.history {
                        writeln!(
                            w,
                            "{},{},{},{},{},{},{},{}",
                            bw.browser,
                            wi + 1,
                            i,
                            csv_escape(&tab.url),
                            csv_escape(&tab.title),
                            entry.index,
                            csv_escape(&entry.url),
                            csv_escape(&entry.title),
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
