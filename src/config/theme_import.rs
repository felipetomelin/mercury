//! Strict, read-only import of flat Base16 YAML and Ghostty color files.
//! Files are loaded with configuration, never from the render path.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;

use serde::Deserialize;

use super::{CustomThemeColors, THEME_NAMES};

const MAX_THEME_FILE_BYTES: u64 = 128 * 1024;
const MAX_THEME_IMPORTS: usize = 32;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeImportFormat {
    Base16,
    Ghostty,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeImportConfig {
    pub name: String,
    pub path: PathBuf,
    pub format: ThemeImportFormat,
}

fn hex_color(value: &str) -> Result<String, String> {
    let value = value.trim().trim_matches(['"', '\'']);
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("expected six-digit RGB color, got {value:?}"));
    }
    Ok(format!("#{}", hex.to_ascii_lowercase()))
}

fn base16(content: &str) -> Result<CustomThemeColors, String> {
    let mut colors = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("unsupported Base16 line: {line:?}"))?;
        let key = key.trim().to_ascii_lowercase();
        if key == "scheme" || key == "author" {
            continue;
        }
        if !matches!(
            key.as_str(),
            "base00"
                | "base01"
                | "base02"
                | "base03"
                | "base04"
                | "base05"
                | "base06"
                | "base07"
                | "base08"
                | "base09"
                | "base0a"
                | "base0b"
                | "base0c"
                | "base0d"
                | "base0e"
                | "base0f"
        ) {
            return Err(format!("unsupported Base16 key {key:?}"));
        }
        if colors.insert(key.clone(), hex_color(value)?).is_some() {
            return Err(format!("duplicate Base16 key {key:?}"));
        }
    }
    let get = |key: &str| {
        colors
            .get(key)
            .cloned()
            .ok_or_else(|| format!("missing Base16 key {key}"))
    };
    for index in 0..16 {
        get(&format!("base{index:02x}"))?;
    }
    Ok(CustomThemeColors {
        panel_bg: Some(get("base00")?),
        active_row_bg: Some(get("base01")?),
        selection_bg: Some(get("base02")?),
        surface0: Some(get("base02")?),
        surface1: Some(get("base03")?),
        surface_dim: Some(get("base01")?),
        overlay0: Some(get("base03")?),
        overlay1: Some(get("base04")?),
        text: Some(get("base07")?),
        subtext0: Some(get("base05")?),
        accent: Some(get("base0d")?),
        red: Some(get("base08")?),
        peach: Some(get("base09")?),
        yellow: Some(get("base0a")?),
        green: Some(get("base0b")?),
        teal: Some(get("base0c")?),
        blue: Some(get("base0d")?),
        mauve: Some(get("base0e")?),
        gauge_ok: Some(get("base0b")?),
        gauge_warn: Some(get("base0a")?),
        gauge_crit: Some(get("base08")?),
        ..Default::default()
    })
}

fn ghostty(content: &str) -> Result<CustomThemeColors, String> {
    let mut values = HashMap::new();
    let mut palette = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("unsupported Ghostty line: {line:?}"))?;
        let key = key.trim().to_ascii_lowercase();
        if key == "palette" {
            let (index, color) = value
                .trim()
                .split_once('=')
                .ok_or_else(|| format!("invalid Ghostty palette entry {value:?}"))?;
            let index: u8 = index
                .trim()
                .parse()
                .map_err(|_| format!("invalid Ghostty palette index {index:?}"))?;
            if index > 15 || palette.insert(index, hex_color(color)?).is_some() {
                return Err(format!(
                    "duplicate or unsupported Ghostty palette index {index}"
                ));
            }
        } else if matches!(
            key.as_str(),
            "background"
                | "foreground"
                | "selection-background"
                | "selection-foreground"
                | "cursor-color"
                | "cursor-text"
        ) {
            if values.insert(key.clone(), hex_color(value)?).is_some() {
                return Err(format!("duplicate Ghostty key {key:?}"));
            }
        } else {
            return Err(format!("unsupported Ghostty key {key:?}"));
        }
    }
    let background = values
        .get("background")
        .cloned()
        .ok_or("missing Ghostty background")?;
    let foreground = values
        .get("foreground")
        .cloned()
        .ok_or("missing Ghostty foreground")?;
    let pick = |index: u8| palette.get(&index).cloned();
    Ok(CustomThemeColors {
        panel_bg: Some(background.clone()),
        active_row_bg: values
            .get("selection-background")
            .cloned()
            .or_else(|| pick(8)),
        selection_bg: values
            .get("selection-background")
            .cloned()
            .or_else(|| pick(8)),
        surface0: pick(8),
        surface1: pick(7),
        surface_dim: Some(background),
        text: Some(foreground.clone()),
        subtext0: values
            .get("selection-foreground")
            .cloned()
            .or_else(|| pick(7)),
        accent: values.get("cursor-color").cloned().or_else(|| pick(6)),
        red: pick(1),
        green: pick(2),
        yellow: pick(3),
        blue: pick(4),
        mauve: pick(5),
        teal: pick(6),
        gauge_ok: pick(2),
        gauge_warn: pick(3),
        gauge_crit: pick(1),
        ..Default::default()
    })
}

pub(super) fn load_imports(
    imports: &[ThemeImportConfig],
) -> (Vec<(String, CustomThemeColors)>, Vec<String>) {
    let mut loaded = Vec::new();
    let mut diagnostics = Vec::new();
    let mut names: HashSet<String> = THEME_NAMES.iter().map(|name| (*name).to_owned()).collect();
    for import in imports.iter().take(MAX_THEME_IMPORTS) {
        let name = import.name.trim().to_ascii_lowercase();
        let result = (|| {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err("name must use 1–64 lowercase letters, digits or hyphens".to_owned());
            }
            if super::theme::canonical_theme_name(&name).is_some() || !names.insert(name.clone()) {
                return Err("name collides with a built-in or another import".to_owned());
            }
            if !import.path.is_absolute() {
                return Err("path must be absolute".to_owned());
            }
            let file = std::fs::File::open(&import.path).map_err(|error| error.to_string())?;
            let metadata = file.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.len() > MAX_THEME_FILE_BYTES {
                return Err("theme file must be regular and at most 128 KiB".to_owned());
            }
            let mut content = String::new();
            file.take(MAX_THEME_FILE_BYTES + 1)
                .read_to_string(&mut content)
                .map_err(|error| error.to_string())?;
            if content.len() as u64 > MAX_THEME_FILE_BYTES {
                return Err("theme file exceeds 128 KiB".to_owned());
            }
            match import.format {
                ThemeImportFormat::Base16 => base16(&content),
                ThemeImportFormat::Ghostty => ghostty(&content),
            }
        })();
        match result {
            Ok(colors) => loaded.push((name, colors)),
            Err(error) => diagnostics.push(format!("theme.import {:?}: {error}", import.name)),
        }
    }
    if imports.len() > MAX_THEME_IMPORTS {
        diagnostics.push(format!(
            "theme.import: at most {MAX_THEME_IMPORTS} imports are supported; excess entries ignored"
        ));
    }
    (loaded, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_base16_maps_semantic_colors_and_rejects_missing_key() {
        let input = (0..16)
            .map(|index| format!("base{index:02X}: '{index:02x}{index:02x}{index:02x}'"))
            .collect::<Vec<_>>()
            .join("\n");
        let imported = base16(&input).unwrap();
        assert_eq!(imported.panel_bg.as_deref(), Some("#000000"));
        assert_eq!(imported.gauge_crit.as_deref(), Some("#080808"));
        assert!(base16(&input.replace("base0F", "missing")).is_err());
    }

    #[test]
    fn ghostty_maps_palette_and_rejects_invalid_color() {
        let input = "background = #112233\nforeground = #ddeeff\ncursor-text = #ddeeff\npalette = 1=#aa1122\npalette = 2=#11aa22\npalette = 3=#aaaa22\n";
        let imported = ghostty(input).unwrap();
        assert_eq!(imported.panel_bg.as_deref(), Some("#112233"));
        assert_eq!(imported.gauge_ok.as_deref(), Some("#11aa22"));
        assert_eq!(imported.gauge_crit.as_deref(), Some("#aa1122"));
        assert!(ghostty("background = red\nforeground = #ddeeff").is_err());
    }

    #[test]
    fn configured_file_loads_once_and_collision_is_diagnosed() {
        let path = std::env::temp_dir().join(format!(
            "mercury-theme-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "background = #112233\nforeground = #ddeeff\npalette = 2=#11aa22\n",
        )
        .unwrap();
        let imports = vec![
            ThemeImportConfig {
                name: "ocean".into(),
                path: path.clone(),
                format: ThemeImportFormat::Ghostty,
            },
            ThemeImportConfig {
                name: "mercury".into(),
                path: path.clone(),
                format: ThemeImportFormat::Ghostty,
            },
        ];
        let (loaded, diagnostics) = load_imports(&imports);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, "ocean");
        assert_eq!(loaded[0].1.gauge_ok.as_deref(), Some("#11aa22"));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("collides"));
        std::fs::remove_file(path).unwrap();
    }
}
