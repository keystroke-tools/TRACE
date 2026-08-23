//! Bounded, simulator-dispatched setup inspection for approachable comparisons.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

use serde::Serialize;
use tauri::Manager;
use trace_storage::metadata::{MetadataStore, SetupFileRecord};

const MAX_SETUP_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SETUP_VALUES: usize = 4_096;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupComparison {
    baseline_name: String,
    alternative_name: String,
    changed_values: usize,
    unchanged_values: usize,
    sections: Vec<SetupComparisonSection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupComparisonSection {
    name: String,
    changes: Vec<SetupValueChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupValueChange {
    key: String,
    baseline_value: Option<String>,
    alternative_value: Option<String>,
}

type SetupDocument = BTreeMap<String, BTreeMap<String, String>>;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn compare_setups(
    app: tauri::AppHandle,
    baseline_setup_id: String,
    alternative_setup_id: String,
) -> Result<SetupComparison, String> {
    if baseline_setup_id == alternative_setup_id {
        return Err("choose two different setups".into());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let baseline = setup_record(&store, &baseline_setup_id)?;
    let alternative = setup_record(&store, &alternative_setup_id)?;
    ensure_compatible(&baseline, &alternative)?;
    match baseline.simulator_key.as_str() {
        "assetto-corsa" => {
            let baseline_document = parse_ac_setup(&read_setup(&baseline.installed_path)?)?;
            let alternative_document = parse_ac_setup(&read_setup(&alternative.installed_path)?)?;
            Ok(compare_documents(
                baseline.name,
                alternative.name,
                &baseline_document,
                &alternative_document,
            ))
        }
        simulator => Err(format!("setup comparison is not available for {simulator}")),
    }
}

fn setup_record(store: &MetadataStore, id: &str) -> Result<SetupFileRecord, String> {
    store
        .setup_file(id)
        .map_err(|error| format!("failed to read setup library: {error:?}"))?
        .ok_or_else(|| "setup is no longer in the local library".into())
}

fn ensure_compatible(left: &SetupFileRecord, right: &SetupFileRecord) -> Result<(), String> {
    if left.simulator_key != right.simulator_key
        || !left
            .source_car_id
            .eq_ignore_ascii_case(&right.source_car_id)
        || !left
            .source_track_id
            .eq_ignore_ascii_case(&right.source_track_id)
        || !left
            .layout_id
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case(right.layout_id.as_deref().unwrap_or(""))
    {
        return Err("setups must use the same simulator, car, track, and layout".into());
    }
    Ok(())
}

fn read_setup(path: &str) -> Result<Vec<u8>, String> {
    let file = File::open(Path::new(path))
        .map_err(|error| format!("failed to open setup file: {error}"))?;
    let mut contents = Vec::new();
    file.take(MAX_SETUP_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|error| format!("failed to read setup file: {error}"))?;
    if contents.is_empty() || u64::try_from(contents.len()).unwrap_or(u64::MAX) > MAX_SETUP_BYTES {
        return Err("setup file is empty or exceeds the 4 MiB limit".into());
    }
    Ok(contents)
}

fn parse_ac_setup(contents: &[u8]) -> Result<SetupDocument, String> {
    let mut document = SetupDocument::new();
    let mut section = "GENERAL".to_owned();
    let mut values = 0_usize;
    for line in String::from_utf8_lossy(contents).lines().map(str::trim) {
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim();
            if !name.is_empty() {
                section = name.to_ascii_uppercase();
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        values += 1;
        if values > MAX_SETUP_VALUES {
            return Err("setup contains too many values".into());
        }
        document
            .entry(section.clone())
            .or_default()
            .insert(key.to_ascii_uppercase(), value.trim().to_owned());
    }
    if document.is_empty() {
        return Err("setup does not contain readable INI values".into());
    }
    Ok(document)
}

fn compare_documents(
    baseline_name: String,
    alternative_name: String,
    baseline: &SetupDocument,
    alternative: &SetupDocument,
) -> SetupComparison {
    let sections: BTreeSet<_> = baseline.keys().chain(alternative.keys()).cloned().collect();
    let mut unchanged_values = 0;
    let mut changed_values = 0;
    let sections = sections
        .into_iter()
        .filter_map(|section| {
            let baseline_values = baseline.get(&section);
            let alternative_values = alternative.get(&section);
            let keys: BTreeSet<_> = baseline_values
                .into_iter()
                .flat_map(|values| values.keys())
                .chain(
                    alternative_values
                        .into_iter()
                        .flat_map(|values| values.keys()),
                )
                .cloned()
                .collect();
            let changes = keys
                .into_iter()
                .filter_map(|key| {
                    let baseline_value = baseline_values.and_then(|values| values.get(&key));
                    let alternative_value = alternative_values.and_then(|values| values.get(&key));
                    if baseline_value == alternative_value {
                        unchanged_values += 1;
                        return None;
                    }
                    changed_values += 1;
                    Some(SetupValueChange {
                        key,
                        baseline_value: baseline_value.cloned(),
                        alternative_value: alternative_value.cloned(),
                    })
                })
                .collect::<Vec<_>>();
            (!changes.is_empty()).then_some(SetupComparisonSection {
                name: section,
                changes,
            })
        })
        .collect();
    SetupComparison {
        baseline_name,
        alternative_name,
        changed_values,
        unchanged_values,
        sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_compares_ac_ini_values_without_reporting_unchanged_noise() {
        let baseline =
            parse_ac_setup(b"; setup\n[TYRES]\nPRESSURE_LF=20\nPRESSURE_RF=20\n[ARB]\nFRONT=3\n")
                .expect("baseline");
        let alternative =
            parse_ac_setup(b"[TYRES]\nPRESSURE_LF=21\nPRESSURE_RF=20\n[ARB]\nFRONT=4\nREAR=2\n")
                .expect("alternative");
        let comparison = compare_documents(
            "race.ini".into(),
            "qualifying.ini".into(),
            &baseline,
            &alternative,
        );
        assert_eq!(comparison.changed_values, 3);
        assert_eq!(comparison.unchanged_values, 1);
        assert_eq!(comparison.sections.len(), 2);
        assert_eq!(comparison.sections[0].name, "ARB");
        assert_eq!(comparison.sections[1].changes[0].key, "PRESSURE_LF");
    }
}
