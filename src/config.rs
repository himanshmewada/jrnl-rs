use crate::{path, ui};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Encrypt {
    Bool(bool),
    Version(String),
}
impl Encrypt {
    pub fn on(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Version(_) => true,
        }
    }
    pub fn v1_hint(&self) -> bool {
        matches!(self, Self::Version(v) if v == "jrnlv1")
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Template {
    #[allow(dead_code)]
    Bool(bool),
    Path(String),
}
impl Template {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Path(p) => Some(p),
            _ => None,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Linewrap {
    Num(usize),
    Bool(bool),
    Word(String),
}
impl Linewrap {
    pub fn width(&self) -> Option<usize> {
        match self {
            Self::Num(n) => Some(*n),
            Self::Bool(false) => None,
            Self::Word(w) if w == "auto" => terminal_size::terminal_size()
                .map(|(terminal_size::Width(w), _)| w as usize)
                .or(Some(79)),
            _ => Some(79),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Colors {
    #[serde(default = "none")]
    pub body: String,
    #[serde(default = "none")]
    pub date: String,
    #[serde(default = "none")]
    pub tags: String,
    #[serde(default = "none")]
    pub title: String,
}
fn none() -> String {
    "none".into()
}
impl Default for Colors {
    fn default() -> Self {
        Self {
            body: none(),
            date: none(),
            tags: none(),
            title: none(),
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GitCfg {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub push: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Settings {
    pub journal: String,
    #[serde(default)]
    pub editor: String,
    #[serde(default = "off")]
    pub encrypt: Encrypt,
    #[serde(default = "no_template")]
    pub template: Template,
    #[serde(default = "dh")]
    pub default_hour: u32,
    #[serde(default)]
    pub default_minute: u32,
    #[serde(default = "timeformat")]
    pub timeformat: String,
    #[serde(default = "tagsymbols")]
    pub tagsymbols: String,
    #[serde(default = "yes")]
    pub highlight: bool,
    #[serde(default = "wrap")]
    pub linewrap: Linewrap,
    #[serde(default = "indent")]
    pub indent_character: String,
    #[serde(default)]
    pub colors: Colors,
    #[serde(default)]
    pub display_format: Option<String>,
    #[serde(default)]
    pub git: GitCfg,
}
fn off() -> Encrypt {
    Encrypt::Bool(false)
}
fn no_template() -> Template {
    Template::Bool(false)
}
fn dh() -> u32 {
    9
}
fn yes() -> bool {
    true
}
fn timeformat() -> String {
    "%Y-%m-%d %H:%M".into()
}
fn tagsymbols() -> String {
    "#@".into()
}
fn wrap() -> Linewrap {
    Linewrap::Num(79)
}
fn indent() -> String {
    "|".into()
}

pub fn default_config() -> Value {
    let mut journals = Mapping::new();
    let mut def = Mapping::new();
    def.insert(
        "journal".into(),
        path::default_journal_path()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    journals.insert("default".into(), Value::Mapping(def));
    let mut m = Mapping::new();
    m.insert("version".into(), VERSION.into());
    m.insert("journals".into(), journals.into());
    m.insert(
        "editor".into(),
        std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_default()
            .into(),
    );
    m.insert("encrypt".into(), false.into());
    m.insert("template".into(), false.into());
    m.insert("default_hour".into(), 9.into());
    m.insert("default_minute".into(), 0.into());
    m.insert("timeformat".into(), timeformat().into());
    m.insert("tagsymbols".into(), tagsymbols().into());
    m.insert("highlight".into(), true.into());
    m.insert("linewrap".into(), 79.into());
    m.insert("indent_character".into(), indent().into());
    m.insert(
        "colors".into(),
        serde_yaml::to_value(Colors::default()).unwrap(),
    );
    Value::Mapping(m)
}

pub fn save(config: &Value, alt: Option<&str>) -> Result<()> {
    let mut c = config.clone();
    c.as_mapping_mut()
        .unwrap()
        .insert("version".into(), VERSION.into());
    let p = alt.map(path::expand).unwrap_or_else(path::config_path);
    path::atomic_write(&p, serde_yaml::to_string(&c)?.as_bytes())
}

pub fn load_or_install(alt: Option<&str>) -> Result<Value> {
    let p = match alt {
        Some(a) => {
            let p = path::expand(a);
            if !p.exists() {
                bail!(
                    "Alternate configuration file not found at the given path: {}",
                    p.display()
                );
            }
            p
        }
        None => path::config_path(),
    };
    if !p.exists() {
        return install();
    }
    let text = std::fs::read_to_string(&p)?;
    let mut config: Value = serde_yaml::from_str(&text)
        .map_err(|_| anyhow!("Unable to parse config file at: {}", p.display()))?;
    if config.is_null() {
        bail!("Unable to parse config file at: {}", p.display());
    }

    // Upgrade: fill missing keys, bump version.
    let defaults = default_config();
    let mut changed = false;
    let map = config.as_mapping_mut().unwrap();
    for (k, v) in defaults.as_mapping().unwrap() {
        if !map.contains_key(k) {
            map.insert(k.clone(), v.clone());
            changed = true;
        }
    }
    if map.get("version").and_then(Value::as_str) != Some(VERSION) {
        changed = true;
    }
    if changed {
        save(&config, alt)?;
        ui::normal(&format!(
            "Configuration updated to newest version at {}",
            p.display()
        ));
    }
    Ok(config)
}

fn install() -> Result<Value> {
    let mut config = default_config();
    let default_path = path::default_journal_path();
    let answer = ui::prompt(&format!(
        "Path to your journal file (leave blank for {}): ",
        default_path.display()
    ));
    let journal_path = if answer.trim().is_empty() {
        default_path
    } else {
        path::expand(answer.trim())
    };
    if let Some(parent) = journal_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    config["journals"]["default"]["journal"] = journal_path.to_string_lossy().into_owned().into();

    if ui::yes_no(
        "Do you want to encrypt your journal? (You can always change this later)",
        false,
    ) {
        config["encrypt"] = true.into();
        ui::normal("Journal will be encrypted");
    }
    if ui::yes_no("Do you want jrnl to use colors to display entries?", true) {
        let colors = Colors {
            body: "none".into(),
            date: "black".into(),
            tags: "yellow".into(),
            title: "cyan".into(),
        };
        config["colors"] = serde_yaml::to_value(colors)?;
    }
    let _ = path::templates_dir();
    save(&config, None)?;
    ui::normal(&format!(
        "jrnl configuration created at {}\nFor advanced features, read the docs at https://jrnl.sh",
        path::config_path().display()
    ));
    Ok(config)
}

pub fn apply_overrides(config: &mut Value, pairs: &[(String, String)]) -> Result<()> {
    for (key, val) in pairs {
        let parsed: Value = serde_yaml::from_str(val).unwrap_or(Value::String(val.clone()));
        let keys: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
        if keys.is_empty() {
            continue;
        }
        let mut node: &mut Value = &mut *config; // re-borrow each iteration
        for (i, k) in keys.iter().enumerate() {
            if i == keys.len() - 1 {
                node.as_mapping_mut()
                    .ok_or_else(|| anyhow!("override target is not a mapping"))?
                    .insert(Value::String(k.to_string()), parsed.clone());
            } else {
                let map = node
                    .as_mapping_mut()
                    .ok_or_else(|| anyhow!("override target is not a mapping"))?;
                if !map.get(*k).map(Value::is_mapping).unwrap_or(false) {
                    map.insert(Value::String(k.to_string()), Value::Mapping(Mapping::new()));
                }
                node = map.get_mut(*k).unwrap();
            }
        }
    }
    Ok(())
}

pub fn take_journal_name(text: &mut Vec<String>, config: &Value) -> String {
    let mut name = "default".to_string();
    if let Some(first) = text.first() {
        let candidate = first.trim_end_matches(':');
        if config["journals"].get(candidate).is_some() {
            name = candidate.to_string();
            text.remove(0);
        }
    }
    name
}

pub fn scope(config: &Value, name: &str) -> Result<Settings> {
    let journals = config
        .get("journals")
        .ok_or_else(|| anyhow!("no journals configured"))?;
    if journals.get(name).is_none() {
        bail!("No '{name}' journal configured");
    }
    let mut scoped = config.clone();
    let entry = journals.get(name).unwrap().clone();
    let map = scoped.as_mapping_mut().unwrap();
    match entry {
        Value::String(p) => {
            map.insert("journal".into(), p.into());
        }
        Value::Mapping(overrides) => {
            for (k, v) in overrides {
                map.insert(k, v);
            }
        }
        _ => bail!("invalid journal configuration for '{name}'"),
    }
    Ok(serde_yaml::from_value(scoped)?)
}

/// Persist a key at the level it originally lived (journal override vs top level).
pub fn set_scoped(config: &mut Value, name: &str, key: &str, val: Value, force_local: bool) {
    let journals = config.get_mut("journals").and_then(Value::as_mapping_mut);
    if let Some(journals) = journals {
        if let Some(entry) = journals.get_mut(name) {
            if entry.is_mapping() {
                let m = entry.as_mapping_mut().unwrap();
                if force_local || m.contains_key(key) {
                    m.insert(key.into(), val.clone());
                    return;
                }
            } else if force_local {
                let path = entry.clone();
                let mut m = Mapping::new();
                m.insert("journal".into(), path);
                m.insert(key.into(), val.clone());
                *entry = Value::Mapping(m);
                return;
            }
        }
    }
    config.as_mapping_mut().unwrap().insert(key.into(), val);
}
