//! Stable project markers shared by history and the workspace/agent sidebar.

use ratatui::style::Color;

use super::{ClientEndpointId, Palette};

pub(super) fn color(path: Option<&str>, endpoint: &ClientEndpointId, palette: &Palette) -> Color {
    let Some(path) = normalized_path(path.unwrap_or_default()) else {
        return palette.overlay0;
    };
    // FNV-1a has no randomized seed and remains identical across process restarts.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in endpoint
        .storage_key()
        .bytes()
        .chain([0])
        .chain(path.bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    let colors = [
        palette.blue,
        palette.mauve,
        palette.teal,
        palette.peach,
        palette.green,
        palette.yellow,
    ];
    colors[(hash % colors.len() as u64) as usize]
}

pub(super) fn marker(buffer: &mut ratatui::buffer::Buffer, x: u16, y: u16, color: Color) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_symbol("▏").set_fg(color);
    }
}

pub(super) fn workspace_marker(
    buffer: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    indented: bool,
    color: Color,
) {
    if !area.is_empty() {
        marker(
            buffer,
            area.x + u16::from(indented && area.width > 1),
            area.y,
            color,
        );
    }
}

fn normalized_path(path: &str) -> Option<String> {
    let windows = path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\");
    let path = if windows {
        path.replace('\\', "/").to_lowercase()
    } else {
        path.to_owned()
    };
    if !(path.starts_with('/') || (windows && path.as_bytes().get(2) == Some(&b'/'))) {
        return None;
    }
    let prefix = if path.starts_with("//") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part: &&str| !part.ends_with(':')) {
                    parts.pop();
                }
            }
            _ => parts.push(part),
        }
    }
    Some(format!("{prefix}{}", parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_path_normalizes_without_filesystem_access() {
        assert_eq!(normalized_path("/repo/./app/../"), Some("/repo".into()));
        assert_eq!(normalized_path("/"), Some("/".into()));
        assert_eq!(
            normalized_path("C:\\Repos\\App\\"),
            Some("c:/repos/app".into())
        );
        assert_eq!(
            normalized_path("\\\\Host\\Share\\App"),
            Some("//host/share/app".into())
        );
        assert_eq!(normalized_path("relative/path"), None);
    }

    #[test]
    fn markers_are_stable_in_both_themes_and_unknown_paths_are_neutral() {
        for palette in [Palette::mercury(), Palette::mercury_light()] {
            assert_eq!(
                color(Some("/repo/./"), &ClientEndpointId::Local, &palette),
                color(Some("/repo"), &ClientEndpointId::Local, &palette)
            );
            assert_eq!(
                color(None, &ClientEndpointId::Local, &palette),
                palette.overlay0
            );
            let colors: Vec<_> = ["/project/a", "/project/b", "/project/c"]
                .into_iter()
                .map(|path| color(Some(path), &ClientEndpointId::Local, &palette))
                .collect();
            assert!(colors.windows(2).any(|pair| pair[0] != pair[1]));
        }
    }
}
