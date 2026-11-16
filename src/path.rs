use anyhow::{Result, anyhow};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("jrnl")
        .join("jrnl.yaml")
}

pub fn default_journal_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("jrnl")
        .join("journal.txt")
}

pub fn templates_dir() -> PathBuf {
    let p = dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("jrnl")
        .join("templates");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Expand `~`, `$VAR`, `${VAR}` and `%VAR%`.
pub fn expand(p: &str) -> PathBuf {
    let mut s = p.to_string();
    if s == "~" {
        if let Some(h) = dirs::home_dir() {
            return h;
        }
    } else if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(h) = dirs::home_dir() {
            s = h.join(rest).to_string_lossy().into_owned();
        }
    }
    let re = regex::Regex::new(r"\$\{(\w+)\}|\$(\w+)|%(\w+)%").unwrap();
    let out = re.replace_all(&s, |c: &regex::Captures| {
        let name = c.get(1).or(c.get(2)).or(c.get(3)).unwrap().as_str();
        std::env::var(name).unwrap_or_default()
    });
    PathBuf::from(out.into_owned())
}

/// Crash-safe write: temp file in the same directory, then atomic rename.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".jrnl-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(data)?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| anyhow!(e.error))?;
    Ok(())
}
