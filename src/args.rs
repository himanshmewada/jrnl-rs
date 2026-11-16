//! Hand-rolled parser matching jrnl's unusual CLI: single-dash long flags,
//! intermixed flags/text, `-N` shorthand, `-not -starred`, optional values.

#[derive(Clone, Copy, PartialEq)]
pub enum Preconfig {
    Help,
    Version,
    Diagnostic,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Postconfig {
    List,
    Encrypt,
    Decrypt,
    Import,
}

#[derive(Default)]
pub struct Args {
    pub text: Vec<String>,
    pub debug: bool,
    pub preconfig: Option<Preconfig>,
    pub postconfig: Option<Postconfig>,
    pub filename: Option<String>,
    pub template: Option<String>,
    pub on: Option<String>,
    pub today_in_history: bool,
    pub month: Option<String>,
    pub day: Option<String>,
    pub year: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub contains: Vec<String>,
    pub strict: bool,
    pub starred: bool,
    pub tagged: bool,
    pub limit: Option<usize>,
    pub excluded: Vec<String>,
    pub exclude_starred: bool,
    pub exclude_tagged: bool,
    pub edit: bool,
    pub delete: bool,
    pub change_time: Option<String>,
    pub export: Option<String>,
    pub tags: bool,
    pub short: bool,
    pub config_override: Vec<(String, String)>,
    pub config_file: Option<String>,
    // Extras beyond jrnl:
    pub all: bool,
    pub random: bool,
    pub stats: bool,
    pub count: bool,
    pub backup: bool,
    pub grep: Option<String>,
}

pub const FORMATS: &[&str] = &[
    "pretty", "short", "text", "txt", "json", "md", "markdown", "xml", "yaml", "tags", "dates",
    "fancy", "boxed", "calendar", "heatmap",
];

pub fn parse(raw: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    let mut only_text = false;

    macro_rules! value {
        ($flag:expr) => {{
            i += 1;
            raw.get(i)
                .cloned()
                .ok_or_else(|| format!("argument {}: expected a value", $flag))?
        }};
    }

    while i < raw.len() {
        let tok = raw[i].clone();
        if only_text || !tok.starts_with('-') || tok == "-" {
            a.text.push(tok);
            i += 1;
            continue;
        }
        if tok == "--" {
            only_text = true;
            i += 1;
            continue;
        }
        // -123  =>  -n 123
        if let Some(n) = tok.strip_prefix('-').and_then(|s| s.parse::<usize>().ok()) {
            a.limit = Some(n);
            i += 1;
            continue;
        }
        let name = tok.trim_start_matches('-');
        match name {
            "help" | "h" => a.preconfig = Some(Preconfig::Help),
            "version" | "v" => a.preconfig = Some(Preconfig::Version),
            "diagnostic" => a.preconfig = Some(Preconfig::Diagnostic),
            "debug" => a.debug = true,
            "list" | "ls" => a.postconfig = Some(Postconfig::List),
            "encrypt" => a.postconfig = Some(Postconfig::Encrypt),
            "decrypt" => a.postconfig = Some(Postconfig::Decrypt),
            "import" => a.postconfig = Some(Postconfig::Import),
            "file" | "i" | "o" => a.filename = Some(value!(tok)),
            "template" => a.template = Some(value!(tok)),
            "on" => a.on = Some(value!(tok)),
            "today-in-history" | "on-this-day" => a.today_in_history = true,
            "month" => a.month = Some(value!(tok)),
            "day" => a.day = Some(value!(tok)),
            "year" => a.year = Some(value!(tok)),
            "from" => a.from = Some(value!(tok)),
            "to" | "until" => a.to = Some(value!(tok)),
            "contains" => a.contains.push(value!(tok)),
            "and" => a.strict = true,
            "starred" => a.starred = true,
            "tagged" => a.tagged = true,
            "n" => {
                a.limit = Some(usize::MAX);
                if let Some(next) = raw.get(i + 1) {
                    if let Ok(n) = next.parse::<usize>() {
                        a.limit = Some(n);
                        i += 1;
                    }
                }
            }
            "not" => match raw.get(i + 1).map(String::as_str) {
                Some("-starred") | Some("--starred") => {
                    a.exclude_starred = true;
                    i += 1;
                }
                Some("-tagged") | Some("--tagged") => {
                    a.exclude_tagged = true;
                    i += 1;
                }
                Some(v) if !v.starts_with('-') => {
                    a.excluded.push(v.to_string());
                    i += 1;
                }
                _ => return Err("argument -not: expected 1 argument".into()),
            },
            "edit" => a.edit = true,
            "delete" => a.delete = true,
            "change-time" => {
                a.change_time = Some("now".into());
                if let Some(next) = raw.get(i + 1) {
                    if !next.starts_with('-') {
                        a.change_time = Some(next.clone());
                        i += 1;
                    }
                }
            }
            "format" | "export" => {
                let f = value!(tok).to_lowercase();
                if !FORMATS.contains(&f.as_str()) {
                    return Err(format!(
                        "unknown format '{f}'. Valid: {}",
                        FORMATS.join(", ")
                    ));
                }
                a.export = Some(f);
            }
            "tags" => a.tags = true,
            "short" | "s" => a.short = true,
            "config-override" | "co" => {
                let k = value!(tok);
                let v = value!(tok);
                a.config_override.push((k, v));
            }
            "config-file" | "cf" => a.config_file = Some(value!(tok)),
            // Extras:
            "all" => a.all = true,
            "random" => a.random = true,
            "stats" => a.stats = true,
            "count" => a.count = true,
            "backup" => a.backup = true,
            "grep" | "regex" => a.grep = Some(value!(tok)),
            _ => return Err(format!("unrecognized argument: {tok}")),
        }
        i += 1;
    }
    Ok(a)
}

pub fn help() -> &'static str {
    r#"jrnl (diary, Rust edition) — collect your thoughts without leaving the command line.

WRITING
  jrnl yesterday: Called in sick. Used the time to write my book.
  jrnl *Best day of my life.            Star an entry
  jrnl                                  Compose in editor / prompt
  jrnl --template daily.md              Use a template
  jrnl < entry.txt                      Pipe an entry in

LISTING & SEARCHING
  jrnl --all                            ★ List ALL entries
  jrnl --all --short                    All entries, titles only
  jrnl --all --format json              All entries as JSON
  jrnl --list [--format json|yaml]      List configured journals
  jrnl -n 10 | jrnl -10                 Last 10 entries
  jrnl -on DATE | -from DATE | -to DATE | -until DATE
  jrnl -month M | -day D | -year Y | -today-in-history
  jrnl -contains TEXT [-contains ...] [-and]
  jrnl @tag1 @tag2 [-and] | -starred | -tagged
  jrnl -not @tag | -not -starred | -not -tagged
  jrnl --grep REGEX                     (extra) regex search

ACTIONS ON RESULTS
  jrnl ... --edit                       Edit in external editor
  jrnl ... --delete                     Interactively delete
  jrnl ... --change-time [DATE]         Interactively re-timestamp

FORMATS
  --format pretty|short|text|json|markdown|xml|yaml|tags|dates|fancy|calendar
  --file PATH                           Write to file (or per-entry files to a dir)
  --tags | --short

STANDALONE
  --encrypt | --decrypt                 Manage AES encryption (v3, reads v1/v2)
  --import [--file F]                   Import entries (stdin by default)
  --help | --version | --diagnostic | --debug

CONFIG
  --config-file PATH                    Alternate config for this run
  --config-override KEY VALUE           Dot-notation override (repeatable)

EXTRAS (beyond jrnl)
  --all                                 Explicitly list every entry
  --stats                               Word counts, streaks, top tags
  --random                              Show one random entry
  --count                               Print only the number of matches
  --backup                              Timestamped backup of the journal
  git: {enabled: true, push: false}     Per-journal git auto-commit
"#
}
