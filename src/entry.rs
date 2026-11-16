use crate::dates;
use chrono::NaiveDateTime;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Entry {
    pub date: NaiveDateTime,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub starred: bool,
    pub modified: bool,
    pub uuid: Option<String>,
    /// Original DayOne plist dict, preserved for round-tripping metadata.
    pub meta: Option<plist::Value>,
}

fn sentence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:[.!?…‼‽⁇⁈⁉⋯﹒﹗]['’\x22”]?[\])]*\s+)|[！．？｡。]").unwrap())
}

pub fn split_title(text: &str) -> (String, String) {
    let t = text.trim_start_matches('\n');
    if let Some(pos) = t.find('\n') {
        return (t[..pos].trim().into(), t[pos + 1..].trim().into());
    }
    if let Some(m) = sentence_re().find(t) {
        return (t[..m.end()].trim().into(), t[m.end()..].trim().into());
    }
    (t.trim().into(), String::new())
}

pub fn tag_regex(symbols: &str) -> Regex {
    let escaped: String = symbols
        .chars()
        .map(|c| {
            if "\\]^-[".contains(c) {
                format!("\\{c}")
            } else {
                c.to_string()
            }
        })
        .collect();
    Regex::new(&format!(r"(?:^|\s)([{escaped}][-+*#/\w]+)")).unwrap()
}

pub fn parse_tags(text: &str, symbols: &str) -> Vec<String> {
    let mut tags: Vec<String> = tag_regex(symbols)
        .captures_iter(text)
        .map(|c| c[1].to_lowercase())
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

impl Entry {
    pub fn new(date: NaiveDateTime, raw: &str, mut starred: bool, symbols: &str) -> Self {
        let (mut title, body) = split_title(raw);
        if title.starts_with('*') {
            starred = true;
            title = title.trim_start_matches('*').trim().into();
        }
        if title.ends_with('*') {
            starred = true;
            title = title.trim_end_matches('*').trim().into();
        }
        let tags = parse_tags(raw, symbols);
        Self {
            date,
            title,
            body,
            tags,
            starred,
            modified: false,
            uuid: None,
            meta: None,
        }
    }

    pub fn fulltext(&self) -> String {
        format!("{} {}", self.title, self.body)
    }

    pub fn key(&self) -> (NaiveDateTime, String, String, bool) {
        (
            self.date,
            self.title.trim().into(),
            self.body.trim_end().into(),
            self.starred,
        )
    }

    /// jrnl on-disk representation.
    pub fn to_storage(&self, timeformat: &str) -> String {
        let mut title = format!(
            "[{}] {}",
            self.date.format(timeformat),
            self.title.trim_end()
        );
        if self.starred {
            title.push_str(" *");
        }
        if self.body.trim_end().is_empty() {
            format!("{title}\n")
        } else {
            format!("{title}\n{}\n", self.body.trim_end())
        }
    }

    pub fn short_line(&self, timeformat: &str) -> String {
        format!("{} {}", self.date.format(timeformat), self.title)
    }
}

/// Parse the bracketed-date storage format.
pub fn parse_journal(text: &str, timeformat: &str, symbols: &str) -> Vec<Entry> {
    if text.trim().is_empty() {
        return vec![];
    }
    let re = Regex::new(r"(?:\A|\n)\[([^\]]+)\] ").unwrap();
    let mut entries: Vec<Entry> = vec![];
    let mut last_body_start = 0usize;
    let mut have_any = false;

    for m in re.find_iter(text) {
        let caps = re.captures(&text[m.start()..]).unwrap();
        let blob = &caps[1];
        let date = parse_blob_date(blob, timeformat);
        let Some(date) = date else { continue };
        if have_any {
            let raw = &text[last_body_start..m.start()];
            let e = entries.last_mut().unwrap();
            *e = Entry::new(e.date, raw, false, symbols);
        }
        entries.push(Entry::new(date, "", false, symbols));
        last_body_start = m.end();
        have_any = true;
    }
    if !have_any {
        // No dated entries: treat everything as one entry made now.
        let now = dates::parse("now", false, 9, 0).unwrap();
        return vec![Entry::new(now, text, false, symbols)];
    }
    let raw = &text[last_body_start..];
    let e = entries.last_mut().unwrap();
    *e = Entry::new(e.date, raw, false, symbols);
    entries
}

fn parse_blob_date(blob: &str, timeformat: &str) -> Option<NaiveDateTime> {
    if let Ok(dt) = NaiveDateTime::parse_from_str(blob, timeformat) {
        return Some(dt);
    }
    // Time-only formats (folder journals with "%H:%M"): date comes from the path.
    if let Ok(t) = chrono::NaiveTime::parse_from_str(blob, timeformat) {
        return Some(chrono::Local::now().date_naive().and_time(t));
    }
    if blob.len() <= 6 {
        return None;
    } // markdown footnotes like [1]
    dates::parse(blob, false, 0, 0)
}

/// jrnl 1.x plaintext: dates without brackets.
pub fn parse_legacy(text: &str, timeformat: &str, symbols: &str) -> Vec<Entry> {
    let date_len = chrono::Local::now().format(timeformat).to_string().len();
    let mut entries: Vec<Entry> = vec![];
    let mut current: Option<(NaiveDateTime, String)> = None;
    for line in text.lines() {
        let line = line.trim_end();
        let prefix: String = line.chars().take(date_len).collect();
        if let Ok(d) = NaiveDateTime::parse_from_str(&prefix, timeformat) {
            if let Some((date, raw)) = current.take() {
                entries.push(Entry::new(date, &raw, false, symbols));
            }
            let rest: String = line.chars().skip(date_len + 1).collect();
            current = Some((d, format!("{rest}\n")));
        } else if let Some((_, raw)) = current.as_mut() {
            // Escape bracketed lines so they aren't misread after conversion.
            if line.starts_with('[') {
                raw.push(' ');
            }
            raw.push_str(line);
            raw.push('\n');
        }
    }
    if let Some((date, raw)) = current {
        entries.push(Entry::new(date, &raw, false, symbols));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_split() {
        assert_eq!(
            split_title("Called in sick. Wrote my book."),
            ("Called in sick.".into(), "Wrote my book.".into())
        );
        assert_eq!(
            split_title("Line one\nLine two"),
            ("Line one".into(), "Line two".into())
        );
    }
    #[test]
    fn tags_not_emails() {
        assert_eq!(
            parse_tags("mail me at test@example.com or @home", "#@"),
            vec!["@home"]
        );
    }
    #[test]
    fn star_stripped() {
        let d = dates::parse("now", false, 9, 0).unwrap();
        let e = Entry::new(d, "Best day ever.*", false, "@");
        assert!(e.starred);
        assert_eq!(e.title, "Best day ever.");
    }
}
