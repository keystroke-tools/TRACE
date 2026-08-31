//! Simulator-aware setup inspection and safe copy editing.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_storage::metadata::{MetadataStore, NewSetupImport, SetupFileRecord};

use crate::ac_content::AcContentNames;

const ASSETTO_CORSA_ID: &str = "assetto-corsa";
const MAX_SETUP_BYTES: u64 = 4 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EDITABLE_VALUES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupDocument {
    setup_id: String,
    name: String,
    simulator_id: String,
    source_car_id: String,
    metadata_available: bool,
    groups: Vec<SetupDocumentGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupDocumentGroup {
    name: String,
    values: Vec<SetupDocumentValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupDocumentValue {
    section: String,
    label: String,
    value: String,
    editable: bool,
    description: Option<String>,
    minimum: Option<String>,
    maximum: Option<String>,
    step: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveSetupCopyOptions {
    source_setup_id: String,
    name: String,
    values: Vec<SetupValueInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupValueInput {
    section: String,
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupSaveResult {
    setup_id: String,
    name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SetupMetadata {
    label: Option<String>,
    category: Option<String>,
    help_key: Option<String>,
    minimum: Option<String>,
    maximum: Option<String>,
    step: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AcSetupMetadata {
    fields: BTreeMap<String, SetupMetadata>,
    labels: BTreeMap<String, String>,
    tabs: BTreeMap<String, String>,
    help: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IniSection {
    name: String,
    values: Vec<(String, String)>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns the application handle and identifier.
pub(crate) fn setup_document(
    app: tauri::AppHandle,
    setup_id: String,
) -> Result<SetupDocument, String> {
    let store = open_metadata_store(&app)?;
    let record = store
        .setup_file(setup_id.trim())
        .map_err(|error| format!("could not read setup metadata: {error:?}"))?
        .ok_or("setup is no longer in the local library")?;
    let configured_path = store
        .simulator_install_path(ASSETTO_CORSA_ID)
        .map_err(|error| format!("could not read simulator settings: {error:?}"))?
        .map(PathBuf::from);
    drop(store);

    let source = read_text_bounded(Path::new(&record.installed_path), MAX_SETUP_BYTES)
        .map_err(|error| format!("could not read setup file: {error}"))?;
    let ac_root = (record.simulator_key == ASSETTO_CORSA_ID)
        .then(|| AcContentNames::discover(configured_path.as_deref()))
        .and_then(|content| content.root().map(Path::to_path_buf));
    Ok(parse_document(&record, &source, ac_root.as_deref()))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns deserialized options.
pub(crate) fn save_setup_copy(
    app: tauri::AppHandle,
    options: SaveSetupCopyOptions,
) -> Result<SetupSaveResult, String> {
    let mut store = open_metadata_store(&app)?;
    let record = store
        .setup_file(options.source_setup_id.trim())
        .map_err(|error| format!("could not read setup metadata: {error:?}"))?
        .ok_or("source setup is no longer in the local library")?;
    if record.simulator_key != ASSETTO_CORSA_ID {
        return Err(format!(
            "setup editing is not available for {}",
            record.simulator_key
        ));
    }
    let source_path = Path::new(&record.installed_path);
    let source = read_text_bounded(source_path, MAX_SETUP_BYTES)
        .map_err(|error| format!("could not read source setup: {error}"))?;
    let output = rewrite_values(&source, &options.values)?;
    let name = normalise_setup_name(&options.name)?;
    let parent = source_path
        .parent()
        .ok_or("source setup does not have a destination folder")?;
    let output_path = parent.join(&name);
    if output_path == source_path {
        return Err("choose a different name; TRACE never overwrites the source setup".into());
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                format!("a setup named {name} already exists")
            } else {
                format!("could not create setup copy: {error}")
            }
        })?;
    file.write_all(output.as_bytes())
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not finish setup copy: {error}"))?;

    let imported_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("could not timestamp setup copy: {error}"))?;
    let setup_id = setup_id(&record.simulator_key, &output_path);
    let content_sha256: [u8; 32] = Sha256::digest(output.as_bytes()).into();
    store
        .save_setup_import(&NewSetupImport {
            id: setup_id.clone(),
            simulator_key: record.simulator_key,
            source_car_id: record.source_car_id,
            source_track_id: record.source_track_id,
            layout_id: record.layout_id,
            name: name.clone(),
            installed_path: output_path.to_string_lossy().into_owned(),
            source_archive: None,
            content_sha256,
            imported_at,
        })
        .map_err(|error| {
            format!(
                "setup was saved as {} but could not be indexed: {error:?}",
                output_path.display()
            )
        })?;
    Ok(SetupSaveResult { setup_id, name })
}

fn parse_document(record: &SetupFileRecord, source: &str, ac_root: Option<&Path>) -> SetupDocument {
    let sections = parse_ini(source);
    let metadata = ac_root.map_or_else(AcSetupMetadata::default, |root| {
        load_ac_metadata(root, &record.source_car_id)
    });
    let metadata_available = !metadata.fields.is_empty();
    let mut groups: Vec<SetupDocumentGroup> = Vec::new();
    for section in sections {
        let Some((key, value)) = section
            .values
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("VALUE"))
            .or_else(|| section.values.first())
        else {
            continue;
        };
        let field_metadata = metadata.fields.get(&section.name);
        let raw_category = field_metadata
            .and_then(|value| value.category.as_deref())
            .unwrap_or_else(|| {
                if section.name == "CAR" {
                    "IDENTITY"
                } else {
                    "OTHER"
                }
            });
        let category = metadata
            .tabs
            .get(raw_category)
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| humanise_identifier(raw_category));
        let label = field_metadata
            .and_then(|value| value.label.as_ref())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                metadata
                    .labels
                    .get(&section.name)
                    .filter(|value| !value.trim().is_empty())
            })
            .cloned()
            .unwrap_or_else(|| humanise_identifier(&section.name));
        let description = field_metadata
            .and_then(|value| value.help_key.as_ref())
            .and_then(|key| metadata.help.get(key))
            .cloned();
        let entry = SetupDocumentValue {
            section: section.name,
            label,
            value: value.clone(),
            editable: key.eq_ignore_ascii_case("VALUE"),
            description,
            minimum: field_metadata.and_then(|value| value.minimum.clone()),
            maximum: field_metadata.and_then(|value| value.maximum.clone()),
            step: field_metadata.and_then(|value| value.step.clone()),
        };
        if let Some(group) = groups.iter_mut().find(|group| group.name == category) {
            group.values.push(entry);
        } else {
            groups.push(SetupDocumentGroup {
                name: category,
                values: vec![entry],
            });
        }
    }
    SetupDocument {
        setup_id: record.id.clone(),
        name: record.name.clone(),
        simulator_id: record.simulator_key.clone(),
        source_car_id: record.source_car_id.clone(),
        metadata_available,
        groups,
    }
}

fn load_ac_metadata(root: &Path, source_car_id: &str) -> AcSetupMetadata {
    if !valid_path_component(source_car_id) {
        return AcSetupMetadata::default();
    }
    let setup_path = root
        .join("content/cars")
        .join(source_car_id)
        .join("data/setup.ini");
    let metadata = read_text_bounded(&setup_path, MAX_METADATA_BYTES)
        .ok()
        .map(|source| parse_ac_setup_metadata(&source))
        .unwrap_or_default();
    let locale_root = root.join("system/locales/setup");
    let locale = read_text_bounded(&locale_root.join("en.ini"), MAX_METADATA_BYTES)
        .ok()
        .map(|source| parse_ini(&source))
        .unwrap_or_default();
    let labels = ini_section_map(&locale, "SETUP");
    let tabs = ini_section_map(&locale, "TABS");
    let help = read_text_bounded(&locale_root.join("en.tag"), MAX_METADATA_BYTES)
        .ok()
        .map(|source| parse_tag_sections(&source))
        .unwrap_or_default();
    AcSetupMetadata {
        fields: metadata,
        labels,
        tabs,
        help,
    }
}

fn parse_ac_setup_metadata(source: &str) -> BTreeMap<String, SetupMetadata> {
    parse_ini(source)
        .into_iter()
        .map(|section| {
            let values = section.values.into_iter().collect::<BTreeMap<_, _>>();
            let metadata = SetupMetadata {
                label: values.get("NAME").cloned(),
                category: values.get("TAB").cloned(),
                help_key: values.get("HELP").cloned(),
                minimum: values.get("MIN").cloned(),
                maximum: values.get("MAX").cloned(),
                step: values.get("STEP").cloned(),
            };
            (section.name, metadata)
        })
        .collect()
}

fn parse_ini(source: &str) -> Vec<IniSection> {
    let mut sections: Vec<IniSection> = Vec::new();
    for raw_line in source.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|value| value.split_once(']'))
            .map(|(name, _)| name.trim())
        {
            if !name.is_empty() {
                sections.push(IniSection {
                    name: name.to_ascii_uppercase(),
                    values: Vec::new(),
                });
            }
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let Some(section) = sections.last_mut() else {
            continue;
        };
        let value = raw_value.split(';').next().unwrap_or_default().trim();
        section
            .values
            .push((key.trim().to_ascii_uppercase(), value.to_owned()));
    }
    sections
}

fn parse_tag_sections(source: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut body: Vec<&str> = Vec::new();
    for raw_line in source.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim_end();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            if let Some(name) = current.take() {
                let text = body.join("\n").trim().to_owned();
                if !text.is_empty() {
                    result.insert(name, text);
                }
            }
            current = Some(name.trim().to_ascii_uppercase());
            body.clear();
        } else if current.is_some() {
            body.push(line);
        }
    }
    if let Some(name) = current {
        let text = body.join("\n").trim().to_owned();
        if !text.is_empty() {
            result.insert(name, text);
        }
    }
    result
}

fn ini_section_map(sections: &[IniSection], name: &str) -> BTreeMap<String, String> {
    sections
        .iter()
        .find(|section| section.name == name)
        .map(|section| section.values.iter().cloned().collect())
        .unwrap_or_default()
}

fn rewrite_values(source: &str, values: &[SetupValueInput]) -> Result<String, String> {
    if values.is_empty() || values.len() > MAX_EDITABLE_VALUES {
        return Err(format!(
            "provide between 1 and {MAX_EDITABLE_VALUES} setup values"
        ));
    }
    let mut replacements = BTreeMap::new();
    for input in values {
        let section = input.section.trim().to_ascii_uppercase();
        if section.is_empty()
            || section.len() > 128
            || input.value.len() > 128
            || input.value.trim().is_empty()
            || input.value.chars().any(char::is_control)
        {
            return Err("setup contains an invalid section or value".into());
        }
        if replacements.insert(section, input.value.trim()).is_some() {
            return Err("setup contains a duplicate edited section".into());
        }
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let trailing_newline = source.ends_with('\n');
    let mut current_section = String::new();
    let mut replaced = BTreeSet::new();
    let mut output = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|value| value.split_once(']'))
            .map(|(name, _)| name.trim())
        {
            current_section = name.to_ascii_uppercase();
            output.push(line.to_owned());
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            output.push(line.to_owned());
            continue;
        };
        let Some(value) = replacements.get(&current_section) else {
            output.push(line.to_owned());
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("VALUE") {
            output.push(line.to_owned());
            continue;
        }
        let equals = line.find('=').ok_or("setup value is malformed")?;
        let comment = line[equals + 1..]
            .find(';')
            .map(|index| line[equals + 1 + index..].trim_start());
        let mut rewritten = format!("{}{}", &line[..=equals], value);
        if let Some(comment) = comment {
            rewritten.push(' ');
            rewritten.push_str(comment);
        }
        output.push(rewritten);
        replaced.insert(current_section.clone());
    }
    if replaced.len() != replacements.len() {
        return Err("one or more edited values no longer exist in the source setup".into());
    }
    let mut output = output.join(newline);
    if trailing_newline {
        output.push_str(newline);
    }
    Ok(output)
}

fn normalise_setup_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let name = if name.to_ascii_lowercase().ends_with(".ini") {
        name.to_owned()
    } else {
        format!("{name}.ini")
    };
    if !valid_path_component(&name) || name.len() > 255 {
        return Err("enter a safe setup filename".into());
    }
    Ok(name)
}

fn valid_path_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.ends_with([' ', '.'])
        && !value
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
}

fn humanise_identifier(value: &str) -> String {
    value
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                format!(
                    "{}{}",
                    first.to_uppercase(),
                    characters.as_str().to_lowercase()
                )
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_text_bounded(path: &Path, maximum: u64) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err("file is empty, unavailable, or too large".into());
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| "file is too large")?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .and_then(|file| file.take(maximum + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err("file is too large".into());
    }
    String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8 text".into())
}

fn open_metadata_store(app: &tauri::AppHandle) -> Result<MetadataStore, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("could not locate TRACE data: {error}"))?;
    fs::create_dir_all(&app_data)
        .map_err(|error| format!("could not prepare TRACE data: {error}"))?;
    MetadataStore::open(&app_data.join("trace.sqlite"))
        .map_err(|error| format!("could not open setup library: {error:?}"))
}

fn setup_id(simulator_id: &str, path: &Path) -> String {
    let mut identity = Sha256::new();
    identity.update(simulator_id.as_bytes());
    identity.update([0]);
    identity.update(path.to_string_lossy().as_bytes());
    format!("setup-{:x}", identity.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> SetupFileRecord {
        SetupFileRecord {
            id: "setup-1".into(),
            simulator_key: ASSETTO_CORSA_ID.into(),
            source_car_id: "car-1".into(),
            source_track_id: "track-1".into(),
            layout_id: None,
            name: "race.ini".into(),
            installed_path: "race.ini".into(),
        }
    }

    #[test]
    fn parses_editable_values_and_read_only_identity() {
        let document = parse_document(
            &record(),
            "[ARB_FRONT]\nVALUE=4\n\n[CAR]\nMODEL=car-1\n",
            None,
        );
        assert_eq!(document.groups.len(), 2);
        assert!(document.groups[0].values[0].editable);
        assert_eq!(document.groups[0].values[0].label, "Arb Front");
        assert!(!document.groups[1].values[0].editable);
    }

    #[test]
    fn parses_ac_ranges_labels_and_help_text() {
        let metadata = parse_ac_setup_metadata(
            "[ARB_FRONT]\nTAB=SUSPENSION\nNAME=Front ARB\nMIN=0\nMAX=5\nSTEP=1\nHELP=HELP_FRONT_ARB\n",
        );
        assert_eq!(metadata["ARB_FRONT"].label.as_deref(), Some("Front ARB"));
        assert_eq!(metadata["ARB_FRONT"].minimum.as_deref(), Some("0"));
        let help = parse_tag_sections("[HELP_FRONT_ARB]\nRaise the value for more response.\n");
        assert_eq!(help["HELP_FRONT_ARB"], "Raise the value for more response.");
    }

    #[test]
    fn rewrites_only_requested_values_and_preserves_comments() {
        let source = "[ARB_FRONT]\r\nVALUE=4 ; original\r\n\r\n[FUEL]\r\nVALUE=20\r\n";
        let output = rewrite_values(
            source,
            &[SetupValueInput {
                section: "ARB_FRONT".into(),
                value: "5".into(),
            }],
        )
        .expect("rewrite");
        assert!(output.contains("VALUE=5 ; original"));
        assert!(output.contains("[FUEL]\r\nVALUE=20"));
        assert!(output.ends_with("\r\n"));
    }

    #[test]
    fn copy_names_are_safe_and_never_accept_paths() {
        assert_eq!(
            normalise_setup_name("wet race").as_deref(),
            Ok("wet race.ini")
        );
        assert!(normalise_setup_name("../race.ini").is_err());
        assert!(normalise_setup_name("race?.ini").is_err());
    }
}
