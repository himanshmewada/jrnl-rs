use crate::{
    args::{Args, Postconfig, Preconfig},
    config,
    config::Settings,
    crypto,
    dates,
    editor,
    export,
    journal::{self, Journal, Kind}, // <-- add `self` here
    path,
    stats,
    ui,
};
use anyhow::{Result, bail};
use std::io::IsTerminal;

pub fn run(mut args: Args) -> Result<i32> {
    // 1. Preconfig commands
    match args.preconfig {
        Some(Preconfig::Help) => {
            print!("{}", crate::args::help());
            return Ok(0);
        }
        Some(Preconfig::Version) => {
            println!(
                "jrnl (diary) v{}\nCopyright © 2026 — MIT License.\nA Rust clone of jrnl (https://jrnl.sh).",
                config::VERSION
            );
            return Ok(0);
        }
        Some(Preconfig::Diagnostic) => {
            println!(
                "diary: v{}\nOS: {} {}\nArch: {}",
                config::VERSION,
                std::env::consts::OS,
                std::env::consts::FAMILY,
                std::env::consts::ARCH
            );
            return Ok(0);
        }
        None => {}
    }

    // 2. Config
    let mut original = config::load_or_install(args.config_file.as_deref())?;
    config::apply_overrides(&mut original, &args.config_override)?;
    let name = config::take_journal_name(&mut args.text, &original);
    let settings = config::scope(&original, &name)?;
    let alt = args.config_file.as_deref();

    // 3. Postconfig standalone commands
    if let Some(cmd) = args.postconfig {
        return match cmd {
            Postconfig::List => {
                list_journals(&original, args.export.as_deref());
                Ok(0)
            }
            Postconfig::Encrypt => encrypt_command(&args, &settings, &name, &mut original, alt),
            Postconfig::Decrypt => decrypt_command(&args, &settings, &name, &mut original, alt),
            Postconfig::Import => import_command(&args, &settings, &name, &mut original, alt),
        };
    }

    // 4. Open journal
    let mut journal = Journal::open(&name, settings.clone())?;

    if args.backup {
        let dest = journal.backup()?;
        ui::normal(&format!("Created a backup at {}", dest.display()));
        if !has_display(&args) && !has_search(&args) && !has_action(&args) && args.text.is_empty() {
            return Ok(0);
        }
    }

    // 5. Append vs search
    let all_tags = !args.text.is_empty()
        && args.text.iter().all(|w| {
            w.chars()
                .next()
                .map(|c| settings.tagsymbols.contains(c))
                .unwrap_or(false)
        });
    let mut append = !has_search(&args) && !has_action(&args) && !has_display(&args) && !all_tags;
    if args.edit && !args.text.is_empty() {
        append = true;
    }

    if append {
        append_mode(&args, &settings, &mut journal)?;
    } else {
        journal.filter(&args)?;
        let found = journal.len();
        print_found_count(found, &args);
        if has_action(&args) {
            perform_actions(&args, &settings, &mut journal)?;
            if found > 0 {
                print_change_counts(&journal);
            }
        } else if args.count {
            println!("{found}");
        } else if args.stats {
            println!("{}", stats::report(&journal));
        } else if args.random {
            if found > 0 {
                use rand::Rng;
                let i = rand::thread_rng().gen_range(0..found);
                println!(
                    "{}",
                    export::pretty_entry(&journal.entries[i], &settings, false)
                );
            }
        } else if !args.backup || has_display(&args) {
            export::display(&journal, &args)?;
        }
    }

    // 6. Persist config changes (e.g. legacy encryption upgrade)
    persist_pending(&journal, &mut original, alt)?;
    Ok(0)
}

fn has_search(a: &Args) -> bool {
    a.on.is_some()
        || a.today_in_history
        || a.month.is_some()
        || a.day.is_some()
        || a.year.is_some()
        || a.from.is_some()
        || a.to.is_some()
        || !a.contains.is_empty()
        || a.strict
        || a.starred
        || a.tagged
        || a.limit.is_some()
        || !a.excluded.is_empty()
        || a.exclude_starred
        || a.exclude_tagged
        || a.grep.is_some()
}
fn has_action(a: &Args) -> bool {
    a.edit || a.delete || a.change_time.is_some()
}
fn has_display(a: &Args) -> bool {
    a.tags || a.short || a.export.is_some() || a.all || a.random || a.stats || a.count
}

fn append_mode(args: &Args, settings: &Settings, journal: &mut Journal) -> Result<()> {
    let template_path = args
        .template
        .clone()
        .or_else(|| settings.template.path().map(String::from));
    let template_text = match &template_path {
        Some(p) => Some(editor::read_template(p)?),
        None => None,
    };

    let raw = if !args.text.is_empty() {
        let raw = args.text.join(" ");
        if args.edit {
            journal::open_editor(settings, &raw)?
        } else {
            raw
        }
    } else if !std::io::stdin().is_terminal() {
        editor::read_stdin()
    } else if !settings.editor.trim().is_empty() {
        journal::open_editor(settings, template_text.as_deref().unwrap_or(""))?
    } else {
        editor::compose_via_stdin()
    };

    if let Some(t) = &template_text {
        if &raw == t {
            ui::normal("No entry to save, because the template was not changed");
            return Ok(());
        }
    }
    if raw.trim().is_empty() {
        ui::normal("No entry to save, because no text was received");
        return Ok(());
    }

    let existing = journal.existing_tags();
    let entry = journal.new_entry(&raw);
    for (tag, similar) in crate::journal::similar_tags(&existing, &entry.tags) {
        ui::warning(&format!(
            "The tag '{tag}' is similar to: {}\nWas this intentional? If not, edit the entry with 'jrnl --edit' to fix it.",
            similar.join(", ")
        ));
    }
    if journal.name != "default" {
        ui::normal(&format!("Entry added to {} journal", journal.name));
    }
    journal.write(None)?;
    Ok(())
}

fn perform_actions(args: &Args, settings: &Settings, journal: &mut Journal) -> Result<()> {
    if let Some(ct) = &args.change_time {
        let idx = journal.prompt_action("Change time for");
        if !idx.is_empty() {
            let date = dates::parse(ct, false, settings.default_hour, settings.default_minute)
                .ok_or_else(|| anyhow::anyhow!("could not parse date '{ct}'"))?;
            journal.change_time(&idx, date);
            journal.write(None)?;
        }
    }
    if args.delete {
        let idx = journal.prompt_action("Delete entry");
        if !idx.is_empty() {
            journal.delete_entries(&idx);
            journal.write(None)?;
        }
    }
    if args.edit {
        let edited = journal::open_editor(settings, &journal.editable_str())?;
        if edited.trim().is_empty() {
            ui::warning(
                "No text received from editor. Were you trying to delete all the entries?\nThis seems a bit drastic, so the operation was cancelled.\nTo delete all entries, use the --delete option.",
            );
            return Ok(());
        }
        journal.parse_editable(&edited);
        journal.write(None)?;
    }
    Ok(())
}

fn print_found_count(n: usize, args: &Args) {
    if n == 0 {
        if args.edit || args.change_time.is_some() {
            ui::warning("No entries to modify, because the search returned no results");
        } else if args.delete {
            ui::warning("No entries to delete, because the search returned no results");
        } else {
            ui::normal("no entries found");
        }
    } else if args.limit != Some(n) {
        ui::normal(&format!(
            "{n} entr{} found",
            if n == 1 { "y" } else { "ies" }
        ));
    }
}

fn print_change_counts(j: &Journal) {
    let (added, deleted, modified) = j.change_counts();
    let plural = |n: usize| if n == 1 { "entry" } else { "entries" };
    if added > 0 {
        ui::normal(&format!("{added} {} added", plural(added)));
    }
    if deleted > 0 {
        ui::normal(&format!("{deleted} {} deleted", plural(deleted)));
    }
    if modified > 0 {
        ui::normal(&format!("{modified} {} modified", plural(modified)));
    }
    if added + deleted + modified == 0 {
        ui::normal("No edits to save, because nothing was changed");
    }
}

fn persist_pending(j: &Journal, original: &mut serde_yaml::Value, alt: Option<&str>) -> Result<()> {
    if j.pending_config.is_empty() {
        return Ok(());
    }
    for (k, v) in &j.pending_config {
        config::set_scoped(original, &j.name, k, v.clone(), false);
    }
    config::save(original, alt)
}

// ---------------- standalone commands ----------------
fn list_journals(config: &serde_yaml::Value, format: Option<&str>) {
    let config_path = path::config_path().to_string_lossy().into_owned();
    let journals = config
        .get("journals")
        .cloned()
        .unwrap_or(serde_yaml::Value::Null);
    match format {
        Some("json") => println!(
            "{}",
            serde_json::json!({"config_path": config_path, "journals": journals})
        ),
        Some("yaml") => {
            let mut m = serde_yaml::Mapping::new();
            m.insert("config_path".into(), config_path.into());
            m.insert("journals".into(), journals);
            print!("{}", serde_yaml::to_string(&m).unwrap());
        }
        _ => {
            println!("Journals defined in config ({config_path})");
            if let Some(map) = journals.as_mapping() {
                for (name, cfg) in map {
                    let p = cfg
                        .get("journal")
                        .and_then(|v| v.as_str())
                        .or_else(|| cfg.as_str())
                        .unwrap_or("?");
                    println!(" * {:<20} -> {p}", name.as_str().unwrap_or("?"));
                }
            }
        }
    }
}

fn encrypt_command(
    args: &Args,
    settings: &Settings,
    name: &str,
    original: &mut serde_yaml::Value,
    alt: Option<&str>,
) -> Result<i32> {
    let journal = Journal::open(name, settings.clone())?;
    if journal.kind != Kind::File {
        let kind_name = match journal.kind {
            Kind::Folder => "Folder",
            Kind::DayOne => "DayOne",
            Kind::File => "File",
        };
        bail!(
            "The journal {name} can't be encrypted because it is a {kind_name} journal.\nTo encrypt it, create a new journal referencing a file, export this journal to the new journal, then encrypt the new journal."
        );
    }
    if settings.encrypt.on() {
        ui::normal(&format!(
            "Journal {name} is already encrypted. Create a new password."
        ));
    }
    let text = journal.serialize();
    let mut pw = None;
    let bytes = crypto::encrypt_v3(&text, name, &mut pw, true)?;
    let target = args.filename.as_deref().map(path::expand);
    path::atomic_write(target.as_deref().unwrap_or(&journal.path), &bytes)?;
    ui::normal(&format!(
        "Journal encrypted to {}",
        target.as_deref().unwrap_or(&journal.path).display()
    ));
    if target.is_none() {
        config::set_scoped(original, name, "encrypt", true.into(), true);
        config::save(original, alt)?;
    }
    Ok(0)
}

fn decrypt_command(
    args: &Args,
    settings: &Settings,
    name: &str,
    original: &mut serde_yaml::Value,
    alt: Option<&str>,
) -> Result<i32> {
    if !settings.encrypt.on() {
        bail!("Journal \"{name}\" is not encrypted");
    }
    let journal = Journal::open(name, settings.clone())?;
    let target = args.filename.as_deref().map(path::expand);
    path::atomic_write(
        target.as_deref().unwrap_or(&journal.path),
        journal.serialize().as_bytes(),
    )?;
    ui::normal(&format!(
        "Journal decrypted to {}",
        target.as_deref().unwrap_or(&journal.path).display()
    ));
    if target.is_none() {
        config::set_scoped(original, name, "encrypt", false.into(), true);
        config::save(original, alt)?;
    }
    Ok(0)
}

fn import_command(
    args: &Args,
    settings: &Settings,
    name: &str,
    original: &mut serde_yaml::Value,
    alt: Option<&str>,
) -> Result<i32> {
    let mut journal = Journal::open(name, settings.clone())?;
    let text = match &args.filename {
        Some(f) => std::fs::read_to_string(path::expand(f))?,
        None => editor::read_stdin(),
    };
    let count = journal.import_text(&text);
    journal.write(None)?;
    ui::normal(&format!("{count} imported to {name} journal"));
    persist_pending(&journal, original, alt)?;
    Ok(0)
}
