# jrnl-rs

**Collect your thoughts without leaving the command line.**

A from-scratch Rust reimplementation of [jrnl](https://jrnl.sh) — the plaintext
diary for your terminal. One self-contained binary, no Python runtime, and full
compatibility with jrnl's journal format, config file, and v1/v2/v3 encryption.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust: edition 2024](https://img.shields.io/badge/rust-edition%202024-orange.svg)](Cargo.toml)

> Independent reimplementation. Not affiliated with, or endorsed by, the upstream
> jrnl project — but it reads and writes the same files.

---

## Contents

- [Why](#why)
- [Install](#install)
- [Quick start](#quick-start)
- [Writing entries](#writing-entries)
- [Searching](#searching)
- [Acting on results](#acting-on-results)
- [Exporting](#exporting)
- [Configuration](#configuration)
- [Encryption](#encryption)
- [Journal backends](#journal-backends)
- [Importing](#importing)
- [Git sync](#git-sync)
- [Extras beyond jrnl](#extras-beyond-jrnl)
- [CLI reference](#cli-reference)
- [Project layout](#project-layout)
- [Development](#development)
- [Security notes](#security-notes)
- [License](#license)

---

## Why

Your diary should be as fast as your shell. `jrnl-rs` keeps everything in
human-readable plaintext (or Fernet-encrypted ciphertext), so your entries
outlive the tool. `grep`, `git`, `rsync` and your favourite editor all work on it.

- ✍️ **Frictionless capture** — `jrnl yesterday: Called in sick. Wrote my book.`
- 🗓️ **Natural-language dates** — `last tuesday at 3pm`, `3 days ago`, `june 20`
- 🏷️ **Tags & stars** — `@work`, `#idea`, `*Best day ever.`
- 🔎 **Expressive search** — dates, tags, boolean `-and`, exclusions, regex
- 🔐 **Real encryption** — Fernet (AES-128-CBC + HMAC-SHA256), OS keyring
- 📚 **Three backends** — single file, `YYYY/MM/DD.txt` folders, Day One `.doentry`
- 📤 **11 export formats** — JSON, Markdown, XML, YAML, boxed cards, calendar heatmap
- 📊 **Stats & streaks**, random entry, counts, timestamped backups
- 🧬 **Git auto-commit** per journal
- 🛟 **Crash-safe writes** — every save is a temp-file + atomic rename

---

## Install

Requires **Rust 1.85+** (edition 2024).

```bash
git clone https://github.com/<you>/jrnl-rs
cd jrnl-rs
cargo install --path .
```

This installs a binary named **`jrnl`** into `~/.cargo/bin`.

Or build without installing:

```bash
cargo build --release
./target/release/jrnl --version
```

> **Name clash:** if you already have Python jrnl installed, both provide `jrnl`.
> Either uninstall the old one, or alias this build (`alias diary=~/.cargo/bin/jrnl`).
> They share the same config and journal files, so you can switch freely.

---

## Quick start

```bash
# First run asks where to keep your journal, whether to encrypt, and colors.
jrnl

# Write
jrnl yesterday: Called in sick. Used the time to write my book.
jrnl "*Best day of my life."          # quote the * so your shell doesn't glob it
jrnl 3pm: Shipped the thing. @work #win

# Read
jrnl --all --short                    # every entry, titles only
jrnl -n 5                             # last five
jrnl @work -and -starred
jrnl -from "last monday" -to today
jrnl -today-in-history
jrnl --format calendar

# Manage
jrnl --stats
jrnl --backup
jrnl --encrypt
```

Bare `jrnl` **composes** a new entry. To dump the whole journal, use `--all`.

---

## Writing entries

### Titles, bodies, stars, tags

The title is the first line — or, on a single line, everything up to the first
sentence-ending punctuation followed by whitespace. The rest is the body.

```
jrnl Called in sick. Used the time to write my book.
#    └── title ──┘ └──────────── body ────────────┘
```

- A leading or trailing `*` marks the entry **starred** and is stripped from the text.
- Tags start with `@` or `#` (configurable via `tagsymbols`), must follow
  whitespace or the start of the text, and are lowercased and de-duplicated.
  `test@example.com` is an email, not a tag.
- If a new tag looks like an existing one (normalized Levenshtein ≥ 0.8, same
  symbol), you get a warning with the closest matches.

### Date prefixes

Anything before a `: ` on the first line is parsed as a timestamp:

| Input | Meaning |
|---|---|
| `yesterday:` / `tomorrow:` / `today:` | relative days |
| `last tuesday:` / `fri:` | most recent weekday |
| `3 days ago:` / `2 weeks ago:` / `in 2 weeks:` | relative offsets |
| `june 20:` / `20 june 2019:` | month names or numbers |
| `2020-06-20:` / `2020-06-20 14:30:` | ISO dates |
| `06/20/2020:` | US-style dates |
| `3pm:` / `at 15:45:` / `noon:` / `midnight:` | time only, today |
| `2020-06-20T14:30:00+02:00:` | RFC 3339 |
| `now:` | this instant |

Date-only inputs use `default_hour` / `default_minute` (09:00 by default). If the
year was guessed and the result lands more than 28 days in the future, it's
pulled back a year — the same "prefer the past" heuristic jrnl uses.

### Composing

| Situation | What happens |
|---|---|
| `jrnl some text` | saved immediately |
| `jrnl` with `editor` configured | opens `$EDITOR` on a temp file |
| `jrnl` with no editor | prompts on stdin (finish with `Ctrl+D`, or `Ctrl+Z`+Enter on Windows) |
| `jrnl < entry.txt` / `cat note \| jrnl` | reads the piped text |
| `jrnl --template daily.md` | pre-fills the editor from a template |
| `jrnl --edit some text` | writes the text, then opens the editor |

Templates are looked up in the templates directory first, then as a normal path
(`~`, `$VAR`, `${VAR}` and `%VAR%` are expanded). Saving an unmodified template,
or empty text, is a no-op.

---

## Searching

Any search flag switches the tool from *writing* to *reading*. A message like
`3 entries found` goes to **stderr**, so stdout stays pipe-clean.

| Flag | Effect |
|---|---|
| `--all` | every entry |
| `-n N`, `-N` | last N entries (`-n` alone = all) |
| `-on DATE` | that day only |
| `-from DATE`, `-to DATE` / `-until DATE` | inclusive range |
| `-month M`, `-day D`, `-year Y` | calendar fields (`-month aug`, `-year 24`) |
| `-today-in-history` / `-on-this-day` | this month+day, any year |
| `-contains TEXT` | substring match; repeatable |
| `@tag #tag` | entries carrying those tags |
| `-and` | require **all** tags/terms instead of any |
| `-starred` / `-tagged` | only starred / only tagged |
| `-not @tag`, `-not -starred`, `-not -tagged` | exclusions |
| `--grep REGEX` / `--regex` | regular expression over title + body |

Filters compose:

```bash
jrnl -from 2024-01-01 -to 2024-06-30 @work -and -starred -not @admin -n 20
jrnl -contains coffee -contains tea -and --count
jrnl --grep '(?i)deploy(ed|ment)' --short
```

---

## Acting on results

Search first, then act. Each match is confirmed individually with a `[y/N]` prompt.

```bash
jrnl -on yesterday --edit                    # open matches in $EDITOR
jrnl @typo --delete                          # delete selected entries
jrnl -n 1 --change-time "2024-01-01 10:00"   # re-timestamp
jrnl -n 1 --change-time                      # defaults to "now"
```

Editing round-trips through the on-disk format, so you can retitle, re-tag,
re-date, split or merge entries. Wiping the buffer is refused (use `--delete`).
For Day One journals each entry carries a `# UUID` comment line — keep it and
metadata is preserved; remove it and that entry is deleted.

Afterwards you get a summary: `2 entries modified`, `1 entry deleted`, etc.

---

## Exporting

```bash
jrnl --all --format json
jrnl -from last-month --format markdown --file diary.md
jrnl @travel --format yaml --file ./export/     # one file per entry
```

| Format | Aliases | Output |
|---|---|---|
| `pretty` | *(default)* | colored, wrapped, indented |
| `short` | `-s`, `--short` | date + title only |
| `text` | `txt` | raw journal storage format |
| `json` | | entries + tag counts |
| `markdown` | `md` | year/month headings, shifted body headings |
| `xml` | | `<journal>` with entries and tags |
| `yaml` | | **directory export only**, one document per entry |
| `tags` | `--tags` | tag → count table |
| `dates` | | `YYYY-MM-DD, count` |
| `fancy` | `boxed` | Unicode card per entry |
| `calendar` | `heatmap` | month grids colored by entries/day |

`--file` (also `-i` / `-o`) writes to a file, or — if the path is an existing
directory — to one file per entry named `YYYY-MM-DD_title-slug.ext`. Long titles
are truncated rather than failing. Set `display_format` in config to change the
default.

Colors and wrapping respect `colors`, `highlight`, `linewrap` and
`indent_character`; ANSI is disabled automatically when stdout isn't a TTY.

---

## Configuration

Config is YAML, created on first run and auto-upgraded (missing keys are filled
in, `version` is bumped).

| | Linux | macOS | Windows |
|---|---|---|---|
| Config | `~/.config/jrnl/jrnl.yaml` | `~/Library/Application Support/jrnl/jrnl.yaml` | `%APPDATA%\jrnl\jrnl.yaml` |
| Default journal | `~/.local/share/jrnl/journal.txt` | `~/Library/Application Support/jrnl/journal.txt` | `%APPDATA%\jrnl\journal.txt` |
| Templates | `~/.local/share/jrnl/templates/` | `~/Library/Application Support/jrnl/templates/` | `%APPDATA%\jrnl\templates\` |

```yaml
version: 0.1.0

journals:
  default: ~/journal.txt
  work:
    journal: ~/work/diary/          # trailing slash → folder journal
    encrypt: true
    tagsymbols: "@"
    display_format: short
    git:
      enabled: true
      push: false
  memories:
    journal: ~/Day One/Journal.dayone
    timeformat: "%Y-%m-%d %H:%M"

editor: "code --wait"               # or vim, nano, "subl -w" …
encrypt: false                      # false | true | "jrnlv1" (legacy hint)
template: false                     # false | path to a template
default_hour: 9
default_minute: 0
timeformat: "%Y-%m-%d %H:%M"
tagsymbols: "#@"
highlight: true
linewrap: 79                        # number | "auto" | false
indent_character: "|"
display_format: pretty
colors:
  date: black
  title: cyan
  body: none
  tags: yellow
git:
  enabled: false
  push: false
```

Every top-level key can be overridden per journal. Select a journal by naming it
first:

```bash
jrnl work: Standup ran long again. @meetings
jrnl work -n 10 --short
jrnl --list                          # add --format json|yaml for machine output
```

Temporary overrides, without touching the file:

```bash
jrnl --config-override display_format short -n 3
jrnl --config-override colors.tags green --all
jrnl --config-file ./alt.yaml --all
```

`--config-override` (`-co`) takes a dot-notation key and a value, and is repeatable.

---

## Encryption

Only **single-file** journals can be encrypted. Folder and Day One journals are
many files and are left alone (you'll get a warning if `encrypt: true` is set on
one).

```bash
jrnl --encrypt                 # encrypt in place, prompt for a new password
jrnl --decrypt                 # back to plaintext
jrnl --encrypt --file copy.enc # encrypt to a new file, leave config untouched
```

You'll be asked whether to store the password in the OS keychain (macOS
Keychain, Windows Credential Manager, Linux Secret Service). Without a keyring
entry you get up to three password attempts per run.

### On-disk format (v3)

```
"JRNLv3" | u16 header length (BE) | base64(JSON header) | Fernet token
                                   └─ {"salt": urlsafe_b64(16 random bytes)}
```

- **KDF:** PBKDF2-HMAC-SHA256, 100 000 iterations, 32-byte key, random per-file salt
- **Fernet:** `0x80` ‖ 64-bit timestamp ‖ 16-byte IV ‖ AES-128-CBC/PKCS7 ‖ HMAC-SHA256
- Key halves: first 16 bytes sign, last 16 bytes encrypt — per the Fernet spec
- The HMAC is verified **before** decryption

### Legacy compatibility

| Version | Scheme | Support |
|---|---|---|
| v1 | AES-256-CBC, SHA-256(password) key, no MAC | read → auto-upgrade |
| v2 | Fernet with a fixed salt | read → auto-upgrade |
| v3 | Fernet with a random per-file salt | read + write |

Opening a v1/v2 journal warns you; the next save rewrites it as v3 and updates
the config. Set `encrypt: "jrnlv1"` if a legacy file isn't being detected.

> ⚠️ **There is no recovery.** Lose the password and the entries are gone.
> Run `jrnl --backup` before re-encrypting.

---

## Journal backends

| Kind | Detected by | Layout |
|---|---|---|
| **File** | a file path (default) | one text file, `[YYYY-MM-DD HH:MM] Title` blocks |
| **Folder** | existing directory, or path ending in `/` | `YYYY/MM/DD.txt`, one file per day |
| **Day One** | `*.dayone` directory or a dir containing `entries/` | `.doentry` XML plists |

- **File** journals are created (with their parent directories) on first use.
- **Folder** journals treat the path as authoritative for the date, so renaming
  a day file moves its entries. Empty day files are cleaned up after edits.
- **Day One** journals preserve UUIDs, `Starred`, `Tags`, `Time Zone` and the
  original plist metadata; timestamps are converted to UTC using the entry's own
  zone. New entries get a `Creator` block identifying this tool.

Storage format:

```
[2026-11-17 09:00] Called in sick. *
Used the time to write my book.

[2026-11-17 15:00] Shipped the thing.
Finally. @work #win
```

---

## Importing

```bash
jrnl --import --file old-journal.txt
cat backup.txt | jrnl work --import
```

Entries are parsed with your configured `timeformat` and de-duplicated against
existing ones by (date, title, body, starred), so re-importing is safe.

> `--file` is the shared I/O flag: it's the **input** for `--import` and the
> **output** for exports.

---

## Git sync

```yaml
journals:
  default:
    journal: ~/journal.txt
    git:
      enabled: true
      push: false
```

After every write, the journal's directory is versioned automatically:
`git init` if needed → `git add .` → `git commit -m "jrnl: update journal"` →
optional `git push` (a failed push warns but doesn't fail the write).

> `git add .` stages **everything** in that directory. Give your journal its own
> folder, or keep `push: false` and review commits yourself.

---

## Extras beyond jrnl

| Flag | What it does |
|---|---|
| `--all` | explicitly list every entry |
| `--stats` | entry/word counts, average length, active days, current & longest streak, starred count, entries per year, top 10 tags |
| `--random` | one random entry from the matches — a nice "on this day" alternative |
| `--count` | print just the number of matches (stdout, script-friendly) |
| `--backup` | timestamped copy: `journal.txt.backup-20261117-093000` (recursive for directories) |
| `--grep REGEX` | regular-expression search |

```bash
jrnl --stats
jrnl @gratitude --random
jrnl -from 2026-01-01 --count
jrnl --backup && jrnl --encrypt
```

Example `--stats` output:

```
Entries:          412
Words:            38210
Avg words/entry:  92.7
Active days:      287
First entry:      2024-03-02
Last entry:       2026-11-17
Current streak:   6 day(s)
Longest streak:   34 day(s)
Starred:          21
Entries per year:
  2024: 140
  2025: 198
  2026: 74

Top tags:
  @work                88
  @idea                41
```

---

## CLI reference

Flags accept one **or** two dashes (`-contains` ≡ `--contains`), may be
interleaved with entry text, and `--` stops flag parsing entirely.

```
WRITING
  jrnl [DATE:] TEXT            write an entry
  jrnl                         compose in editor / stdin
  --template PATH              pre-fill from a template
  --edit                       write, then open the editor

READING
  --all  -n N  -N              selection
  -on / -from / -to / -until   date range
  -month / -day / -year        calendar fields
  -today-in-history            this day, any year
  -contains TEXT  -and         substring search
  @tag  -starred  -tagged      tag / flag filters
  -not X  -not -starred        exclusions
  --grep REGEX                 regex search

ACTIONS
  --edit  --delete  --change-time [DATE]

OUTPUT
  --format FMT  --short  --tags
  --file PATH                  file, or directory for per-entry export

STANDALONE
  --list [--format json|yaml]
  --encrypt  --decrypt  --import
  --help  --version  --diagnostic  --debug

CONFIG
  --config-file PATH           (-cf)
  --config-override KEY VALUE  (-co, repeatable)

EXTRAS
  --all  --stats  --random  --count  --backup  --grep
```

Exit codes: `0` success · `1` runtime error · `2` bad arguments.

---

## Project layout

```
src/
├── main.rs         entry point, arg parsing, exit codes
├── args.rs         hand-rolled jrnl-compatible CLI parser + help text
├── config.rs       YAML load/install/upgrade, journal scoping, overrides
├── controller.rs   orchestration: write vs. search, actions, standalone commands
├── crypto.rs       PBKDF2 + Fernet, jrnl v1/v2/v3, keyring integration
├── dates.rs        natural-language date/time parsing
├── dayone.rs       Day One .doentry plist read/write
├── editor.rs       $EDITOR invocation, stdin capture, templates
├── entry.rs        Entry model, title/body split, tags, storage parse/serialize
├── export.rs       11 renderers, colors, wrapping, file/directory export
├── journal.rs      backends, filtering, editing, git sync, backups
├── path.rs         ~/$VAR expansion, atomic writes
├── stats.rs        word counts, streaks, per-year totals, top tags
└── ui.rs           prompts, colored output, debug macro
```

---

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets
cargo fmt
```

The crypto layer is split so the interesting parts are testable without a TTY or
a keyring: `encrypt_v3_with_password` and `decrypt_with` are pure functions, and
the interactive password/keyring handling is a thin wrapper on top. Existing
tests cover v3 round-trips (including non-ASCII), salt uniqueness, the legacy
raw-JSON header, date parsing, title splitting, tag extraction and starring.

Run with `--debug` for internal diagnostics on stderr.

---

## Security notes

Worth knowing before you trust it with anything sensitive:

- **v3 is authenticated.** HMAC-SHA256 is checked before any decryption, and
  each file gets a fresh random salt. The 100 000 PBKDF2 iterations match jrnl's
  format for compatibility — that's conservative by current standards.
- **v1 is weak** (unauthenticated AES-256-CBC with a plain SHA-256 key). It's
  supported for reading only, and upgraded to v3 on the next save.
- **Editing an encrypted journal uses a plaintext temp file** in your system
  temp directory, deleted after the editor exits. Same behaviour as upstream
  jrnl — be aware of it on shared machines or with tmp-on-disk setups.
- **Passwords are held in memory** for the lifetime of the process; keyring
  storage is opt-in.
- **Writes are atomic** (temp file in the same directory, then rename), so a
  crash mid-save won't truncate your journal. It is not a locking mechanism —
  don't run two writers against one journal concurrently.
- **Backups of encrypted journals are ciphertext copies.** Keep them somewhere
  safe, and keep the password somewhere else.

---

## License

MIT © 2026 Himansh Mewada — see [LICENSE](LICENSE).

Built as an independent Rust reimplementation of
[jrnl](https://jrnl.sh), originally by Manuel Ebert and contributors.
All credit for the format, the CLI design and the idea goes to them.
