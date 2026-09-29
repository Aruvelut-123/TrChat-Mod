//! Language service — the Bukkit v2 `lang/` message files ported to Pumpkin.
//!
//! The upstream plugin resolves translatable messages with a `LangService`
//! picking per-player locale files (`lang/zh_CN.yml`, `lang/en_US.yml`, …) and
//! falls back through: player language → `defaultLanguage` → `en_US` → the raw
//! key. This module mirrors that chain without a plugin-side locale registry
//! (Pumpkin does not expose one yet), using the configured default language
//! plus `en_US` as the hard fallback.
//!
//! The message tables come from the bundled Mod defaults
//! ([`crate::config::defaults::LANGS`], byte-identical to
//! `src/main/resources/defaults/lang/`) and any `lang/*.yml` files the server
//! operator edits in the data folder — user files win over the bundled copy,
//! mirroring how the Mod seeds its `config/trchat/lang/` on first run.
//!
//! Message values use `{0}` / `{1}` positional placeholders, resolved by
//! [`Lang::format`] — a tiny re-implementation of the `MessageFormat`-less
//! `.format(key, args)` used by the upstream message API.

use std::collections::HashMap;
use std::sync::RwLock;

use serde_yaml::Value;

use crate::config::defaults;

/// Resolved message table for one locale. The keys are the upstream TrChat
/// lang keys (`General-Too-Long`, `Channel-No-Speak-Permission`, …).
pub struct Lang {
    /// All loaded locales, keyed by their lowercase-BCP-47 name (`zh_cn`,
    /// `en_us`, `es_es`). Never empty — `en_us` is always present.
    locales: HashMap<String, HashMap<String, String>>,
    /// The default language id from `chat.defaultLanguage`, normalized.
    default: String,
}

static LANG: std::sync::OnceLock<RwLock<Lang>> = std::sync::OnceLock::new();

impl Lang {
    /// Creates the in-memory language table from the bundled YAML defaults,
    /// then merges every `lang/*.yml` file found in the data folder over them
    /// (user files win, matching the Mod's "defaults copied on first run").
    pub fn init(data_folder: &str, default_language: &str) -> Self {
        let mut locales = HashMap::new();
        for (name, raw) in defaults::LANGS {
            let table = parse_table(raw);
            if !table.is_empty() {
                locales.insert(normalize(name), table);
            }
        }
        if !data_folder.is_empty() {
            merge_folder_overrides(&mut locales, data_folder);
        }
        let default = normalize(default_language);
        if !locales.contains_key(&default) {
            // Unknown configured language: silently fall back to en_US, same
            // as the upstream `LangService` which also tolerates missing files.
            eprintln!("[trchat] lang: unknown default language '{default}', using en_us");
        }
        Self { locales, default }
    }

    /// Resolves `key` in `locale`, falling back to the default language and
    /// then to `en_us`; finally the raw key itself is returned, matching the
    /// upstream chain's last resort.
    pub fn get<'a>(&'a self, key: &'a str, locale: &str) -> &'a str {
        let locale = normalize(locale);
        self.locales
            .get(&locale)
            .and_then(|m| m.get(key))
            .or_else(|| self.locales.get(&self.default).and_then(|m| m.get(key)))
            .or_else(|| self.locales.get("en_us").and_then(|m| m.get(key)))
            .map(String::as_str)
            .unwrap_or(key)
    }

    /// Resolves `key` and substitutes `{0}`, `{1}`, … with the given args.
    pub fn format(&self, key: &str, locale: &str, args: &[&str]) -> String {
        let template = self.get(key, locale);
        let mut out = template.to_string();
        for (i, arg) in args.iter().enumerate() {
            out = out.replace(&format!("{{{i}}}"), arg);
        }
        out
    }
}

/// Access to the process-wide language table (seeded by
/// [`SharedConfig::load`]; falls back to bundled `en_US` before that).
pub fn lang() -> &'static RwLock<Lang> {
    LANG.get_or_init(|| RwLock::new(Lang::init("", "en_us")))
}

/// (Re)seeds the process-wide language table from the data folder. Called on
/// plugin load and on every `/trchat reload` so language edits apply live.
pub fn lang_init(data_folder: &str, default_language: &str) {
    let next = Lang::init(data_folder, default_language);
    let lock = LANG.get_or_init(|| RwLock::new(Lang::init("", "en_us")));
    *lock.write().unwrap_or_else(|e| e.into_inner()) = next;
}

/// Parses one `lang/<locale>.yml` document into a flat key → string table.
/// Nested sections (`Placeholder-Translations:`) and non-string values are
/// skipped, mirroring the Mod loader which drops unknown keys on load.
fn parse_table(raw: &str) -> HashMap<String, String> {
    let Ok(Value::Mapping(map)) = serde_yaml::from_str::<Value>(raw) else {
        eprintln!("[trchat] lang: bundled file is not a mapping, ignored");
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (k, v) in map {
        let Some(key) = k.as_str() else { continue };
        match v {
            Value::String(s) => {
                out.insert(key.to_string(), s);
            }
            Value::Number(n) => {
                out.insert(key.to_string(), n.to_string());
            }
            _ => {}
        }
    }
    out
}

/// Merges `data_folder/lang/*.yml` over the bundled tables. The data-folder
/// copy is the one operators edit after the first-run seed, so it wins.
fn merge_folder_overrides(
    locales: &mut HashMap<String, HashMap<String, String>>,
    data_folder: &str,
) {
    let dir = std::path::Path::new(data_folder).join("lang");
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return; // no override folder yet
    };
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }
        let locale = name
            .rsplit_once('.')
            .map(|(stem, _)| normalize(stem))
            .unwrap_or_else(|| normalize(&name));
        let Ok(raw) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let table = parse_table(&raw);
        if table.is_empty() {
            continue;
        }
        locales
            .entry(locale)
            .and_modify(|existing| existing.extend(table.clone()))
            .or_insert(table);
    }
}

fn normalize(locale: &str) -> String {
    locale.to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trchat-pumpkin-lang-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn bundled_tables_load() {
        let lang = Lang::init("", "zh_cn");
        assert_eq!(
            lang.get("General-Muted", "zh_cn"),
            "&8[&3Tr&bChat&8] &c你已被禁言，解除时间：{0}，原因：{1}"
        );
        assert_eq!(
            lang.get("Channel-Join", "en_us"),
            "&8[&3Tr&bChat&8] &aJoined channel {0}."
        );
        assert!(lang.get("General-Too-Long", "es_es").contains("largo"));
        // nested sections are skipped, not a parse error
        assert_eq!(
            lang.get("Placeholder-Translations", "en_us"),
            "Placeholder-Translations"
        );
    }

    #[test]
    fn falls_back_through_default_to_en_us() {
        let lang = Lang::init("", "zh_cn");
        assert_eq!(
            lang.get("General-Muted", "zh_cn"),
            "&8[&3Tr&bChat&8] &c你已被禁言，解除时间：{0}，原因：{1}"
        );
        // unknown locale falls back to the default (zh_cn)
        assert_eq!(
            lang.get("General-Muted", "ja_jp"),
            "&8[&3Tr&bChat&8] &c你已被禁言，解除时间：{0}，原因：{1}"
        );
        // en_US resolves directly
        assert_eq!(
            lang.get("Channel-Join", "en_us"),
            "&8[&3Tr&bChat&8] &aJoined channel {0}."
        );
    }

    #[test]
    fn unknown_key_returns_the_key_itself() {
        let lang = Lang::init("", "en_us");
        assert_eq!(lang.get("No-Such-Key", "en_us"), "No-Such-Key");
    }

    #[test]
    fn positional_formatting() {
        let lang = Lang::init("", "zh_cn");
        assert_eq!(
            lang.format("General-Too-Long", "zh_cn", &["100", "256"]),
            "&8[&3Tr&bChat&8] &7你的聊天内容过长。&8[&6100&8/&2256&8]"
        );
    }

    #[test]
    fn data_folder_overrides_win() {
        let dir = temp_dir("override");
        let lang_dir = dir.join("lang");
        std::fs::create_dir_all(&lang_dir).unwrap();
        std::fs::write(
            lang_dir.join("zh_CN.yml"),
            "General-Muted: '&c自定义禁言文案'\nChannel-Join: '&a自定义加入文案'\n",
        )
        .unwrap();
        let lang = Lang::init(&dir.to_string_lossy(), "zh_cn");
        assert_eq!(lang.get("General-Muted", "zh_cn"), "&c自定义禁言文案");
        // bundled keys untouched by the override still resolve
        assert!(lang.get("General-Too-Long", "zh_cn").contains("过长"));
    }
}
