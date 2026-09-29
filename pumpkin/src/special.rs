//! `special-chars.yml` — 资源包特殊字符表与颜色包裹。
//!
//! 移植自 Mod 的 `SpecialChars.java`：当消息包含配置的特殊字符（表情、
//! 图标等资源包字形）时，把它们包上频道的 `special-char-color`，并在每段
//! 结尾恢复消息默认色，避免字形吃掉后续文字的颜色。手动出现的 `&` 色码
//! 会被保留（Mod 语义：若段内出现手动色码则不再插入后缀）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

/// 进程级特殊字符表；`reload` 时整表替换。
static CHARS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

fn table() -> &'static RwLock<HashSet<String>> {
    CHARS.get_or_init(|| RwLock::new(HashSet::new()))
}

/// 从 `<folder>/special-chars.yml` 的 `SpecialChars` 列表重载字符表。
pub fn reload(folder: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(folder.join("special-chars.yml"))
        .map_err(|e| format!("read special-chars.yml: {e}"))?;
    let value: serde_yaml::Value =
        serde_yaml::from_str(&raw).map_err(|e| format!("parse special-chars.yml: {e}"))?;
    let mut set = HashSet::new();
    if let Some(list) = value.get("SpecialChars").and_then(|v| v.as_sequence()) {
        for item in list {
            // §7 — elements go through `String.valueOf`, so a YAML scalar such
            // as `- 123` contributes `"123"` rather than being dropped.
            let s = scalar_to_string(item);
            let s = s.trim();
            if !s.is_empty() {
                set.insert(s.to_string());
            }
        }
    }
    *table().write().unwrap_or_else(|e| e.into_inner()) = set;
    Ok(())
}

/// §7 — `String.valueOf` coercion for a YAML list element. Strings pass
/// through unchanged; other scalars use their literal YAML text; a `null`/`~`
/// entry (or a nested collection) contributes nothing.
fn scalar_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Whether `text` contains any configured special character (code-point scan,
/// mirroring `SpecialChars.hasSpecialChars`).
pub fn has_special_chars(text: &str) -> bool {
    let special = table().read().unwrap_or_else(|e| e.into_inner());
    if special.is_empty() || text.is_empty() {
        return false;
    }
    text.chars().any(|c| special.contains(&c.to_string()))
}

/// Wraps configured special characters with `prefix` and restores `suffix`
/// after each run, keeping manual `&` color codes intact (Mod semantics:
/// a manual color inside a run suppresses the suffix insertion).
pub fn wrap_special_chars(text: &str, prefix: &str, suffix: &str) -> String {
    let special = table().read().unwrap_or_else(|e| e.into_inner());
    if text.is_empty() || prefix.is_empty() || special.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_special = false;
    let mut manual_color = false;
    let mut has_manual_color = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        let cp = c as u32;
        let is_special = special.contains(&c.to_string());
        // ZWJ / skin-tone modifiers / variation selector — part of an emoji run.
        let is_extension = cp == 0x200D || (0x1F3FB..=0x1F3FF).contains(&cp) || cp == 0xFE0F;

        if !is_special && !is_extension && c == '&' {
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            let is_default_prefix = i == 0 && text.starts_with(suffix);
            if !is_default_prefix {
                has_manual_color = !(next == 'r' || next == 'R');
            }
            out.push(c);
            if let Some(&n) = chars.get(i + 1) {
                out.push(n);
            }
            i += if i + 1 < chars.len() { 2 } else { 1 };
            continue;
        }

        if is_special {
            if !in_special {
                manual_color = has_manual_color;
                in_special = true;
                if !manual_color {
                    out.push_str(prefix);
                }
            }
            out.push(c);
        } else if is_extension && in_special {
            out.push(c);
        } else if in_special {
            if !manual_color {
                out.push_str(suffix);
            }
            in_special = false;
            out.push(c);
        } else {
            out.push(c);
        }
        i += 1;
    }
    if in_special && !manual_color {
        out.push_str(suffix);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests share the process-wide table, so serialize them.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Locks the suite and points the table at a temp `special-chars.yml`
    /// with the given entries; the guard lives until the test body ends.
    fn with_table(entries: &[&str]) -> (std::sync::MutexGuard<'static, ()>, tempfile_dir::TempDir) {
        let guard = SERIAL.lock().unwrap();
        let dir = tempfile_dir::TempDir::new();
        let list = entries
            .iter()
            .map(|e| format!("      - '{e}'"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            dir.path().join("special-chars.yml"),
            format!("# test\nSpecialChars:\n{list}\n"),
        )
        .unwrap();
        reload(dir.path()).unwrap();
        (guard, dir)
    }

    #[test]
    fn non_string_scalars_are_coerced_like_string_value_of() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile_dir::TempDir::new();
        // §7 — `String.valueOf` on each element, then drop blanks; `~`/null
        // entries contribute nothing. `hasSpecialChars` scans single code
        // points, so the assertions use one-character entries.
        std::fs::write(
            dir.path().join("special-chars.yml"),
            "SpecialChars:\n  - 7\n  - true\n  - '★'\n  - '   '\n  - ~\n",
        )
        .unwrap();
        reload(dir.path()).unwrap();
        let table_len = table().read().unwrap().len();
        assert_eq!(table_len, 3, "numeric, bool and glyph entries kept, blanks dropped");
        assert!(has_special_chars("7"), "numeric scalar coerced to a string");
        assert!(has_special_chars("★"));
        assert!(!has_special_chars("~"), "null entry contributes nothing");
        assert!(!has_special_chars("   "));
    }

    #[test]
    fn empty_table_never_matches_or_wraps() {
        let (_guard, _dir) = with_table(&[]);
        assert!(!has_special_chars("plain text"));
        assert_eq!(wrap_special_chars("a★b", "&f", "&7"), "a★b");
    }

    #[test]
    fn wraps_configured_chars_with_prefix_suffix() {
        let (_guard, _dir) = with_table(&["★", "☂"]);
        assert!(has_special_chars("你好★世界"));
        assert!(!has_special_chars("hello world"));
        assert_eq!(
            wrap_special_chars("&7你好★世界", "&f", "&7"),
            "&7你好&f★&7世界"
        );
        assert_eq!(wrap_special_chars("a☂b★c", "&f", "&7"), "a&f☂&7b&f★&7c");
    }

    #[test]
    fn manual_color_before_a_run_suppresses_the_prefix() {
        let (_guard, _dir) = with_table(&["★"]);
        // A manual `&e` right before the glyph is the run's own color — the
        // wrap prefix must not be inserted on top of it.
        assert_eq!(wrap_special_chars("&e★", "&f", "&7"), "&e★");
        // A manual color *inside* the run is copied verbatim; the suffix is
        // still restored when the run ends (Mod semantics).
        assert_eq!(wrap_special_chars("★&e字", "&f", "&7"), "&f★&e&7字");
    }

    #[test]
    fn emoji_extension_stays_inside_the_run() {
        let (_guard, _dir) = with_table(&["👦", "⭐"]);
        // ZWJ and skin-tone modifiers join the glyph; the trailing ♂ is not
        // an extension, so it closes the run (restoring the suffix).
        let joined = "👦\u{200D}\u{2642}";
        assert_eq!(wrap_special_chars(joined, "&f", "&7"), "&f👦\u{200D}&7♂");
        // Skin-tone modifier after the base stays in the run.
        assert_eq!(
            wrap_special_chars("👦\u{1F3FB}", "&f", "&7"),
            "&f👦\u{1F3FB}&7"
        );
        // Unrelated text after the run restores the suffix first.
        assert_eq!(wrap_special_chars("⭐ok", "&f", "&7"), "&f⭐&7ok");
    }

    #[test]
    fn resets_state_across_runs() {
        let (_guard, _dir) = with_table(&["★"]);
        assert_eq!(wrap_special_chars("★a★", "&f", "&7"), "&f★&7a&f★&7");
    }
}

/// Minimal temp-directory helper so the crate tests need no extra dependency.
#[cfg(test)]
mod tempfile_dir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("trchat-special-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
