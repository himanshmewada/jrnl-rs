use crate::{
    args::Args,
    config::Settings,
    entry::{Entry, tag_regex},
    journal::Journal,
    ui,
};
use anyhow::{Result, bail};
use chrono::{Datelike, Local, NaiveDate};
use colored::{Color, Colorize};
use std::collections::BTreeMap;
use std::str::FromStr;

fn color(name: &str) -> Option<Color> {
    if name.eq_ignore_ascii_case("none") {
        None
    } else {
        Color::from_str(&name.to_lowercase()).ok()
    }
}

fn paint(text: &str, c: &str, bold: bool) -> String {
    match color(c) {
        Some(c) => {
            if bold {
                text.color(c).bold().to_string()
            } else {
                text.color(c).to_string()
            }
        }
        None => text.to_string(),
    }
}

/// Colorize text, highlighting tags with the tags color.
fn highlight(text: &str, base: &str, s: &Settings, bold: bool) -> String {
    if !s.highlight {
        return paint(text, base, bold);
    }
    let re = tag_regex(&s.tagsymbols);
    let mut out = String::new();
    let mut last = 0;
    for c in re.captures_iter(text) {
        let m = c.get(1).unwrap();
        out.push_str(&paint(&text[last..m.start()], base, bold));
        out.push_str(&paint(m.as_str(), &s.colors.tags, true));
        last = m.end();
    }
    out.push_str(&paint(&text[last..], base, bold));
    out
}

fn wrap(text: &str, width: Option<usize>) -> Vec<String> {
    match width {
        Some(w) if w > 0 => textwrap::wrap(text, w)
            .iter()
            .map(|s| s.to_string())
            .collect(),
        _ => text.lines().map(String::from).collect(),
    }
}

// ---------------- pretty / short ----------------
pub fn pretty_entry(e: &Entry, s: &Settings, short: bool) -> String {
    let datestr = e.date.format(&s.timeformat).to_string();
    let width = s.linewrap.width();
    let head = format!("{datestr} {}", e.title);
    let mut lines = wrap(&head, width).into_iter();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        let rest = first.get(datestr.len()..).unwrap_or("");
        out.push_str(&paint(&datestr, &s.colors.date, true));
        out.push_str(&highlight(rest, &s.colors.title, s, true));
        out.push('\n');
    }
    for l in lines {
        out.push_str(&highlight(&l, &s.colors.title, s, true));
        out.push('\n');
    }
    if short || e.body.trim().is_empty() {
        return out.trim_end().to_string();
    }

    let indent = if s.indent_character.is_empty() {
        String::new()
    } else {
        format!("{} ", s.indent_character.trim_end())
    };
    let body_width = width.map(|w| w.saturating_sub(indent.len()).max(1));
    for line in wrap(e.body.trim_end(), body_width) {
        let painted = highlight(&line, &s.colors.body, s, false);
        out.push_str(&paint(&indent, &s.colors.body, false));
        out.push_str(&painted);
        out.push('\n');
    }
    out.trim_end().to_string()
}

pub fn pretty(j: &Journal, short: bool) -> String {
    j.entries
        .iter()
        .map(|e| pretty_entry(e, &j.settings, short))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------- data formats ----------------
fn entry_json(e: &Entry) -> serde_json::Value {
    let mut v = serde_json::json!({
        "title": e.title, "body": e.body,
        "date": e.date.format("%Y-%m-%d").to_string(),
        "time": e.date.format("%H:%M").to_string(),
        "tags": e.tags, "starred": e.starred,
    });
    if let Some(u) = &e.uuid {
        v["uuid"] = u.clone().into();
    }
    v
}

pub fn json(j: &Journal) -> String {
    let mut tags = serde_json::Map::new();
    for (t, n) in tag_counts(j) {
        tags.insert(t, n.into());
    }
    serde_json::to_string_pretty(&serde_json::json!({
        "tags": tags,
        "entries": j.entries.iter().map(entry_json).collect::<Vec<_>>(),
    }))
    .unwrap()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn xml(j: &Journal) -> String {
    let mut out = String::from("<?xml version=\"1.0\" ?>\n<journal>\n  <entries>\n");
    for e in &j.entries {
        out.push_str(&format!(
            "    <entry date=\"{}\" starred=\"{}\">\n",
            e.date.format("%Y-%m-%dT%H:%M:%S"),
            if e.starred { "true" } else { "" }
        ));
        for t in &e.tags {
            out.push_str(&format!("      <tag name=\"{}\"/>\n", xml_escape(t)));
        }
        out.push_str(&format!(
            "      {}\n    </entry>\n",
            xml_escape(&e.fulltext())
        ));
    }
    out.push_str("  </entries>\n  <tags>\n");
    for (t, n) in tag_counts(j) {
        out.push_str(&format!("    <tag name=\"{}\">{n}</tag>\n", xml_escape(&t)));
    }
    out.push_str("  </tags>\n</journal>\n");
    out
}

pub fn markdown(j: &Journal) -> String {
    let mut out: Vec<String> = vec![];
    let (mut year, mut month) = (0, 0);
    for e in &j.entries {
        if e.date.year() != year {
            year = e.date.year();
            out.push(format!("# {year}"));
            out.push(String::new());
        }
        if e.date.month() != month {
            month = e.date.month();
            out.push(format!("## {}", e.date.format("%B")));
            out.push(String::new());
        }
        out.push(format!(
            "### {} {}",
            e.date.format(&j.settings.timeformat),
            e.title
        ));
        out.push(shift_headings(&e.body, "###"));
        out.push(String::new());
    }
    out.join("\n")
}

fn shift_headings(body: &str, prefix: &str) -> String {
    let mut out = String::new();
    let mut prev = String::new();
    for line in body.lines() {
        let t = line.trim_end();
        if regex::Regex::new(r"^#+ ").unwrap().is_match(t) {
            out.push_str(&format!("{prefix}{line}\n"));
        } else if !t.is_empty() && t.chars().all(|c| c == '=') && !prev.trim().is_empty() {
            out.push_str(&format!("{prefix}# {prev}\n"));
        } else if !t.is_empty() && t.chars().all(|c| c == '-') && !prev.trim().is_empty() {
            out.push_str(&format!("{prefix}## {prev}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
        prev = line.to_string();
    }
    out
}

pub fn yaml_entry(e: &Entry, s: &Settings) -> String {
    let tags = e
        .tags
        .iter()
        .map(|t| t.trim_start_matches(|c| s.tagsymbols.contains(c)))
        .collect::<Vec<_>>()
        .join(", ");
    let body: String = e.body.lines().map(|l| format!("\t\t{l}\n")).collect();
    format!(
        "---\ntitle: {}\ndate: {}\nstarred: {}\ntags: {}\nbody: |\n{}...\n",
        e.title,
        e.date.format(&s.timeformat),
        e.starred,
        tags,
        body
    )
}

pub fn tag_counts(j: &Journal) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &j.entries {
        for t in e.tags.iter().collect::<std::collections::HashSet<_>>() {
            *counts.entry(t.clone()).or_default() += 1;
        }
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

pub fn tags_report(j: &Journal) -> String {
    let counts = tag_counts(j);
    if counts.is_empty() {
        return "[No tags found in journal.]".into();
    }
    counts
        .iter()
        .map(|(t, n)| format!("{t:<20} : {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn dates_report(j: &Journal) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &j.entries {
        *counts
            .entry(e.date.format("%Y-%m-%d").to_string())
            .or_default() += 1;
    }
    counts
        .iter()
        .map(|(d, n)| format!("{d}, {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------- fancy ----------------
pub fn fancy_entry(e: &Entry, s: &Settings) -> Result<String> {
    let date = e.date.format(&s.timeformat).to_string();
    let width = s.linewrap.width().unwrap_or(79);
    if date.len() + 4 > width {
        bail!(
            "The provided linewrap value of {width} is too small to display the timestamps for journal."
        );
    }
    let inner = width - 2;
    let title_w = width - date.len() - 2;
    let mut card = vec![format!("┎{}╮{date}", "─".repeat(title_w))];
    let tw = textwrap::Options::new(title_w)
        .initial_indent("┃ ")
        .subsequent_indent("┃ ");
    let title_lines = textwrap::wrap(&e.title, tw);
    let first = title_lines.first().cloned().unwrap_or_else(|| "┃ ".into());
    card.push(format!(
        "{}╘{}╕",
        pad(&first, title_w + 2),
        "═".repeat(date.len().saturating_sub(1))
    ));
    if !e.body.trim().is_empty() {
        card.push(format!("┠{}┤", "╌".repeat(inner)));
        for line in e.body.lines() {
            let wrapped = textwrap::wrap(
                line,
                textwrap::Options::new(inner)
                    .initial_indent("┃ ")
                    .subsequent_indent("┃ "),
            );
            if wrapped.is_empty() {
                card.push(format!("┃{}│", " ".repeat(inner)));
            }
            for w in wrapped {
                card.push(format!("{}│", pad(&w, inner + 2)));
            }
        }
    }
    card.push(format!("┖{}┘", "─".repeat(inner)));
    Ok(card.join("\n"))
}

fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

// ---------------- calendar heatmap ----------------
pub fn calendar(j: &Journal) -> String {
    if j.entries.is_empty() {
        return "[No entries found.]".into();
    }
    let mut counts: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    for e in &j.entries {
        *counts.entry(e.date.date()).or_default() += 1;
    }
    let first = j.entries.first().unwrap().date.date();
    let now = Local::now().date_naive();
    let mut out = String::new();
    for year in first.year()..=now.year() {
        out.push_str(&format!("\n────────── {year} ──────────\n"));
        for month in 1..=12 {
            if year == now.year() && month > now.month() {
                break;
            }
            let Some(first_day) = NaiveDate::from_ymd_opt(year, month, 1) else {
                continue;
            };
            let days = (1..=31)
                .filter_map(|d| NaiveDate::from_ymd_opt(year, month, d))
                .count() as u32;
            let total: usize = (1..=days)
                .map(|d| {
                    counts
                        .get(&NaiveDate::from_ymd_opt(year, month, d).unwrap())
                        .copied()
                        .unwrap_or(0)
                })
                .sum();
            if total == 0 {
                continue;
            }
            out.push_str(&format!(
                "\n{} {year} ({total} entries)\n Mo Tu We Th Fr Sa Su\n",
                first_day.format("%B")
            ));
            let lead = first_day.weekday().num_days_from_monday();
            for _ in 0..lead {
                out.push_str("   ");
            }
            for d in 1..=days {
                let date = NaiveDate::from_ymd_opt(year, month, d).unwrap();
                let n = counts.get(&date).copied().unwrap_or(0);
                let cell = format!("{d:>2}");
                let cell = if date > now {
                    format!("{:>2}", "")
                } else {
                    match n {
                        0 => cell.red().to_string(),
                        1 => cell.yellow().bold().to_string(),
                        2 => cell.green().bold().to_string(),
                        _ => cell.white().bold().to_string(),
                    }
                };
                out.push_str(&cell);
                out.push(if date.weekday().num_days_from_monday() == 6 {
                    '\n'
                } else {
                    ' '
                });
            }
            out.push('\n');
        }
    }
    out
}

// ---------------- dispatch ----------------
pub fn render(j: &Journal, fmt: &str) -> Result<String> {
    Ok(match fmt {
        "pretty" => pretty(j, false),
        "short" => pretty(j, true),
        "text" | "txt" => j.serialize(),
        "json" => json(j),
        "md" | "markdown" => markdown(j),
        "xml" => xml(j),
        "tags" => tags_report(j),
        "dates" => dates_report(j),
        "fancy" | "boxed" => j
            .entries
            .iter()
            .map(|e| fancy_entry(e, &j.settings))
            .collect::<Result<Vec<_>>>()?
            .join("\n"),
        "calendar" | "heatmap" => calendar(j),
        "yaml" => bail!("YAML export must be to a directory, not a single file"),
        _ => bail!("unknown format '{fmt}'"),
    })
}

fn render_entry(e: &Entry, s: &Settings, fmt: &str) -> Result<String> {
    Ok(match fmt {
        "pretty" => pretty_entry(e, s, false),
        "short" => pretty_entry(e, s, true),
        "text" | "txt" => e.to_storage(&s.timeformat),
        "json" => serde_json::to_string_pretty(&entry_json(e))?,
        "md" | "markdown" => format!(
            "# {} {}\n{}",
            e.date.format(&s.timeformat),
            e.title,
            shift_headings(&e.body, "#")
        ),
        "yaml" => yaml_entry(e, s),
        "fancy" | "boxed" => fancy_entry(e, s)?,
        "xml" => format!(
            "<entry date=\"{}\" starred=\"{}\">{}</entry>",
            e.date.format("%Y-%m-%dT%H:%M:%S"),
            e.starred,
            xml_escape(&e.fulltext())
        ),
        _ => bail!("format '{fmt}' cannot export individual entries"),
    })
}

fn slug(s: &str) -> String {
    let lowered = s.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else if c.is_whitespace() {
                '-'
            } else {
                '\0'
            }
        })
        .filter(|c| *c != '\0')
        .collect();
    let mut out = String::new();
    for part in cleaned.split('-').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(part);
    }
    out
}

fn extension(fmt: &str) -> &'static str {
    match fmt {
        "json" => "json",
        "md" | "markdown" | "yaml" => "md",
        "xml" => "xml",
        _ => "txt",
    }
}

/// Print, write to a file, or export per-entry files into a directory.
pub fn display(j: &Journal, args: &Args) -> Result<()> {
    if j.entries.is_empty() {
        return Ok(());
    }
    let fmt = if args.tags {
        "tags".to_string()
    } else if args.short {
        "short".to_string()
    } else {
        args.export
            .clone()
            .or_else(|| j.settings.display_format.clone())
            .unwrap_or_else(|| "pretty".into())
    };

    match &args.filename {
        None => println!("{}", render(j, &fmt)?),
        Some(dest) => {
            let p = crate::path::expand(dest);
            if p.is_dir() {
                for e in &j.entries {
                    let mut name = format!(
                        "{}_{}.{}",
                        e.date.format("%Y-%m-%d"),
                        slug(&e.title),
                        extension(&fmt)
                    );
                    // Shrink absurdly long titles instead of failing.
                    while name.len() > 200 {
                        name = name[..name.len() / 2].to_string() + "." + extension(&fmt);
                    }
                    crate::path::atomic_write(
                        &p.join(name),
                        render_entry(e, &j.settings, &fmt)?.as_bytes(),
                    )?;
                }
                ui::normal(&format!("Journal exported to {}", p.display()));
            } else {
                std::fs::write(&p, render(j, &fmt)?)?;
                ui::normal(&format!("Journal exported to {}", p.display()));
            }
        }
    }
    Ok(())
}
