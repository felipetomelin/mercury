//! Opt-in Claude Code statusLine chaining. The chain file retains the exact
//! previous statusLine object so uninstall can restore it without guessing.

use std::io;
use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstRootNode};
use jsonc_parser::ParseOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config_file::{check_config_target, write_config};

const CHAIN_NAME: &str = "mercury-statusline-chain.json";
const BACKUP_NAME: &str = "settings.json.mercury-statusline-backup";

#[derive(Serialize, Deserialize)]
struct Chain {
    previous: Option<Value>,
    bridge_command: String,
}

pub(crate) fn chain_path(settings_path: &Path) -> PathBuf {
    settings_path.with_file_name(CHAIN_NAME)
}

pub(crate) fn previous_command(path: &Path) -> io::Result<Option<String>> {
    let chain: Chain = serde_json::from_slice(&std::fs::read(path)?)?;
    Ok(chain
        .previous
        .as_ref()
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn parse_settings(content: &str, path: &Path) -> io::Result<(CstRootNode, Value)> {
    let root = CstRootNode::parse(content, &ParseOptions::default())
        .map_err(|err| io::Error::other(format!("invalid {}: {err}", path.display())))?;
    let value: Value = serde_json::from_str(content)?;
    if !value.is_object() {
        return Err(io::Error::other("claude settings must be a JSON object"));
    }
    Ok((root, value))
}

fn bridge_command(executable: &Path, chain: &Path) -> String {
    #[cfg(windows)]
    {
        format!(
            "\"{}\" statusline --chain-file \"{}\"",
            executable.display(),
            chain.display()
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "{} statusline --chain-file {}",
            super::command::shell_single_quote(&executable.display().to_string()),
            super::command::shell_single_quote(&chain.display().to_string())
        )
    }
}

fn cst_value(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(values.iter().map(cst_value).collect()),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), cst_value(value)))
                .collect(),
        ),
    }
}

fn edit_statusline(content: &str, path: &Path, replacement: Option<&Value>) -> io::Result<String> {
    let (root, _) = parse_settings(content, path)?;
    let object = root
        .value()
        .and_then(|value| value.as_object())
        .ok_or_else(|| io::Error::other("claude settings must be a JSON object"))?;
    match replacement {
        Some(value) => {
            let input = cst_value(value);
            if let Some(property) = object.get("statusLine") {
                property.set_value(input);
            } else {
                object.append("statusLine", input);
            }
        }
        None => {
            if let Some(property) = object.get("statusLine") {
                property.remove();
            }
        }
    }
    let updated = root.to_string();
    let (_, parsed) = parse_settings(&updated, path)?;
    if parsed.get("statusLine") != replacement {
        return Err(io::Error::other(
            "failed to update claude statusLine safely",
        ));
    }
    Ok(updated)
}

/// Called only by an explicit `mercury integration install claude` command.
pub(crate) fn install(settings_path: &Path, executable: &Path) -> io::Result<bool> {
    let chain = chain_path(settings_path);
    let backup = settings_path.with_file_name(BACKUP_NAME);
    check_config_target(settings_path)?;
    check_config_target(&chain)?;
    check_config_target(&backup)?;
    let content = std::fs::read_to_string(settings_path)?;
    let (_, settings) = parse_settings(&content, settings_path)?;
    let command = bridge_command(executable, &chain);
    if settings
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        == Some(command.as_str())
    {
        if !chain.exists() {
            return Err(io::Error::other(
                "Claude statusLine uses Mercury but its chain file is missing",
            ));
        }
        return Ok(false);
    }
    if chain.exists() {
        return Err(io::Error::other(format!(
            "cannot replace statusLine: existing chain file at {} needs review",
            chain.display()
        )));
    }
    let previous = settings.get("statusLine").cloned();
    let mut replacement = previous
        .clone()
        .unwrap_or_else(|| serde_json::json!({"type": "command"}));
    let Some(statusline) = replacement.as_object_mut() else {
        return Err(io::Error::other("Claude statusLine must be an object"));
    };
    statusline.insert("type".into(), Value::String("command".into()));
    statusline.insert("command".into(), Value::String(command.clone()));
    let updated = edit_statusline(&content, settings_path, Some(&replacement))?;
    if !backup.exists() {
        write_config(&backup, content.as_bytes())?;
    }
    write_config(
        &chain,
        serde_json::to_vec(&Chain {
            previous,
            bridge_command: command,
        })?,
    )?;
    if let Err(err) = write_config(settings_path, updated) {
        let _ = std::fs::remove_file(&chain);
        return Err(err);
    }
    Ok(true)
}

pub(crate) fn uninstall(settings_path: &Path) -> io::Result<bool> {
    let chain = chain_path(settings_path);
    if !chain.exists() || !settings_path.exists() {
        return Ok(false);
    }
    check_config_target(settings_path)?;
    check_config_target(&chain)?;
    let content = std::fs::read_to_string(settings_path)?;
    let (_, settings) = parse_settings(&content, settings_path)?;
    let saved: Chain = serde_json::from_slice(&std::fs::read(&chain)?)?;
    let installed = settings
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        == Some(saved.bridge_command.as_str());
    if !installed {
        return Err(io::Error::other(
            "statusLine changed since Mercury installed it; restore from the backup manually",
        ));
    }
    let updated = edit_statusline(&content, settings_path, saved.previous.as_ref())?;
    write_config(settings_path, updated)?;
    std::fs::remove_file(chain)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_preserves_unrelated_settings_and_restores_previous_statusline() {
        let path = Path::new("settings.json");
        let original = "{\n  \"theme\": \"dark\",\n  \"statusLine\": {\"type\": \"command\", \"command\": \"old\", \"padding\": 2}\n}\n";
        let old = serde_json::json!({"type":"command","command":"old","padding":2});
        let new = serde_json::json!({"type":"command","command":"mercury statusline"});
        let installed = edit_statusline(original, path, Some(&new)).unwrap();
        assert!(installed.contains("\"theme\": \"dark\""));
        let restored = edit_statusline(&installed, path, Some(&old)).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&restored).unwrap(),
            serde_json::from_str::<Value>(original).unwrap()
        );
    }

    #[test]
    fn install_is_idempotent_and_uninstall_restores_previous_command() {
        let dir = std::env::temp_dir().join(format!(
            "mercury-statusline-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        let original = r#"{"theme":"dark","statusLine":{"type":"command","command":"printf old","padding":2}}"#;
        std::fs::write(&settings, original).unwrap();
        let executable = dir.join("mercury executable");

        assert!(install(&settings, &executable).unwrap());
        let installed: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(installed["statusLine"]["padding"], 2);
        assert!(!install(&settings, &executable).unwrap());
        assert_eq!(
            previous_command(&chain_path(&settings)).unwrap().as_deref(),
            Some("printf old")
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(BACKUP_NAME)).unwrap(),
            original
        );
        assert!(uninstall(&settings).unwrap());
        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&settings).unwrap()).unwrap(),
            serde_json::from_str::<Value>(original).unwrap()
        );
        assert!(!chain_path(&settings).exists());
        assert!(!uninstall(&settings).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
