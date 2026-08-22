use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde::Serialize;

const MAX_UI_METADATA_BYTES: u64 = 1_048_576;
const MAX_AI_SPLINE_BYTES: u64 = 32 * 1_048_576;
const MAX_AI_POINTS: usize = 100_000;
const MAX_RENDER_POINTS: usize = 4_000;
const AI_HEADER_BYTES: usize = 16;
const AI_POINT_BYTES: usize = 20;
const AI_DETAIL_BYTES: usize = 72;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcTrackGeometry {
    pub(crate) centre_line: Vec<AcTrackPoint>,
    pub(crate) left_boundary: Vec<AcTrackPoint>,
    pub(crate) right_boundary: Vec<AcTrackPoint>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcTrackPoint {
    pub(crate) x_m: f32,
    pub(crate) z_m: f32,
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

    pub(crate) fn track_geometry(
        &self,
        source_id: &str,
        layout_id: Option<&str>,
    ) -> Option<AcTrackGeometry> {
        let root = self.root.as_deref()?.join("content/tracks").join(source_id);
        if !safe_content_id(source_id) {
            return None;
        }
        track_layout_roots(&root, layout_id)
            .into_iter()
            .find_map(|layout| read_ai_spline(&layout.join("ai/fast_lane.ai")))
    }
}

fn read_ai_spline(path: &Path) -> Option<AcTrackGeometry> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() < 128 || metadata.len() > MAX_AI_SPLINE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let version = read_i32(&bytes, 0)?;
    let point_count = usize::try_from(read_i32(&bytes, 4)?).ok()?;
    if version != 7 || !(3..=MAX_AI_POINTS).contains(&point_count) {
        return None;
    }
    let points_end = AI_HEADER_BYTES.checked_add(point_count.checked_mul(AI_POINT_BYTES)?)?;
    let details_start = points_end;
    let details_end = details_start.checked_add(point_count.checked_mul(AI_DETAIL_BYTES)?)?;
    if details_end > bytes.len() {
        return None;
    }

    let mut centre = Vec::with_capacity(point_count);
    let mut widths = Vec::with_capacity(point_count);
    for index in 0..point_count {
        let point = AI_HEADER_BYTES + index * AI_POINT_BYTES;
        let x = read_f32(&bytes, point)?;
        let z = read_f32(&bytes, point + 8)?;
        let detail = details_start + index * AI_DETAIL_BYTES;
        let left = read_f32(&bytes, detail + 24)?;
        let right = read_f32(&bytes, detail + 28)?;
        if !x.is_finite()
            || !z.is_finite()
            || !left.is_finite()
            || !right.is_finite()
            || !(0.0..=100.0).contains(&left)
            || !(0.0..=100.0).contains(&right)
        {
            return None;
        }
        centre.push(AcTrackPoint { x_m: x, z_m: z });
        widths.push((left, right));
    }

    let mut left_boundary = Vec::with_capacity(point_count);
    let mut right_boundary = Vec::with_capacity(point_count);
    for index in 0..point_count {
        let previous = centre[(index + point_count - 1) % point_count];
        let next = centre[(index + 1) % point_count];
        let dx = next.x_m - previous.x_m;
        let dz = next.z_m - previous.z_m;
        let magnitude = dx.hypot(dz);
        if !magnitude.is_finite() || magnitude < 0.001 {
            return None;
        }
        let (left, right) = widths[index];
        left_boundary.push(AcTrackPoint {
            x_m: centre[index].x_m + dz / magnitude * left,
            z_m: centre[index].z_m - dx / magnitude * left,
        });
        right_boundary.push(AcTrackPoint {
            x_m: centre[index].x_m - dz / magnitude * right,
            z_m: centre[index].z_m + dx / magnitude * right,
        });
    }

    let stride = point_count.div_ceil(MAX_RENDER_POINTS).max(1);
    Some(AcTrackGeometry {
        centre_line: centre.into_iter().step_by(stride).collect(),
        left_boundary: left_boundary.into_iter().step_by(stride).collect(),
        right_boundary: right_boundary.into_iter().step_by(stride).collect(),
    })
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_f32(bytes: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn track_layout_roots(root: &Path, layout_id: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(layout) = layout_id.filter(|value| !value.trim().is_empty()) {
        if safe_content_id(layout) {
            candidates.push(root.join(layout));
        }
        let wanted = normalise_layout(layout);
        if let Ok(entries) = fs::read_dir(root) {
            let mut matches = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let key = normalise_layout(&name);
                    (key == wanted || key.strip_prefix("layout") == Some(wanted.as_str()))
                        .then(|| entry.path())
                })
                .collect::<Vec<_>>();
            matches.sort();
            candidates.extend(matches);
        }
    }
    candidates.push(root.to_path_buf());
    if let Ok(entries) = fs::read_dir(root) {
        let mut spline_layouts = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join("ai/fast_lane.ai").is_file())
            .collect::<Vec<_>>();
        spline_layouts.sort();
        if spline_layouts.len() == 1 {
            candidates.push(spline_layouts.remove(0));
        }
    }
    candidates.dedup();
    candidates
}

fn normalise_layout(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

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

    #[test]
    fn version_seven_ai_spline_produces_world_space_road_edges() {
        let point_count = 4;
        let detail_start = AI_HEADER_BYTES + point_count * AI_POINT_BYTES;
        let mut bytes = vec![0; detail_start + point_count * AI_DETAIL_BYTES];
        put_i32(&mut bytes, 0, 7);
        put_i32(&mut bytes, 4, point_count as i32);
        for (index, (x, z)) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .into_iter()
            .enumerate()
        {
            let point = AI_HEADER_BYTES + index * AI_POINT_BYTES;
            put_f32(&mut bytes, point, x);
            put_f32(&mut bytes, point + 8, z);
            let detail = detail_start + index * AI_DETAIL_BYTES;
            put_f32(&mut bytes, detail + 24, 4.0);
            put_f32(&mut bytes, detail + 28, 6.0);
        }
        let root = env::temp_dir().join(format!("trace-ac-spline-{}", std::process::id()));
        fs::create_dir_all(&root).expect("fixture directory");
        let path = root.join("fast_lane.ai");
        fs::write(&path, bytes).expect("fixture spline");

        let geometry = read_ai_spline(&path).expect("valid spline geometry");
        assert_eq!(geometry.centre_line.len(), point_count);
        assert_eq!(geometry.left_boundary.len(), point_count);
        assert_eq!(geometry.right_boundary.len(), point_count);
        assert_ne!(
            geometry.left_boundary[0].x_m,
            geometry.right_boundary[0].x_m
        );
        assert_ne!(
            geometry.left_boundary[0].z_m,
            geometry.right_boundary[0].z_m
        );
        fs::remove_dir_all(root).expect("fixture cleanup");
    }
}
