use crate::{path, ui};
use anyhow::{Result, bail};
use std::io::{Read, Write};

pub fn read_stdin() -> String {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

pub fn compose_via_stdin() -> String {
    let how = if cfg!(windows) {
        "Ctrl+z and then Enter"
    } else {
        "Ctrl+d"
    };
    ui::normal(&format!(
        "Writing Entry\nTo finish writing, press {how} on a blank line."
    ));
    read_stdin()
}

pub fn get_text_from_editor(
    editor: &str,
    prepopulated: &str,
    template_name: Option<&str>,
) -> Result<String> {
    let suffix = match template_name {
        Some(t) => format!(
            "-{}",
            std::path::Path::new(t)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ),
        None => ".jrnl".into(),
    };
    let mut tmp = tempfile::Builder::new()
        .prefix("jrnl")
        .suffix(&suffix)
        .tempfile()?;
    tmp.write_all(prepopulated.as_bytes())?;
    tmp.flush()?;
    let tmp_path = tmp.path().to_path_buf();

    let parts = shlex::split(editor)
        .ok_or_else(|| anyhow::anyhow!("Could not parse editor command: {editor}"))?;
    if parts.is_empty() {
        bail!("Empty editor command");
    }
    let status = std::process::Command::new(&parts[0])
        .args(&parts[1..])
        .arg(&tmp_path)
        .status()
        .map_err(|_| anyhow::anyhow!(
            "No such file or directory: '{}'\nPlease check the 'editor' key in your config file.", parts[0]))?;
    crate::debug!("editor exited with {status}");
    let raw = std::fs::read_to_string(&tmp_path)?;
    drop(tmp); // deletes the temp file
    Ok(raw)
}

pub fn read_template(template_path: &str) -> Result<String> {
    let in_dir = path::templates_dir().join(template_path);
    let actual = if in_dir.exists() {
        in_dir
    } else {
        path::expand(template_path)
    };
    std::fs::read_to_string(&actual).map_err(|_| {
        anyhow::anyhow!(
            "Unable to find a template file {template_path}.\nChecked:\n  * {}\n  * {}",
            path::templates_dir().join(template_path).display(),
            actual.display()
        )
    })
}
