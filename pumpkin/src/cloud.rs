//! Cloud thesaurus — `filter.yml`'s `Cloud-Thesaurus` block.
//!
//! The Mod merges a remote dictionary into the local word list
//! (`DefaultFilterManager.loadFilter` / `loadCloudThesaurus` /
//! `catchCloudThesaurus`, `api/impl/DefaultFilterManager.kt:48-142`) and the
//! local port does the same over `HttpClient` with a one-hour timer and a
//! `filters/<hex>.json` cache (`FilterService.refreshCloudAsync` / `fetch`).
//!
//! Ported behaviour:
//!
//! * `load` reports the local word count (`Plugin-Loaded-Filter-Local`) and
//!   `refresh` fetches every `Cloud-Thesaurus.Urls` entry with a 30 s timeout.
//! * A successful fetch whose `lastUpdateDate` differs from the last one applied
//!   adds its `words` (minus `Cloud-Thesaurus.Ignored`, case-insensitive) to the
//!   accumulated cloud set and the raw body is cached under
//!   `{data_folder}/filters/<hex>.json`. A failed fetch falls back to that cache
//!   — the "local fallback" of the sandbox — and reports
//!   `Plugin-Failed-Load-Filter-Cloud` only when the accumulated set is empty.
//! * The refresh runs on the host scheduler every 72 000 ticks (one hour) —
//!   the Mod's `submitAsync(period = 60 * 60 * 20)` counts ticks, the same
//!   cadence as the local port's `ticks >= 72_000` — plus once on the tick after
//!   `/trchat reload` (`DefaultFilterManager.loadFilter(updateCloud = true)`).
//!
//! Deviations, both recorded in `docs/spec/data-redis-update.md`: the Mod
//! accumulates the cloud set across refreshes (so a word removed upstream stays
//! blocked), and its notification goes to the console through the plugin log
//! instead of `ProxyCommandSender.sendLang`.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::config::{data_folder, FilterConfig};
use crate::diag;
use crate::http;
use crate::lang;

/// One hour in ticks — the Mod's `submitAsync(period = 60 * 60 * 20)` and the
/// local port's `ticks >= 72_000`.
pub const REFRESH_PERIOD_TICKS: u64 = 60 * 60 * 20;
/// The first refresh runs on the next tick, like the Mod's async task.
pub const REFRESH_DELAY_TICKS: u64 = 1;
/// Connect/read timeout (`DefaultFilterManager.kt:105`, `FilterService.fetch`).
const TIMEOUT_SECONDS: u64 = 30;

/// Accumulated cloud words (`DefaultFilterManager.cloud_words`), longest first.
static CLOUD_WORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// `lastUpdateDate` per URL (`cloud_last_update`).
static LAST_UPDATED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// The cloud words currently in effect; merged into the profile by
/// [`crate::filter::text_filter`].
pub fn cloud_words() -> Vec<String> {
    lock(&CLOUD_WORDS).clone()
}

/// `DefaultFilterManager.loadFilter` (`:48-76`): reports the local profile.
///
/// The cloud refresh itself is scheduled (`ChatManager::init`) or triggered by a
/// reload, so a slow thesaurus never delays the plugin load.
pub fn load(config: &FilterConfig) {
    inform(
        "Plugin-Loaded-Filter-Local",
        &[&config.local_words.len().to_string()],
    );
}

/// `DefaultFilterManager.loadCloudThesaurus` (`:78-102`): fetches every URL and
/// adds what it returned to the accumulated word set.
pub fn refresh(config: &FilterConfig) {
    if !config.cloud_enabled || config.cloud_urls.is_empty() {
        return;
    }
    let mut collected = Vec::new();
    for url in &config.cloud_urls {
        collected.extend(fetch(url, &config.cloud_ignored));
    }
    {
        let mut words = lock(&CLOUD_WORDS);
        words.extend(collected);
        words.sort_by(|a, b| b.len().cmp(&a.len()));
        words.dedup();
    }
    if lock(&CLOUD_WORDS).is_empty() {
        inform("Plugin-Failed-Load-Filter-Cloud", &[]);
    }
}

/// [`refresh`] against the live configuration (the scheduler and `/trchat reload`).
pub fn refresh_current() {
    let config = crate::config::global_config();
    let config = config.read();
    refresh(config.filter_config());
}

/// `catchCloudThesaurus` (`:104-133`): the database over the network, else the
/// cached copy of an earlier successful fetch.
fn fetch(url: &str, ignored: &[String]) -> Vec<String> {
    match http::get(url, &[], TIMEOUT_SECONDS) {
        Ok(body) => {
            let words = read_database(url, &body, ignored);
            if !words.is_empty() {
                write_cache(url, &body);
                let updated = last_update(url).unwrap_or_default();
                inform(
                    "Plugin-Loaded-Filter-Cloud",
                    &[&words.len().to_string(), url, &updated],
                );
            }
            words
        }
        Err(error) => match std::fs::read_to_string(cache_path(url)) {
            Ok(body) => {
                diag::warn(&format!(
                    "cloud thesaurus {url}: {error}; using the cache instead"
                ));
                read_database(url, &body, ignored)
            }
            Err(_) => {
                diag::warn(&format!("cloud thesaurus {url}: {error}"));
                Vec::new()
            }
        },
    }
}

/// `readDatabase` (`:135-155`): requires `lastUpdateDate` and `words`, drops a
/// database whose date was already applied and filters `Ignored` entries
/// (case-insensitive, like `FilterService.fetch`).
fn read_database(url: &str, body: &str, ignored: &[String]) -> Vec<String> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(body) else {
        diag::warn(&format!("cloud thesaurus {url}: the payload is not a JSON object"));
        return Vec::new();
    };
    let Some(updated) = root.get("lastUpdateDate").and_then(|value| value.as_str()) else {
        diag::warn(&format!("cloud thesaurus {url}: no lastUpdateDate"));
        return Vec::new();
    };
    let Some(words) = root.get("words").and_then(|value| value.as_array()) else {
        diag::warn(&format!("cloud thesaurus {url}: no words"));
        return Vec::new();
    };
    {
        let mut last = lock(&LAST_UPDATED);
        match last.iter_mut().find(|(key, _)| key == url) {
            // An unchanged database contributes nothing (`:137-141`).
            Some(entry) if entry.1 == updated => return Vec::new(),
            Some(entry) => entry.1 = updated.to_string(),
            None => last.push((url.to_string(), updated.to_string())),
        }
    }
    words
        .iter()
        .filter_map(|word| word.as_str())
        .filter(|word| !ignored.iter().any(|skip| skip.eq_ignore_ascii_case(word)))
        .map(str::to_string)
        .collect()
}

/// The `lastUpdateDate` recorded for `url`.
fn last_update(url: &str) -> Option<String> {
    lock(&LAST_UPDATED)
        .iter()
        .find(|(key, _)| key == url)
        .map(|(_, updated)| updated.clone())
}

/// Writes the raw database so a later failed fetch can fall back to it
/// (`DefaultFilterManager.kt:116-118`).
fn write_cache(url: &str, body: &str) {
    let path = cache_path(url);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, body) {
        diag::warn(&format!("cloud thesaurus cache {}: {error}", path.display()));
    }
}

/// `filters/<Integer.toHexString(url.hashCode())>.json` — the local port's cache
/// naming (`FilterService.fetch`; the Mod uses `url.digest("md5")` for the same
/// purpose).
fn cache_path(url: &str) -> PathBuf {
    PathBuf::from(data_folder())
        .join("filters")
        .join(format!("{:x}.json", java_hash_code(url) as u32))
}

/// Java's `String.hashCode` (over UTF-16 code units, like the JVM), used for the
/// cache file name.
pub fn java_hash_code(value: &str) -> i32 {
    value
        .encode_utf16()
        .fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)))
}

/// Renders a `lang` key in the default language and writes it to the plugin log,
/// the closest this port gets to the Mod's `console().sendLang(...)`.
fn inform(key: &str, args: &[&str]) {
    let text = lang::lang()
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .format(key, "", args);
    diag::info(&text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_hash_code_matches_the_jvm() {
        assert_eq!(java_hash_code(""), 0);
        assert_eq!(java_hash_code("abc"), 96354);
        assert_eq!(java_hash_code("hello"), 99162322);
        // The cache name is `Integer.toHexString`, i.e. unsigned.
        assert_eq!(format!("{:x}", java_hash_code("abc") as u32), "17862");
        let path = cache_path("https://example.invalid/db.json");
        assert_eq!(path.parent().and_then(|dir| dir.file_name()).and_then(|n| n.to_str()), Some("filters"));
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(
                format!(
                    "{:x}.json",
                    java_hash_code("https://example.invalid/db.json") as u32
                )
                .as_str()
            )
        );
    }

    #[test]
    fn database_parsing_follows_the_mod() {
        let ignored = vec!["nt".to_string()];
        let url = "https://cloud-a.invalid/db.json";
        let body = r#"{"lastUpdateDate":"2024-01-01","words":["bad","NT","worse"]}"#;
        assert_eq!(
            read_database(url, body, &ignored),
            vec!["bad".to_string(), "worse".to_string()]
        );
        // The same date is applied once only.
        assert!(read_database(url, body, &ignored).is_empty());

        let url = "https://cloud-b.invalid/db.json";
        assert!(read_database(url, "not json", &ignored).is_empty());
        assert!(read_database(url, r#"{"words":["bad"]}"#, &ignored).is_empty());
        assert!(read_database(url, r#"{"lastUpdateDate":"x"}"#, &ignored).is_empty());
    }
}
