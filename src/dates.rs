//! Natural-language date parsing (parsedatetime/dateutil equivalent).
use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

pub fn month_num(s: &str) -> Option<u32> {
    let l = s.to_lowercase();
    if let Ok(n) = l.parse::<u32>() {
        return (1..=12).contains(&n).then_some(n);
    }
    (l.len() >= 3)
        .then(|| {
            MONTHS
                .iter()
                .position(|m| m.starts_with(&l))
                .map(|i| i as u32 + 1)
        })
        .flatten()
}

pub fn year_num(s: &str) -> Option<i32> {
    s.parse::<i32>()
        .ok()
        .map(|y| if y < 100 { y + 2000 } else { y })
}

fn default_time(d: NaiveDate, inclusive: bool, dh: u32, dm: u32) -> NaiveDateTime {
    if inclusive {
        d.and_hms_opt(23, 59, 59).unwrap()
    } else {
        d.and_hms_opt(dh, dm, 0).unwrap()
    }
}

fn extract_time(s: &str) -> (Option<(u32, u32)>, String) {
    let re = Regex::new(r"\b(\d{1,2}):(\d{2})(?::\d{2})?\s*(am|pm)?\b").unwrap();
    if let Some(c) = re.captures(s) {
        let mut h: u32 = c[1].parse().unwrap_or(0);
        let m: u32 = c[2].parse().unwrap_or(0);
        match c.get(3).map(|x| x.as_str()) {
            Some("pm") if h < 12 => h += 12,
            Some("am") if h == 12 => h = 0,
            _ => {}
        }
        if h < 24 && m < 60 {
            return (Some((h, m)), re.replace(s, " ").into_owned());
        }
    }
    let re = Regex::new(r"\b(\d{1,2})\s*(am|pm)\b").unwrap();
    if let Some(c) = re.captures(s) {
        let mut h: u32 = c[1].parse().unwrap_or(0);
        if &c[2] == "pm" && h < 12 {
            h += 12;
        }
        if &c[2] == "am" && h == 12 {
            h = 0;
        }
        if h < 24 {
            return (Some((h, 0)), re.replace(s, " ").into_owned());
        }
    }
    if s.contains("noon") {
        return (Some((12, 0)), s.replace("noon", " "));
    }
    if s.contains("midnight") {
        return (Some((0, 0)), s.replace("midnight", " "));
    }
    (None, s.to_string())
}

fn parse_weekday(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let s = s.strip_prefix("last ").unwrap_or(s);
    let target = DAYS.iter().position(|d| d.starts_with(s) && s.len() >= 3)? as u32;
    let cur = today.weekday().num_days_from_monday();
    let back = (cur + 7 - target) % 7;
    Some(today - Duration::days(back as i64))
}

fn parse_relative(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let re = Regex::new(r"^(in )?(\d+) (day|week|month|year)s?( ago)?$").unwrap();
    let c = re.captures(s)?;
    let n: u32 = c[2].parse().ok()?;
    let future = c.get(1).is_some();
    let sign = if future { 1 } else { -1 };
    Some(match &c[3] {
        "day" => today + Duration::days(sign * n as i64),
        "week" => today + Duration::weeks(sign * n as i64),
        "month" => {
            if future {
                today.checked_add_months(Months::new(n))?
            } else {
                today.checked_sub_months(Months::new(n))?
            }
        }
        _ => {
            if future {
                today.checked_add_months(Months::new(n * 12))?
            } else {
                today.checked_sub_months(Months::new(n * 12))?
            }
        }
    })
}

fn parse_words(s: &str, today: NaiveDate) -> Option<(NaiveDate, bool)> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut month = None;
    let mut nums: Vec<i64> = vec![];
    for t in &tokens {
        if let Some(m) = month_num(t) {
            month = Some(m);
        } else if let Ok(n) = t.parse::<i64>() {
            nums.push(n);
        } else if *t != "of" && *t != "the" {
            return None;
        }
    }
    if let Some(m) = month {
        let day = nums
            .iter()
            .find(|n| **n >= 1 && **n <= 31)
            .copied()
            .unwrap_or(1);
        let year = nums
            .iter()
            .find(|n| **n > 31 || **n < 1)
            .copied()
            .or_else(|| nums.iter().filter(|n| **n != day).copied().next());
        let explicit = year.is_some();
        let y = year
            .map(|y| if y < 100 { y + 2000 } else { y })
            .unwrap_or(today.year() as i64);
        let d = NaiveDate::from_ymd_opt(y as i32, m, day as u32)?;
        return Some((d, explicit));
    }
    if nums.len() == 1 && nums[0] >= 1000 {
        return Some((NaiveDate::from_ymd_opt(nums[0] as i32, 1, 1)?, true));
    }
    None
}

pub fn parse(input: &str, inclusive: bool, dh: u32, dm: u32) -> Option<NaiveDateTime> {
    let raw = input.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_lowercase();
    if lower == "now" {
        return Some(Local::now().naive_local());
    }

    let cleaned = lower.replace(" at ", " ");
    for f in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dt%H:%M:%S",
        "%Y-%m-%dt%H:%M",
        "%m/%d/%Y %H:%M",
        "%d/%m/%Y %H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&cleaned, f) {
            return Some(dt);
        }
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Local).naive_local());
    }
    for f in ["%Y-%m-%d", "%m/%d/%Y", "%d/%m/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(&cleaned, f) {
            return Some(default_time(d, inclusive, dh, dm));
        }
    }

    let (time, rest) = extract_time(&cleaned);
    let rest = rest.trim().trim_end_matches(':').trim().to_string();
    let today = Local::now().date_naive();
    let mut year_explicit = false;

    let date = if rest.is_empty() || rest == "today" {
        today
    } else if rest == "yesterday" {
        today - Duration::days(1)
    } else if rest == "tomorrow" {
        today + Duration::days(1)
    } else if let Some(d) = parse_weekday(&rest, today) {
        d
    } else if let Some(d) = parse_relative(&rest, today) {
        d
    } else if let Some((d, ye)) = parse_words(&rest, today) {
        year_explicit = ye;
        d
    } else {
        return None;
    };

    let (h, m) = time.unwrap_or(if inclusive { (23, 59) } else { (dh, dm) });
    let sec = if inclusive && time.is_none() { 59 } else { 0 };
    let mut dt = date.and_time(NaiveTime::from_hms_opt(h, m, sec)?);
    // Prefer past dates when the year was guessed (jrnl heuristic).
    if !year_explicit && dt > Local::now().naive_local() + Duration::days(28) {
        dt = dt.with_year(dt.year() - 1).unwrap_or(dt);
    }
    Some(dt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iso_with_default_time() {
        let d = parse("2020-06-20", false, 9, 0).unwrap();
        assert_eq!(
            d,
            NaiveDate::from_ymd_opt(2020, 6, 20)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap()
        );
    }
    #[test]
    fn yesterday() {
        assert_eq!(
            parse("yesterday", false, 9, 0).unwrap().date(),
            Local::now().date_naive() - Duration::days(1)
        );
    }
}
