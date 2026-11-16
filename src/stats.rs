//! Extra feature: journal statistics & writing streaks.
use crate::journal::Journal;
use chrono::{Duration, Local, NaiveDate};
use std::collections::{BTreeMap, HashSet};

pub fn report(j: &Journal) -> String {
    if j.entries.is_empty() {
        return "No entries yet — write your first one!".into();
    }
    let words: usize = j
        .entries
        .iter()
        .map(|e| e.fulltext().split_whitespace().count())
        .sum();
    let days: HashSet<NaiveDate> = j.entries.iter().map(|e| e.date.date()).collect();
    let mut sorted: Vec<NaiveDate> = days.iter().copied().collect();
    sorted.sort();

    let (mut longest, mut run) = (1usize, 1usize);
    for w in sorted.windows(2) {
        run = if w[1] - w[0] == Duration::days(1) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
    }
    let today = Local::now().date_naive();
    let mut current = 0usize;
    let mut d = if days.contains(&today) {
        today
    } else {
        today - Duration::days(1)
    };
    while days.contains(&d) {
        current += 1;
        d -= Duration::days(1);
    }

    let mut per_year: BTreeMap<i32, usize> = BTreeMap::new();
    for e in &j.entries {
        *per_year
            .entry(e.date.format("%Y").to_string().parse().unwrap())
            .or_default() += 1;
    }

    let mut out = format!(
        "Entries:          {}\nWords:            {words}\nAvg words/entry:  {:.1}\nActive days:      {}\nFirst entry:      {}\nLast entry:       {}\nCurrent streak:   {current} day(s)\nLongest streak:   {longest} day(s)\nStarred:          {}\n\nEntries per year:\n",
        j.entries.len(),
        words as f64 / j.entries.len() as f64,
        days.len(),
        sorted.first().unwrap(),
        sorted.last().unwrap(),
        j.entries.iter().filter(|e| e.starred).count()
    );
    for (y, n) in per_year {
        out.push_str(&format!("  {y}: {n}\n"));
    }
    let tags = crate::export::tag_counts(j);
    if !tags.is_empty() {
        out.push_str("\nTop tags:\n");
        for (t, n) in tags.iter().take(10) {
            out.push_str(&format!("  {t:<20} {n}\n"));
        }
    }
    out
}
