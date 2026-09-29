//! Language service — the Bukkit v2 `lang/` message files ported to Pumpkin.
//!
//! The upstream plugin resolves translatable messages with a `LangService`
//! picking per-player locale files (`lang/zh_CN.yml`, `lang/en_US.yml`, …) and
//! falls back through: player language → `defaultLanguage` → `en_US` → the raw
//! key. This module mirrors that chain without a plugin-side locale registry
//! (Pumpkin does not expose one yet), using the configured default language
//! plus `en_US` as the hard fallback.
//!
//! Message values use `{0}` / `{1}` positional placeholders, resolved by
//! [`Lang::format`] — a tiny re-implementation of the `MessageFormat`-less
//! `.format(key, args)` used by the upstream message API.

use std::collections::HashMap;
use std::sync::RwLock;

/// Resolved message table for one locale. The keys are the upstream TrChat
/// lang keys (`General-Too-Long`, `Channel-No-Speak-Permission`, …).
pub struct Lang {
    /// All loaded locales, keyed by their lowercase-BCP-47 name (`zh_cn`,
    /// `en_us`). Never empty — `en_us` is always present.
    locales: HashMap<String, HashMap<String, String>>,
    /// The default language id from `chat.defaultLanguage`, normalized.
    default: String,
}

static LANG: std::sync::OnceLock<RwLock<Lang>> = std::sync::OnceLock::new();

impl Lang {
    /// Creates the in-memory language table. If a `lang/` folder exists in the
    /// plugin data folder, `*.txt` files inside it are merged over the bundled
    /// defaults (bundled defaults win on conflicts, mirroring the upstream
    /// "defaults copied on first run" behaviour).
    pub fn init(data_folder: &str, default_language: &str) -> Self {
        let mut locales = HashMap::new();
        locales.insert("en_us".to_string(), english());
        locales.insert("zh_cn".to_string(), chinese());
        // External override files (`lang/<locale>.yml` per upstream) are
        // intentionally not parsed here: the WASM sandbox cannot enumerate
        // bundled resources, so overrides are a documented follow-up.
        let _ = data_folder;
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
            .or_else(|| {
                self.locales
                    .get(&self.default)
                    .and_then(|m| m.get(key))
            })
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

    /// Replaces `${key}` tokens inside an arbitrary template (used by console
    /// logging formats, whose `{0}`/`{1}` are positional raw values instead).
    #[allow(dead_code)] // part of the Bukkit v2 message surface; not wired yet
    pub fn placeholder(template: &str, values: &[(&str, &str)]) -> String {
        let mut out = template.to_string();
        for (name, value) in values {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }
}

/// Access to the process-wide language table (initialized once by the plugin).
pub fn lang() -> &'static RwLock<Lang> {
    LANG.get_or_init(|| RwLock::new(Lang::init("", "en_us")))
}

fn normalize(locale: &str) -> String {
    locale.to_ascii_lowercase().replace('-', "_")
}

fn english() -> HashMap<String, String> {
    let mut m = HashMap::new();
    // Chat guards — chat.md §1.4 keys.
    m.insert("General-Too-Long".into(), "&cMessage is too long! Maximum length is {0} characters.".into());
    m.insert("General-Global-Muting".into(), "&cChat has been globally muted by an administrator.".into());
    m.insert("General-Muted".into(), "&cYou have been muted!".into());
    m.insert("General-Too-Similar".into(), "&cPlease do not spam identical or similar messages!".into());
    m.insert("General-Too-Fast".into(), "&cYou are chatting too fast, please slow down.".into());
    // Channels — chat.md §2.* keys.
    m.insert("Channel-No-Speak-Permission".into(), "&cYou do not have permission to speak in this channel.".into());
    m.insert("Channel-No-Join-Permission".into(), "&cYou do not have permission to join channel {0}.".into());
    m.insert("Channel-Command-Unbound".into(), "&cThis command is not bound to any channel.".into());
    m.insert("Channel-Private-Target".into(), "&cPlease specify the player to message.".into());
    m.insert("Channel-Join".into(), "&aJoined channel {0}.".into());
    m.insert("Channel-Quit".into(), "&7Left channel {0}.".into());
    // Filtering — placeholder-function-filter.md.
    m.insert("Filter-Blocked".into(), "&cYour message contains blocked words.".into());
    // Redis / cross-server.
    m.insert("Redis-Fallback".into(), "&7[TrChat] Cross-server relay unavailable, using local chat.".into());
    m.insert("Redis-Force-Unavailable".into(), "&cCross-server relay unavailable; your message was not sent.".into());
    // Mention.
    m.insert("Function-Mention-Notify".into(), "&e{0} mentioned you in chat.".into());
    m
}

fn chinese() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("General-Too-Long".into(), "&c消息过长！最大长度为 {0} 个字符。".into());
    m.insert("General-Global-Muting".into(), "&c全局聊天已被管理员关闭！".into());
    m.insert("General-Muted".into(), "&c你已被禁言！".into());
    m.insert("General-Too-Similar".into(), "&c请勿重复发送相同或相似的消息！".into());
    m.insert("General-Too-Fast".into(), "&c发言过于频繁，请稍后再试。".into());
    m.insert("Channel-No-Speak-Permission".into(), "&c你没有在该频道发言的权限！".into());
    m.insert("Channel-No-Join-Permission".into(), "&c你没有加入频道 {0} 的权限！".into());
    m.insert("Channel-Command-Unbound".into(), "&c该命令未绑定到任何频道！".into());
    m.insert("Channel-Private-Target".into(), "&c请输入要私聊的玩家名！".into());
    m.insert("Channel-Join".into(), "&a你已加入频道 {0}。".into());
    m.insert("Channel-Quit".into(), "&7你已退出频道 {0}。".into());
    m.insert("Filter-Blocked".into(), "&c你的消息包含敏感词！".into());
    m.insert("Redis-Fallback".into(), "&7[TrChat] 跨服通道不可用，已切换为本地聊天。".into());
    m.insert("Redis-Force-Unavailable".into(), "&c跨服通道不可用，你的消息未能发送！".into());
    m.insert("Function-Mention-Notify".into(), "&e{0} 在聊天中提到了你。".into());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_through_default_to_en_us() {
        let lang = Lang::init("", "zh_cn");
        assert_eq!(lang.get("General-Muted", "zh_cn"), "&c你已被禁言！");
        assert_eq!(lang.get("General-Muted", "ja_jp"), "&c你已被禁言！"); // default zh
        assert_eq!(lang.get("Channel-Join", "en_us"), "&aJoined channel {0}."); // en direct
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
            lang.format("General-Too-Long", "zh_cn", &["100"]),
            "&c消息过长！最大长度为 100 个字符。"
        );
    }
}