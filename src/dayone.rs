use crate::{config::Settings, entry::Entry, path};
use anyhow::Result;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use std::time::SystemTime;

pub fn read(root: &std::path::Path, settings: &Settings) -> Result<Vec<Entry>> {
    let mut entries = vec![];
    let entries_dir = root.join("entries");
    let dirs_to_scan = if entries_dir.is_dir() {
        vec![entries_dir]
    } else {
        vec![root.to_path_buf()]
    };
    for dir in dirs_to_scan {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for f in rd.flatten() {
            let p = f.path();
            if p.extension().map(|e| e != "doentry").unwrap_or(true) {
                continue;
            }
            let Ok(value) = plist::Value::from_file(&p) else {
                continue;
            };
            let Some(dict) = value.as_dictionary() else {
                continue;
            };
            let Some(text) = dict.get("Entry Text").and_then(|v| v.as_string()) else {
                continue;
            };
            let Some(date_val) = dict.get("Creation Date").and_then(|v| v.as_date()) else {
                continue;
            };
            let st: SystemTime = date_val.clone().into();
            let utc: DateTime<Utc> = st.into();
            let tz_name = dict
                .get("Time Zone")
                .and_then(|v| v.as_string())
                .unwrap_or("UTC");
            let naive: NaiveDateTime = match tz_name.parse::<chrono_tz::Tz>() {
                Ok(tz) => tz.from_utc_datetime(&utc.naive_utc()).naive_local(),
                Err(_) => utc.naive_utc(),
            };
            let starred = dict
                .get("Starred")
                .and_then(|v| v.as_boolean())
                .unwrap_or(false);
            let mut e = Entry::new(naive, text, starred, &settings.tagsymbols);
            e.uuid = dict
                .get("UUID")
                .and_then(|v| v.as_string())
                .map(|s| s.to_uppercase());
            if let Some(tags) = dict.get("Tags").and_then(|v| v.as_array()) {
                let sym = settings.tagsymbols.chars().next().unwrap_or('@');
                let mut tags: Vec<String> = tags
                    .iter()
                    .filter_map(|t| t.as_string())
                    .map(|t| format!("{sym}{}", t.to_lowercase()))
                    .collect();
                tags.sort();
                if !tags.is_empty() {
                    e.tags = tags;
                }
            }
            e.meta = Some(value);
            entries.push(e);
        }
    }
    Ok(entries)
}

pub fn write(
    root: &std::path::Path,
    entries: &[Entry],
    deleted_uuids: &[String],
    settings: &Settings,
) -> Result<()> {
    let dir = root.join("entries");
    std::fs::create_dir_all(&dir)?;
    for e in entries.iter().filter(|e| e.modified) {
        let uuid = e
            .uuid
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string().to_uppercase());
        let mut dict = e
            .meta
            .as_ref()
            .and_then(|m| m.as_dictionary().cloned())
            .unwrap_or_default();

        // Convert local naive date back to UTC using the entry's zone (or local zone).
        let tz_name = dict
            .get("Time Zone")
            .and_then(|v| v.as_string())
            .map(String::from);
        let utc: DateTime<Utc> = match tz_name
            .as_deref()
            .and_then(|t| t.parse::<chrono_tz::Tz>().ok())
        {
            Some(tz) => tz
                .from_local_datetime(&e.date)
                .single()
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|| local_to_utc(e.date)),
            None => local_to_utc(e.date),
        };
        dict.insert(
            "Creation Date".into(),
            plist::Value::Date(plist::Date::from(SystemTime::from(utc))),
        );
        dict.insert("Starred".into(), e.starred.into());
        dict.insert(
            "Entry Text".into(),
            format!("{}\n{}", e.title, e.body).into(),
        );
        dict.insert("UUID".into(), uuid.to_uppercase().into());
        if dict.get("Time Zone").is_none() {
            dict.insert("Time Zone".into(), "UTC".into());
        }
        let sym: &str = &settings.tagsymbols;
        let tags: Vec<plist::Value> = e
            .tags
            .iter()
            .map(|t| {
                t.trim_start_matches(|c| sym.contains(c))
                    .replace('_', " ")
                    .into()
            })
            .collect();
        dict.insert("Tags".into(), plist::Value::Array(tags));
        // was: dict.entry("Creator".into()).or_insert_with(|| { ... });
        if dict.get("Creator").is_none() {
            let mut c = plist::Dictionary::new();
            c.insert("Device Agent".into(), "".into());
            c.insert(
                "Host Name".into(),
                std::env::var("HOSTNAME")
                    .or_else(|_| std::env::var("COMPUTERNAME"))
                    .unwrap_or_else(|_| "localhost".into())
                    .into(),
            );
            c.insert("OS Agent".into(), std::env::consts::OS.into());
            c.insert(
                "Software Agent".into(),
                format!("diary/{}", env!("CARGO_PKG_VERSION")).into(),
            );
            dict.insert("Creator".into(), plist::Value::Dictionary(c));
        }

        let mut buf = Vec::new();
        plist::Value::Dictionary(dict).to_writer_xml(&mut buf)?;
        path::atomic_write(&dir.join(format!("{uuid}.doentry")), &buf)?;
    }
    for uuid in deleted_uuids {
        let _ = std::fs::remove_file(dir.join(format!("{uuid}.doentry")));
    }
    Ok(())
}

fn local_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now())
}
