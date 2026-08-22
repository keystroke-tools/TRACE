use std::{
    env, fs,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde::Serialize;

const MAX_UI_METADATA_BYTES: u64 = 1_048_576;
const MAX_TRACK_MAP_BYTES: u64 = 8 * 1_048_576;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcTrackMap {
    pub(crate) data_url: String,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) scale_factor: f64,
    pub(crate) x_offset: f64,
    pub(crate) z_offset: f64,
}

#[derive(Debug, Deserialize)]
struct UiMetadata {
    name: String,
}

/// Resolves AC's internal content identifiers through its installed UI metadata.
pub(crate) struct AcContentNames {
    root: Option<PathBuf>,
}

impl AcContentNames {
    pub(crate) fn discover(configured: Option<&Path>) -> Self {
        Self {
            root: configured
                .filter(|path| path.is_dir())
                .map(Path::to_path_buf)
                .or_else(discover_install_root),
        }
    }

    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    #[cfg(test)]
    fn from_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }

    pub(crate) fn car(&self, source_id: &str) -> String {
        self.root
            .as_deref()
            .filter(|_| safe_content_id(source_id))
            .and_then(|root| {
                read_name(
                    &root
                        .join("content/cars")
                        .join(source_id)
                        .join("ui/ui_car.json"),
                )
            })
            .unwrap_or_else(|| source_id.to_owned())
    }

    pub(crate) fn track(&self, source_id: &str, layout_id: Option<&str>) -> String {
        self.root
            .as_deref()
            .filter(|_| safe_content_id(source_id))
            .and_then(|root| track_metadata_path(root, source_id, layout_id))
            .and_then(|path| read_name(&path))
            .unwrap_or_else(|| source_id.to_owned())
    }

    pub(crate) fn track_map(&self, source_id: &str, layout_id: Option<&str>) -> Option<AcTrackMap> {
        let root = self.root.as_deref()?.join("content/tracks").join(source_id);
        if !safe_content_id(source_id) {
            return None;
        }
        let mut candidates = Vec::new();
        if let Some(layout) = layout_id.filter(|value| safe_content_id(value) && !value.is_empty())
        {
            let layout_root = root.join(layout);
            candidates.push((
                layout_root.join("map.png"),
                layout_root.join("data/map.ini"),
            ));
        }
        candidates.push((root.join("map.png"), root.join("data/map.ini")));
        candidates
            .into_iter()
            .find_map(|(image, parameters)| read_track_map(&image, &parameters))
    }
}

fn read_track_map(image_path: &Path, parameters_path: &Path) -> Option<AcTrackMap> {
    let image_metadata = fs::metadata(image_path).ok()?;
    if image_metadata.len() == 0 || image_metadata.len() > MAX_TRACK_MAP_BYTES {
        return None;
    }
    let parameters = fs::read_to_string(parameters_path).ok()?;
    if parameters.len() > 64 * 1024 {
        return None;
    }
    let value = |name: &str| {
        parameters.lines().find_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            (key.trim() == name)
                .then(|| value.trim().parse::<f64>().ok())
                .flatten()
        })
    };
    let width = value("WIDTH").filter(|value| (1.0..=8_192.0).contains(value))?;
    let height = value("HEIGHT").filter(|value| (1.0..=8_192.0).contains(value))?;
    let scale_factor = value("SCALE_FACTOR").filter(|value| value.is_finite() && *value > 0.0)?;
    let x_offset = value("X_OFFSET").filter(|value| value.is_finite())?;
    let z_offset = value("Z_OFFSET").filter(|value| value.is_finite())?;
    let encoded = STANDARD.encode(fs::read(image_path).ok()?);
    Some(AcTrackMap {
        data_url: format!("data:image/png;base64,{encoded}"),
        width,
        height,
        scale_factor,
        x_offset,
        z_offset,
    })
}

fn discover_install_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("TRACE_ASSETTO_CORSA_PATH") {
        candidates.push(PathBuf::from(path));
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = env::var_os(variable) {
            let steam = PathBuf::from(path).join("Steam");
            candidates.push(steam.join("steamapps/common/assettocorsa"));
            candidates.extend(
                steam_library_roots(&steam)
                    .map(|library| library.join("steamapps/common/assettocorsa")),
            );
        }
    }
    candidates.into_iter().find(|path| path.is_dir())
}

fn steam_library_roots(steam: &Path) -> impl Iterator<Item = PathBuf> {
    let paths = fs::read_to_string(steam.join("steamapps/libraryfolders.vdf"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').filter(|value| !value.trim().is_empty());
            if quoted.next()?.trim() != "path" {
                return None;
            }
            quoted
                .next()
                .map(|path| PathBuf::from(path.replace("\\\\", "\\")))
        })
        .collect::<Vec<_>>();
    paths.into_iter()
}

fn track_metadata_path(root: &Path, source_id: &str, layout_id: Option<&str>) -> Option<PathBuf> {
    let ui = root.join("content/tracks").join(source_id).join("ui");
    if let Some(layout) = layout_id.filter(|value| safe_content_id(value) && !value.is_empty()) {
        let configured = ui.join(layout).join("ui_track.json");
        if configured.is_file() {
            return Some(configured);
        }
    }
    let default = ui.join("ui_track.json");
    if default.is_file() {
        return Some(default);
    }
    let mut configured = fs::read_dir(ui)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("ui_track.json"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    configured.sort();
    configured.into_iter().next()
}

fn read_name(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_UI_METADATA_BYTES {
        return None;
    }
    let contents = fs::read_to_string(path).ok()?;
    let metadata: UiMetadata =
        serde_json::from_str(contents.trim_start_matches('\u{feff}')).ok()?;
    let name = metadata.name.trim();
    (!name.is_empty() && name.chars().count() <= 120).then(|| name.to_owned())
}

fn safe_content_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_metadata_preserves_exact_source_identifiers() {
        let names = AcContentNames { root: None };
        assert_eq!(names.track("zandvoort2023", None), "zandvoort2023");
        assert_eq!(names.track("ks_red_bull_ring", None), "ks_red_bull_ring");
        assert_eq!(names.car("tmm_trabant_601"), "tmm_trabant_601");
    }

    #[test]
    fn installed_ui_metadata_wins_over_generic_title_casing() {
        let root = env::temp_dir().join(format!("trace-ac-content-{}", std::process::id()));
        let ui = root.join("content/cars/ks_mazda_mx5_cup/ui");
        fs::create_dir_all(&ui).expect("fixture directory");
        fs::write(ui.join("ui_car.json"), r#"{"name":"Mazda MX5 Cup"}"#).expect("fixture metadata");
        let names = AcContentNames::from_root(root.clone());
        assert_eq!(names.car("ks_mazda_mx5_cup"), "Mazda MX5 Cup");
        fs::remove_dir_all(root).expect("fixture cleanup");
    }
}
