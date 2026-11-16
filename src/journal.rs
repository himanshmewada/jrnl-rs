use crate::{config::Settings, crypto, dates, dayone, editor, entry, entry::Entry, path, ui};
use anyhow::{Result, bail};
use chrono::{Datelike, NaiveDateTime};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    File,
    Folder,
    DayOne,
}

pub struct Journal {
    pub name: String,
    pub settings: Settings,
    pub path: PathBuf,
    pub kind: Kind,
    pub entries: Vec<Entry>,
    pub diff_dates: Vec<NaiveDateTime>,
    pub deleted_uuids: Vec<String>,
    pub added: usize,
    pub deleted: usize,
    pub password: Option<String>,
    pub upgrade_from: Option<crypto::Version>,
    pub pending_config: Vec<(String, serde_yaml::Value)>,
}

impl Journal {
    pub fn open(name: &str, settings: Settings) -> Result<Self> {
        let raw_path = settings.journal.clone();
        let p = path::expand(&raw_path);
        let kind = if p.is_dir() {
            if p.extension().map(|e| e == "dayone").unwrap_or(false) || p.join("entries").is_dir() {
                Kind::DayOne
            } else {
                Kind::Folder
            }
        } else if raw_path.ends_with('/') || raw_path.ends_with('\\') {
            std::fs::create_dir_all(&p)?;
            Kind::Folder
        } else {
            Kind::File
        };

        if settings.encrypt.on() && kind != Kind::File {
            ui::warning(&format!(
                "The config for journal \"{name}\" has 'encrypt' set to true, but this type of journal can't be encrypted. Please fix your config file."
            ));
        }

        let mut j = Self {
            name: name.into(),
            settings,
            path: p,
            kind,
            entries: vec![],
            diff_dates: vec![],
            deleted_uuids: vec![],
            added: 0,
            deleted: 0,
            password: None,
            upgrade_from: None,
            pending_config: vec![],
        };
        match kind {
            Kind::File => j.open_file()?,
            Kind::Folder => j.open_folder()?,
            Kind::DayOne => j.entries = dayone::read(&j.path, &j.settings)?,
        }
        j.sort();
        Ok(j)
    }

    fn open_file(&mut self) -> Result<()> {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                    ui::normal(&format!("Directory {} created", parent.display()));
                }
            }
            path::atomic_write(&self.path, b"")?;
            ui::normal(&format!(
                "Journal '{}' created at {}",
                self.name,
                self.path.display()
            ));
        }
        let data = std::fs::read(&self.path)?;
        let text = if self.settings.encrypt.on() && !data.is_empty() {
            let ver = crypto::detect(&data, self.settings.encrypt.v1_hint());
            let text = crypto::decrypt(&data, ver, &self.name, &mut self.password)?;
            if ver != crypto::Version::V3 {
                self.upgrade_from = Some(ver);
                ui::warning(&format!(
                    "The journal \"{}\" uses the legacy {} encryption format. Save a change to the journal to upgrade to more secure encryption.",
                    self.name,
                    ver.label()
                ));
            }
            text
        } else {
            String::from_utf8_lossy(&data).into_owned()
        };

        self.entries = if self.upgrade_from == Some(crypto::Version::V1) {
            entry::parse_legacy(&text, &self.settings.timeformat, &self.settings.tagsymbols)
        } else {
            entry::parse_journal(&text, &self.settings.timeformat, &self.settings.tagsymbols)
        };
        Ok(())
    }

    fn open_folder(&mut self) -> Result<()> {
        for (file, y, m, d) in folder_files(&self.path) {
            let text = std::fs::read_to_string(&file)?;
            for mut e in
                entry::parse_journal(&text, &self.settings.timeformat, &self.settings.tagsymbols)
            {
                // Path (YYYY/MM/DD) is authoritative for the date (#2006).
                e.date = e
                    .date
                    .with_year(y)
                    .and_then(|x| x.with_month(m))
                    .and_then(|x| x.with_day(d))
                    .unwrap_or(e.date);
                self.entries.push(e);
            }
        }
        Ok(())
    }

    pub fn sort(&mut self) {
        self.entries.sort_by_key(|e| e.date);
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn serialize(&self) -> String {
        self.entries
            .iter()
            .map(|e| e.to_storage(&self.settings.timeformat))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn write(&mut self, target: Option<&Path>) -> Result<()> {
        match self.kind {
            Kind::File => {
                let text = self.serialize();
                let bytes = if self.settings.encrypt.on() {
                    if let Some(from) = self.upgrade_from {
                        ui::warning(&format!(
                            "Upgrading \"{}\" from {} to v3 encryption.",
                            self.name,
                            from.label()
                        ));
                    }
                    let b = crypto::encrypt_v3(&text, &self.name, &mut self.password, false)?;
                    if let Some(from) = self.upgrade_from.take() {
                        self.pending_config.push(("encrypt".into(), true.into()));
                        ui::warning(&format!(
                            "Successfully upgraded \"{}\" from {} to v3 encryption.",
                            self.name,
                            from.label()
                        ));
                    }
                    b
                } else {
                    text.into_bytes()
                };
                path::atomic_write(target.unwrap_or(&self.path), &bytes)?;
            }
            Kind::Folder => self.write_folder()?,
            Kind::DayOne => dayone::write(
                &self.path,
                &self.entries,
                &self.deleted_uuids,
                &self.settings,
            )?,
        }
        self.git_sync();
        Ok(())
    }

    fn write_folder(&mut self) -> Result<()> {
        let mut dates_to_write: Vec<NaiveDateTime> = self.diff_dates.drain(..).collect();
        for e in &self.entries {
            if e.modified && !dates_to_write.iter().any(|d| same_day(*d, e.date)) {
                dates_to_write.push(e.date);
            }
        }
        for d in dates_to_write {
            let file = self.path.join(format!(
                "{:04}/{:02}/{:02}.txt",
                d.year(),
                d.month(),
                d.day()
            ));
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let day_entries: Vec<String> = self
                .entries
                .iter()
                .filter(|e| same_day(e.date, d))
                .map(|e| e.to_storage(&self.settings.timeformat))
                .collect();
            path::atomic_write(&file, day_entries.join("\n").as_bytes())?;
        }
        for (file, _, _, _) in folder_files(&self.path) {
            if std::fs::metadata(&file)
                .map(|m| m.len() == 0)
                .unwrap_or(false)
            {
                let _ = std::fs::remove_file(file);
            }
        }
        Ok(())
    }

    fn git_sync(&self) {
        if !self.settings.git.enabled {
            return;
        }
        let dir = if self.kind == Kind::File {
            self.path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            self.path.clone()
        };
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
        };
        if run(&["rev-parse", "--git-dir"])
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            let _ = run(&["init"]);
        }
        let _ = run(&["add", "."]);
        let _ = run(&["commit", "-m", "jrnl: update journal"]);
        if self.settings.git.push {
            if run(&["push"]).map(|s| !s.success()).unwrap_or(true) {
                ui::warning("git push failed");
            }
        }
    }

    // ---------- writing entries ----------
    pub fn new_entry(&mut self, raw: &str) -> Entry {
        let s = &self.settings;
        let first_line_end = raw
            .find('\n')
            .or_else(|| {
                regex::Regex::new(r"[?!.]+ +")
                    .unwrap()
                    .find(raw)
                    .map(|m| m.end())
            })
            .unwrap_or(raw.len());
        let first_line = &raw[..first_line_end];
        let mut body_raw = raw.to_string();
        let mut date = None;
        let mut starred = false;
        if let Some(colon) = first_line.find(": ") {
            if colon > 0 {
                if let Some(d) =
                    dates::parse(&raw[..colon], false, s.default_hour, s.default_minute)
                {
                    starred = raw[..colon].trim().ends_with('*');
                    body_raw = raw[colon + 1..].trim().to_string();
                    date = Some(d);
                }
            }
        }
        starred = starred
            || first_line.starts_with('*')
            || first_line.ends_with('*')
            || body_raw.starts_with('*');
        let date = date.unwrap_or_else(|| {
            dates::parse("now", false, s.default_hour, s.default_minute).unwrap()
        });
        let e = Entry::new(date, &body_raw, starred, &s.tagsymbols);
        self.entries.push(e.clone());
        self.sort();
        e
    }

    pub fn existing_tags(&self) -> HashSet<String> {
        self.entries
            .iter()
            .flat_map(|e| e.tags.iter().cloned())
            .collect()
    }

    // ---------- filtering ----------
    pub fn filter(&mut self, a: &crate::args::Args) -> Result<()> {
        let tags: HashSet<String> = a.text.iter().map(|t| t.to_lowercase()).collect();
        let excluded: HashSet<String> = a.excluded.iter().map(|t| t.to_lowercase()).collect();
        let mut start = a.from.as_deref().and_then(|d| dates::parse(d, false, 0, 0));
        let mut end = a.to.as_deref().and_then(|d| dates::parse(d, true, 0, 0));
        if let Some(on) = &a.on {
            start = dates::parse(on, false, 0, 0);
            end = dates::parse(on, true, 0, 0);
        }
        let now = chrono::Local::now().naive_local();
        let month = if a.today_in_history {
            Some(now.month())
        } else {
            a.month.as_deref().and_then(dates::month_num)
        };
        let day = if a.today_in_history {
            Some(now.day())
        } else {
            a.day.as_deref().and_then(|d| d.parse::<u32>().ok())
        };
        let year = a.year.as_deref().and_then(dates::year_num);
        let contains: Vec<String> = a.contains.iter().map(|c| c.to_lowercase()).collect();
        let grep = match &a.grep {
            Some(g) => {
                Some(regex::Regex::new(g).map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?)
            }
            None => None,
        };

        self.entries.retain(|e| {
            let etags: HashSet<String> = e.tags.iter().cloned().collect();
            let tag_ok = tags.is_empty()
                || if a.strict {
                    tags.is_subset(&etags)
                } else {
                    !tags.is_disjoint(&etags)
                };
            let star_ok = (!a.starred || e.starred) && (!a.exclude_starred || !e.starred);
            let tagged_ok =
                (!a.tagged || !e.tags.is_empty()) && (!a.exclude_tagged || e.tags.is_empty());
            let excl_ok = excluded.is_disjoint(&etags);
            let date_ok = month.map_or(true, |m| e.date.month() == m)
                && day.map_or(true, |d| e.date.day() == d)
                && year.map_or(true, |y| e.date.year() == y)
                && start.map_or(true, |d| e.date >= d)
                && end.map_or(true, |d| e.date <= d);
            let hay = (e.title.to_lowercase(), e.body.to_lowercase());
            let contains_ok = contains.is_empty()
                || if a.strict {
                    contains
                        .iter()
                        .all(|c| hay.0.contains(c) || hay.1.contains(c))
                } else {
                    contains
                        .iter()
                        .any(|c| hay.0.contains(c) || hay.1.contains(c))
                };
            let grep_ok = grep.as_ref().map_or(true, |g| g.is_match(&e.fulltext()));
            tag_ok && star_ok && tagged_ok && excl_ok && date_ok && contains_ok && grep_ok
        });
        if let Some(n) = a.limit {
            if self.entries.len() > n {
                self.entries = self.entries.split_off(self.entries.len() - n);
            }
        }
        Ok(())
    }

    // ---------- interactive actions ----------
    pub fn prompt_action(&self, question: &str) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&i| {
                ui::yes_no(
                    &format!(
                        "{question} '{}'?",
                        self.entries[i].short_line(&self.settings.timeformat)
                    ),
                    false,
                )
            })
            .collect()
    }

    pub fn delete_entries(&mut self, indices: &[usize]) {
        let set: HashSet<usize> = indices.iter().copied().collect();
        let mut kept = vec![];
        for (i, e) in self.entries.drain(..).enumerate() {
            if set.contains(&i) {
                self.diff_dates.push(e.date);
                if let Some(u) = &e.uuid {
                    self.deleted_uuids.push(u.clone());
                }
                self.deleted += 1;
            } else {
                kept.push(e);
            }
        }
        self.entries = kept;
    }

    pub fn change_time(&mut self, indices: &[usize], date: NaiveDateTime) {
        for (i, e) in self.entries.iter_mut().enumerate() {
            if indices.contains(&i) {
                self.diff_dates.push(e.date);
                e.date = date;
                e.modified = true;
            }
        }
        self.sort();
    }

    // ---------- editing ----------
    pub fn editable_str(&self) -> String {
        if self.kind == Kind::DayOne {
            self.entries
                .iter()
                .map(|e| {
                    format!(
                        "{}\n# {}\n",
                        e.to_storage(&self.settings.timeformat),
                        e.uuid.clone().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            self.serialize()
        }
    }

    pub fn parse_editable(&mut self, edited: &str) {
        let s = &self.settings;
        let mut new_entries = entry::parse_journal(edited, &s.timeformat, &s.tagsymbols);
        if self.kind == Kind::DayOne {
            let re = regex::Regex::new(r"(?m)^ *# ([A-Za-z0-9]+) *$").unwrap();
            for e in &mut new_entries {
                if let Some(c) = re.captures(&e.body) {
                    e.uuid = Some(c[1].to_string());
                    e.body = re.replace(&e.body, "").trim_end().to_string();
                }
            }
            let edited_uuids: HashSet<String> =
                new_entries.iter().filter_map(|e| e.uuid.clone()).collect();
            for old in &self.entries {
                if let Some(u) = &old.uuid {
                    if !edited_uuids.contains(u) {
                        self.deleted_uuids.push(u.clone());
                    }
                }
            }
            // Preserve metadata by uuid.
            for e in &mut new_entries {
                if let Some(old) = self
                    .entries
                    .iter()
                    .find(|o| o.uuid == e.uuid && o.uuid.is_some())
                {
                    e.meta = old.meta.clone();
                }
            }
        }
        let old_keys: HashSet<_> = self.entries.iter().map(|e| e.key()).collect();
        for e in &mut new_entries {
            e.modified = !old_keys.contains(&e.key());
        }
        if new_entries.len() > self.entries.len() {
            self.added += new_entries.len() - self.entries.len();
        } else {
            self.deleted += self.entries.len() - new_entries.len();
            for old in &self.entries {
                if !new_entries.iter().any(|n| n.key() == old.key()) {
                    self.diff_dates.push(old.date);
                }
            }
        }
        self.entries = new_entries;
        self.sort();
    }

    pub fn change_counts(&self) -> (usize, usize, usize) {
        (
            self.added,
            self.deleted,
            self.entries.iter().filter(|e| e.modified).count(),
        )
    }

    pub fn import_text(&mut self, text: &str) -> usize {
        let incoming =
            entry::parse_journal(text, &self.settings.timeformat, &self.settings.tagsymbols);
        let existing: HashSet<_> = self.entries.iter().map(|e| e.key()).collect();
        let mut count = 0;
        for mut e in incoming {
            if !existing.contains(&e.key()) && !self.entries.iter().any(|x| x.key() == e.key()) {
                e.modified = true;
                self.entries.push(e);
                count += 1;
            }
        }
        self.sort();
        count
    }

    pub fn backup(&self) -> Result<PathBuf> {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let dest = self.path.with_file_name(format!(
            "{}.backup-{stamp}",
            self.path.file_name().unwrap_or_default().to_string_lossy()
        ));
        if self.kind == Kind::File {
            std::fs::copy(&self.path, &dest)?;
        } else {
            copy_dir(&self.path, &dest)?;
        }
        Ok(dest)
    }
}

fn same_day(a: NaiveDateTime, b: NaiveDateTime) -> bool {
    a.date() == b.date()
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let e = entry?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

pub fn folder_files(root: &Path) -> Vec<(PathBuf, i32, u32, u32)> {
    let mut out = vec![];
    let Ok(years) = std::fs::read_dir(root) else {
        return out;
    };
    for y in years.flatten() {
        let Some(yn) = y.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        if !y.path().is_dir() || y.file_name().len() != 4 {
            continue;
        }
        let Ok(months) = std::fs::read_dir(y.path()) else {
            continue;
        };
        for m in months.flatten() {
            let Some(mn) = m.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            if !m.path().is_dir() || !(1..=12).contains(&mn) {
                continue;
            }
            let Ok(days) = std::fs::read_dir(m.path()) else {
                continue;
            };
            for d in days.flatten() {
                let p = d.path();
                if p.extension().map(|e| e != "txt").unwrap_or(true) || !p.is_file() {
                    continue;
                }
                let Some(dn) = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<u32>().ok())
                else {
                    continue;
                };
                if chrono::NaiveDate::from_ymd_opt(yn, mn, dn).is_some() {
                    out.push((p, yn, mn, dn));
                }
            }
        }
    }
    out
}

/// Similar-tag warning (same symbol only, like jrnl).
pub fn similar_tags(existing: &HashSet<String>, new_tags: &[String]) -> Vec<(String, Vec<String>)> {
    let mut out = vec![];
    for new in new_tags {
        let mut candidates: Vec<(f64, &String)> = existing
            .iter()
            .filter(|t| t.chars().next() == new.chars().next() && *t != new)
            .map(|t| (strsim::normalized_levenshtein(t, new), t))
            .filter(|(score, _)| *score >= 0.8)
            .collect();
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        if !candidates.is_empty() {
            out.push((
                new.clone(),
                candidates
                    .into_iter()
                    .take(3)
                    .map(|(_, t)| t.clone())
                    .collect(),
            ));
        }
    }
    out
}

pub fn ensure_editor(settings: &Settings) -> Result<()> {
    if settings.editor.trim().is_empty() {
        bail!(
            "There is no editor configured.\nTo use the --edit option, please specify an editor in your config file:\n{}",
            path::config_path().display()
        );
    }
    Ok(())
}

pub fn open_editor(settings: &Settings, prepop: &str) -> Result<String> {
    ensure_editor(settings)?;
    editor::get_text_from_editor(&settings.editor, prepop, settings.template.path())
}
