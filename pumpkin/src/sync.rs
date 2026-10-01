//! YAML reconciliation against the bundled defaults — the NeoForge/Fabric
//! port's `YamlConfigSynchronizer` (`config/YamlConfigSynchronizer.java`).
//!
//! `serde(default)` can only ever fill a missing key with the *type* default.
//! The Mod's config layer instead reconciles the file against the bundled
//! resource, so a key introduced by an update appears with the value it ships
//! with, and the user's own values survive.
//!
//! Rules, taken from the Java implementation:
//!
//! * a file that does not exist yet is a byte-for-byte copy of the bundled
//!   resource (comments and formatting included);
//! * the reconciled mapping follows the bundled key order, then any
//!   schema-only key in file order;
//! * a missing key takes the bundled value, a list keeps the user's list, and
//!   any other scalar keeps the user's value (even when the types differ);
//! * a nested mapping is reconciled recursively, with the schema deciding which
//!   keys are allowed;
//! * a key that neither the bundled default nor the schema declares is dropped.
//!   The Java's `openMapPaths` escape hatch is unused by every call site, so
//!   this port has no such parameter.
//!
//! The file is rewritten only when reconciliation actually changes something, so
//! a freshly seeded file (already identical to the bundled resource) is never
//! reformatted.

use std::path::Path;

use serde_yaml::{Mapping, Value};

/// Reconciles `file` against `default_yaml` (repeating it as the schema, which
/// is what every settings-style file does) and returns the merged mapping.
pub fn synchronize(file: &Path, default_yaml: &str, schema_yaml: &str) -> Result<Value, String> {
    let defaults = load(default_yaml, "the bundled default")?;
    let schema = if default_yaml == schema_yaml {
        defaults.clone()
    } else {
        load(schema_yaml, "the bundled schema")?
    };
    let defaults = as_mapping(&defaults, "the bundled default")?;
    let schema = as_mapping(&schema, "the bundled schema")?;

    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    if !file.exists() {
        std::fs::write(file, default_yaml)
            .map_err(|error| format!("write {}: {error}", file.display()))?;
    }

    let current = read_mapping(file)?;
    let reconciled = Value::Mapping(reconcile_map(defaults, schema, &current));
    if reconciled != Value::Mapping(current) {
        write(file, &reconciled)?;
        crate::diag::info(format!("Repaired YAML configuration {}", file.display()));
    }
    Ok(reconciled)
}

/// `YamlConfigSynchronizer.loadFile`.
fn read_mapping(file: &Path) -> Result<Mapping, String> {
    let raw = std::fs::read_to_string(file)
        .map_err(|error| format!("read {}: {error}", file.display()))?;
    let label = file.display().to_string();
    let value = load(&raw, &label)?;
    as_mapping(&value, &label).cloned()
}

/// `YamlConfigSynchronizer.load` — the root must be a mapping.
fn load(source: &str, label: &str) -> Result<Value, String> {
    let value: Value =
        serde_yaml::from_str(source).map_err(|error| format!("parse {label}: {error}"))?;
    if !value.is_mapping() {
        return Err(format!("{label}: the YAML root must be a mapping"));
    }
    Ok(value)
}

fn as_mapping<'a>(value: &'a Value, label: &str) -> Result<&'a Mapping, String> {
    value
        .as_mapping()
        .ok_or_else(|| format!("{label}: the YAML root must be a mapping"))
}

/// `YamlConfigSynchronizer.reconcileMap`.
fn reconcile_map(defaults: &Mapping, schema: &Mapping, current: &Mapping) -> Mapping {
    let mut output = Mapping::new();
    for (key, default) in defaults {
        let schema_value = schema.get(key).unwrap_or(default);
        let value = match current.get(key) {
            Some(user) => reconcile(default, schema_value, user),
            None => default.clone(),
        };
        output.insert(key.clone(), value);
    }
    for (key, user) in current {
        if output.contains_key(key) {
            continue;
        }
        // A schema-only key is kept when the user configured it; anything the
        // schema does not declare is dropped.
        if let Some(schema_value) = schema.get(key) {
            output.insert(key.clone(), reconcile(schema_value, schema_value, user));
        }
    }
    output
}

/// `YamlConfigSynchronizer.reconcile`.
fn reconcile(default: &Value, schema: &Value, current: &Value) -> Value {
    match default {
        Value::Mapping(default_map) => match current {
            Value::Mapping(current_map) => {
                let allowed = schema.as_mapping().unwrap_or(default_map);
                Value::Mapping(reconcile_map(default_map, allowed, current_map))
            }
            _ => Value::Mapping(default_map.clone()),
        },
        Value::Sequence(_) => match current {
            Value::Sequence(_) => current.clone(),
            _ => default.clone(),
        },
        // Scalars keep the user's value, including a language entry that
        // replaces a string with a rich map or list.
        _ => current.clone(),
    }
}

/// Writes the repaired mapping: a temporary file next to `file` first, then a
/// rename, so a crash cannot leave a half-written configuration behind
/// (`YamlConfigSynchronizer.write`).
fn write(file: &Path, value: &Value) -> Result<(), String> {
    let yaml = serde_yaml::to_string(value)
        .map_err(|error| format!("serialize {}: {error}", file.display()))?;
    let temporary = file.with_extension("yml.tmp");
    std::fs::write(&temporary, &yaml)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    if let Err(error) = std::fs::rename(&temporary, file) {
        // An unsupported atomic move falls back to an in-place write.
        std::fs::write(file, &yaml).map_err(|error| format!("write {}: {error}", file.display()))?;
        let _ = std::fs::remove_file(&temporary);
        crate::diag::warn(&format!(
            "config: could not move the temporary file over {}: {error}",
            file.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("trchat-sync-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    const BUNDLED: &str = "Options:\n  Auto-Join: false\n  Ports: []\nName: ''\n";

    #[test]
    fn a_missing_file_is_a_verbatim_copy() {
        let dir = temp_dir("missing");
        let file = dir.join("Schema.yml");
        let value = synchronize(&file, BUNDLED, BUNDLED).expect("sync");
        assert!(value.is_mapping());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), BUNDLED);
    }

    #[test]
    fn missing_keys_are_filled_from_the_bundled_default() {
        let dir = temp_dir("fill");
        let file = dir.join("Schema.yml");
        std::fs::write(&file, "Options:\n  Auto-Join: true\nName: 'Survival'\n").unwrap();
        let value = synchronize(&file, BUNDLED, BUNDLED).expect("sync");
        // The user's values survive …
        assert_eq!(value["Options"]["Auto-Join"].as_bool(), Some(true));
        assert_eq!(value["Name"].as_str(), Some("Survival"));
        // … and the missing list comes back from the bundled default.
        assert_eq!(value["Options"]["Ports"].as_sequence().map(Vec::len), Some(0));
        // The repaired file is written back.
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("Ports"), "repaired file: {written}");
    }

    #[test]
    fn unknown_keys_are_dropped_and_schema_only_keys_survive() {
        let dir = temp_dir("schema");
        let file = dir.join("Schema.yml");
        let schema = "Options:\n  Auto-Join: false\nName: ''\nExtra: ''\n";
        std::fs::write(&file, "Options:\n  Auto-Join: false\nName: 'x'\nExtra: 'kept'\nGone: 1\n")
            .unwrap();
        let value = synchronize(&file, BUNDLED, schema).expect("sync");
        assert_eq!(value["Extra"].as_str(), Some("kept"));
        assert!(value.get("Gone").is_none(), "unknown key dropped: {value:?}");
    }

    #[test]
    fn lists_are_values_and_scalars_win_over_the_default() {
        let dir = temp_dir("lists");
        let file = dir.join("Schema.yml");
        let defaults = "Options:\n  Ports: []\nName: 'bundled'\n";
        std::fs::write(&file, "Options:\n  Ports: ['a', 'b']\nName: 'mine'\n").unwrap();
        let value = synchronize(&file, defaults, defaults).expect("sync");
        assert_eq!(
            value["Options"]["Ports"]
                .as_sequence()
                .map(|ports| ports.len()),
            Some(2)
        );
        assert_eq!(value["Name"].as_str(), Some("mine"));
    }

    #[test]
    fn a_broken_file_is_reported_instead_of_rewritten() {
        let dir = temp_dir("broken");
        let file = dir.join("Schema.yml");
        std::fs::write(&file, "Options: [unclosed\n").unwrap();
        assert!(synchronize(&file, BUNDLED, BUNDLED).is_err());
        // The broken content is left alone for the operator to inspect.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Options: [unclosed\n");
    }

    /// The files this plugin ships must round-trip byte-for-byte: a fresh copy is
    /// already reconciled, so a server start must never reformat or prune it.
    #[test]
    fn the_bundled_files_are_already_reconciled() {
        let dir = temp_dir("roundtrip");
        for (name, content) in [
            ("settings.yml", crate::config::defaults::SETTINGS),
            ("datasource.yml", crate::config::defaults::DATASOURCE),
            ("filter.yml", crate::config::defaults::FILTER),
        ] {
            let file = dir.join(name);
            synchronize(&file, content, content).expect("first sync");
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                content,
                "{name} was rewritten on the first start"
            );
            synchronize(&file, content, content).expect("second sync");
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                content,
                "{name} changed on a second start"
            );
        }
        for (name, content) in crate::config::defaults::CHANNELS {
            let file = dir.join(format!("{name}.yml"));
            synchronize(&file, content, crate::config::defaults::SCHEMA).expect("channel sync");
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                *content,
                "channels/{name}.yml was rewritten on the first start"
            );
        }
    }
}
