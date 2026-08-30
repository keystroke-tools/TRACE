//! Bounded import of Assetto Corsa setup archives.

use std::{
    collections::BTreeSet,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_storage::metadata::{CompatibleSetup, MetadataStore, NewSetupImport};
use zip::ZipArchive;

use crate::ac_content::AcContentNames;

const MAX_ARCHIVES: usize = 32;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4_096;
const MAX_SETUP_FILES: usize = 512;
const MAX_SETUP_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_SETUP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DISCOVERED_SETUPS: usize = 5_000;
const ASSETTO_CORSA_ID: &str = "assetto-corsa";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupImporterDescriptor {
    simulator_id: &'static str,
    simulator_name: &'static str,
    archive_label: &'static str,
    archive_extensions: Vec<&'static str>,
    folder_label: &'static str,
    folder_hint: &'static str,
    archive_hint: &'static str,
    file_label: &'static str,
    file_extensions: Vec<&'static str>,
    file_hint: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupFolder {
    path: Option<String>,
    found: bool,
    source: &'static str,
}

#[tauri::command]
pub(crate) fn setup_importers() -> Vec<SetupImporterDescriptor> {
    vec![SetupImporterDescriptor {
        simulator_id: ASSETTO_CORSA_ID,
        simulator_name: "Assetto Corsa",
        archive_label: "Assetto Corsa setup archives",
        archive_extensions: vec!["zip"],
        folder_label: "Assetto Corsa setups folder",
        folder_hint: "Usually Documents\\Assetto Corsa\\setups. Change it if your Documents folder lives elsewhere.",
        archive_hint: "TRACE uses an .ld telemetry filename to identify the car and track, then installs every .ini setup in the archive.",
        file_label: "Assetto Corsa setup files",
        file_extensions: vec!["ini"],
        file_hint: "Choose standalone .ini files, then provide the track. TRACE reads the CAR / MODEL value when the file contains one.",
    }]
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupImportResult {
    archive_name: String,
    car: Option<String>,
    track: Option<String>,
    files: Vec<String>,
    destination: Option<String>,
    skipped: Vec<String>,
    error: Option<String>,
    index_warning: Option<String>,
    success: bool,
}

impl SetupImportResult {
    fn for_path(path: &Path) -> Self {
        Self {
            archive_name: path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into(),
            ),
            car: None,
            track: None,
            files: Vec::new(),
            destination: None,
            skipped: Vec::new(),
            error: None,
            index_warning: None,
            success: false,
        }
    }

    fn fail(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns deserialized strings.
pub(crate) fn detect_setup_folder(simulator_id: String) -> Result<SetupFolder, String> {
    if simulator_id != ASSETTO_CORSA_ID {
        return Err(format!(
            "setup importing is not available for {simulator_id}"
        ));
    }
    let candidates = setup_folder_candidates();
    if let Some(path) = candidates.iter().find(|path| path.is_dir()) {
        return Ok(SetupFolder {
            path: Some(path.to_string_lossy().into_owned()),
            found: true,
            source: "detected",
        });
    }
    Ok(SetupFolder {
        path: candidates
            .first()
            .map(|path| path.to_string_lossy().into_owned()),
        found: false,
        source: "default",
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns deserialized strings.
pub(crate) fn import_setup_archives(
    app: tauri::AppHandle,
    simulator_id: String,
    archive_paths: Vec<String>,
    setups_folder: String,
    overwrite: bool,
) -> Vec<SetupImportResult> {
    if simulator_id != ASSETTO_CORSA_ID {
        return vec![
            SetupImportResult::for_path(Path::new("setup archives")).fail(format!(
                "setup importing is not available for {simulator_id}"
            )),
        ];
    }
    if archive_paths.is_empty() {
        return vec![
            SetupImportResult::for_path(Path::new("setup archives"))
                .fail("choose at least one .zip setup archive"),
        ];
    }
    if archive_paths.len() > MAX_ARCHIVES {
        return vec![
            SetupImportResult::for_path(Path::new("setup archives")).fail(format!(
                "choose no more than {MAX_ARCHIVES} archives at once"
            )),
        ];
    }
    let destination = PathBuf::from(setups_folder.trim());
    if setups_folder.trim().is_empty() || destination.as_os_str().is_empty() {
        return vec![
            SetupImportResult::for_path(Path::new("setup archives"))
                .fail("choose an Assetto Corsa setups folder"),
        ];
    }
    let mut results: Vec<_> = archive_paths
        .into_iter()
        .map(PathBuf::from)
        .map(|path| import_archive(&path, &destination, overwrite))
        .collect();
    index_imported_setups(&app, &simulator_id, &mut results, None, true);
    results
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupFileImportOptions {
    simulator_id: String,
    setup_paths: Vec<String>,
    setups_folder: String,
    source_car_id: Option<String>,
    source_track_id: String,
    layout_id: Option<String>,
    overwrite: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSetupAttachmentOptions {
    session_id: String,
    setup_path: String,
    setups_folder: String,
    overwrite: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupDiscoveryResult {
    indexed: usize,
    ignored: usize,
    errors: Vec<String>,
    limited: bool,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns deserialized options.
pub(crate) fn import_setup_files(
    app: tauri::AppHandle,
    options: SetupFileImportOptions,
) -> Vec<SetupImportResult> {
    import_setup_files_inner(&app, &options)
}

fn import_setup_files_inner(
    app: &tauri::AppHandle,
    options: &SetupFileImportOptions,
) -> Vec<SetupImportResult> {
    if options.simulator_id != ASSETTO_CORSA_ID {
        return vec![
            SetupImportResult::for_path(Path::new("setup files")).fail(format!(
                "setup importing is not available for {}",
                options.simulator_id
            )),
        ];
    }
    if options.setup_paths.is_empty() || options.setup_paths.len() > MAX_SETUP_FILES {
        return vec![
            SetupImportResult::for_path(Path::new("setup files")).fail(format!(
                "choose between 1 and {MAX_SETUP_FILES} setup files"
            )),
        ];
    }
    let root = PathBuf::from(options.setups_folder.trim());
    if options.setups_folder.trim().is_empty() {
        return vec![
            SetupImportResult::for_path(Path::new("setup files"))
                .fail("choose an Assetto Corsa setups folder"),
        ];
    }
    let track = options.source_track_id.trim().to_ascii_lowercase();
    if !valid_windows_component(&track) {
        return vec![
            SetupImportResult::for_path(Path::new("setup files"))
                .fail("enter a valid source track identifier"),
        ];
    }
    let layout_id = options
        .layout_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if layout_id.is_some_and(|value| !valid_windows_component(value)) {
        return vec![
            SetupImportResult::for_path(Path::new("setup files"))
                .fail("enter a valid layout identifier"),
        ];
    }
    let explicit_car = options
        .source_car_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if explicit_car
        .as_deref()
        .is_some_and(|value| !valid_windows_component(value))
    {
        return vec![
            SetupImportResult::for_path(Path::new("setup files"))
                .fail("enter a valid source car identifier"),
        ];
    }
    let mut results = options
        .setup_paths
        .iter()
        .map(PathBuf::from)
        .map(|path| {
            import_setup_file(
                &path,
                &root,
                explicit_car.as_deref(),
                &track,
                options.overwrite,
            )
        })
        .collect::<Vec<_>>();
    index_imported_setups(app, &options.simulator_id, &mut results, layout_id, false);
    results
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns deserialized options.
pub(crate) fn attach_session_setup(
    app: tauri::AppHandle,
    options: SessionSetupAttachmentOptions,
) -> Result<Vec<CompatibleSetup>, String> {
    let store = open_metadata_store(&app)?;
    let identity = store
        .session_setup_identity(&options.session_id)
        .map_err(|error| format!("failed to read session setup identity: {error:?}"))?;
    drop(store);
    let import_options = SetupFileImportOptions {
        simulator_id: identity.simulator_key,
        setup_paths: vec![options.setup_path],
        setups_folder: options.setups_folder,
        source_car_id: Some(identity.source_car_id),
        source_track_id: identity.source_track_id,
        layout_id: identity.layout_id,
        overwrite: options.overwrite,
    };
    let results = import_setup_files_inner(&app, &import_options);
    let result = results
        .first()
        .ok_or("setup attachment produced no result")?;
    if !result.success || result.index_warning.is_some() {
        return Err(result
            .error
            .clone()
            .or_else(|| result.index_warning.clone())
            .unwrap_or_else(|| "setup could not be attached".into()));
    }
    let installed_path = result
        .destination
        .as_deref()
        .map(Path::new)
        .and_then(|destination| {
            result
                .files
                .first()
                .or_else(|| result.skipped.first())
                .map(|name| destination.join(name))
        })
        .ok_or("setup attachment did not retain its installed path")?;
    let mut store = open_metadata_store(&app)?;
    let setup = store
        .compatible_setups(&options.session_id, 50)
        .map_err(|error| format!("failed to refresh compatible setups: {error:?}"))?
        .into_iter()
        .find(|setup| Path::new(&setup.installed_path) == installed_path)
        .ok_or("attached setup was not indexed as compatible")?;
    let confirmed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("failed to timestamp setup attachment: {error}"))?;
    store
        .confirm_session_setup(&options.session_id, &setup.id, &confirmed_at)
        .map_err(|error| format!("failed to attach setup to session: {error:?}"))?;
    store
        .compatible_setups(&options.session_id, 50)
        .map_err(|error| format!("failed to refresh compatible setups: {error:?}"))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC owns deserialized strings.
pub(crate) fn index_existing_setups(
    app: tauri::AppHandle,
    simulator_id: String,
    setups_folder: String,
) -> Result<SetupDiscoveryResult, String> {
    if simulator_id != ASSETTO_CORSA_ID {
        return Err(format!(
            "setup discovery is not available for {simulator_id}"
        ));
    }
    discover_assetto_corsa_setups(&app, Path::new(setups_folder.trim()))
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

fn index_imported_setups(
    app: &tauri::AppHandle,
    simulator_id: &str,
    results: &mut [SetupImportResult],
    layout_id: Option<&str>,
    from_archive: bool,
) {
    let app_data = match app.path().app_data_dir() {
        Ok(path) => path,
        Err(error) => {
            set_index_warning(results, &format!("could not locate TRACE data: {error}"));
            return;
        }
    };
    if let Err(error) = fs::create_dir_all(&app_data) {
        set_index_warning(results, &format!("could not prepare TRACE data: {error}"));
        return;
    }
    let mut store = match MetadataStore::open(&app_data.join("trace.sqlite")) {
        Ok(store) => store,
        Err(error) => {
            set_index_warning(results, &format!("could not open setup library: {error:?}"));
            return;
        }
    };
    let imported_at = match OffsetDateTime::now_utc().format(&Rfc3339) {
        Ok(value) => value,
        Err(error) => {
            set_index_warning(
                results,
                &format!("could not timestamp setup import: {error}"),
            );
            return;
        }
    };
    let content_names = (simulator_id == ASSETTO_CORSA_ID).then(|| {
        let configured_path = store
            .simulator_install_path(simulator_id)
            .ok()
            .flatten()
            .map(PathBuf::from);
        AcContentNames::discover(configured_path.as_deref())
    });
    for result in results.iter_mut().filter(|result| result.success) {
        if let Err(error) = index_import_result(
            &mut store,
            simulator_id,
            result,
            layout_id,
            from_archive,
            &imported_at,
        ) {
            result.index_warning = Some(error);
        }
        if let Some(names) = content_names.as_ref() {
            result.car = result.car.as_deref().map(|value| names.car(value));
            result.track = result
                .track
                .as_deref()
                .map(|value| names.track(value, None));
        }
    }
}

fn index_import_result(
    store: &mut MetadataStore,
    simulator_id: &str,
    result: &SetupImportResult,
    layout_id: Option<&str>,
    from_archive: bool,
    imported_at: &str,
) -> Result<(), String> {
    let car = result
        .car
        .as_deref()
        .ok_or("car identity was not retained")?;
    let track = result
        .track
        .as_deref()
        .ok_or("track identity was not retained")?;
    let destination = result
        .destination
        .as_deref()
        .map(Path::new)
        .ok_or("setup destination was not retained")?;
    for name in result.files.iter().chain(&result.skipped) {
        let installed_path = destination.join(name);
        let contents = File::open(&installed_path)
            .and_then(|mut file| read_bounded(&mut file, MAX_SETUP_BYTES))
            .map_err(|error| format!("setup was installed but could not be indexed: {error}"))?;
        let content_sha256: [u8; 32] = Sha256::digest(&contents).into();
        let id = setup_id(simulator_id, &installed_path);
        store
            .save_setup_import(&NewSetupImport {
                id,
                simulator_key: simulator_id.into(),
                source_car_id: car.into(),
                source_track_id: track.into(),
                layout_id: layout_id.map(str::to_owned),
                name: name.clone(),
                installed_path: installed_path.to_string_lossy().into_owned(),
                source_archive: from_archive.then(|| result.archive_name.clone()),
                content_sha256,
                imported_at: imported_at.into(),
            })
            .map_err(|error| format!("setup was installed but could not be indexed: {error:?}"))?;
    }
    Ok(())
}

fn set_index_warning(results: &mut [SetupImportResult], warning: &str) {
    for result in results.iter_mut().filter(|result| result.success) {
        result.index_warning = Some(warning.to_owned());
    }
}

fn setup_folder_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(profile) = env::var_os("USERPROFILE").map(PathBuf::from) {
        candidates.push(profile.join("Documents/Assetto Corsa/setups"));
        candidates.push(profile.join("OneDrive/Documents/Assetto Corsa/setups"));
        candidates.push(profile.join("OneDrive - Personal/Documents/Assetto Corsa/setups"));
    }
    for variable in ["OneDrive", "OneDriveConsumer"] {
        if let Some(one_drive) = env::var_os(variable).map(PathBuf::from) {
            candidates.push(one_drive.join("Documents/Assetto Corsa/setups"));
        }
    }
    if candidates.is_empty()
        && let Some(home) = env::var_os("HOME").map(PathBuf::from)
    {
        candidates.push(home.join("Documents/Assetto Corsa/setups"));
    }
    let mut seen = BTreeSet::new();
    candidates.retain(|path| seen.insert(path.clone()));
    candidates
}

fn import_setup_file(
    path: &Path,
    setups_folder: &Path,
    explicit_car: Option<&str>,
    track: &str,
    overwrite: bool,
) -> SetupImportResult {
    let mut result = SetupImportResult::for_path(path);
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ini"))
    {
        return result.fail("setup file must have an .ini extension");
    }
    let Some(name) = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| valid_windows_component(value))
    else {
        return result.fail("setup file has an unsafe name");
    };
    let contents =
        match File::open(path).and_then(|mut file| read_bounded(&mut file, MAX_SETUP_BYTES)) {
            Ok(contents) => contents,
            Err(error) => return result.fail(format!("could not read setup file: {error}")),
        };
    let detected_car = parse_ini_car(&contents).map(|value| value.to_ascii_lowercase());
    if let (Some(detected), Some(explicit)) = (detected_car.as_deref(), explicit_car)
        && !detected.eq_ignore_ascii_case(explicit)
    {
        return result.fail(format!(
            "setup declares car {detected}, which does not match {explicit}"
        ));
    }
    let Some(car) = detected_car
        .or_else(|| explicit_car.map(str::to_owned))
        .filter(|value| valid_windows_component(value))
    else {
        return result.fail("could not detect the car; enter its source identifier");
    };
    let destination = setups_folder.join(&car).join(track);
    if let Err(error) = fs::create_dir_all(&destination) {
        return result.fail(format!(
            "could not create {}: {error}",
            destination.display()
        ));
    }
    let output_path = destination.join(&name);
    let output = if overwrite {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&output_path)
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
    };
    match output {
        Ok(mut output) => {
            if let Err(error) = output.write_all(&contents).and_then(|()| output.flush()) {
                return result.fail(format!("could not install {name}: {error}"));
            }
            result.files.push(name);
        }
        Err(error) if !overwrite && error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = match File::open(&output_path)
                .and_then(|mut file| read_bounded(&mut file, MAX_SETUP_BYTES))
            {
                Ok(existing) => existing,
                Err(read_error) => {
                    return result.fail(format!(
                        "could not verify existing setup {name}: {read_error}"
                    ));
                }
            };
            if existing != contents {
                return result.fail(format!(
                    "a different setup named {name} already exists; enable replacement or rename the file"
                ));
            }
            result.skipped.push(name);
        }
        Err(error) => return result.fail(format!("could not install setup: {error}")),
    }
    result.car = Some(car);
    result.track = Some(track.to_owned());
    result.destination = Some(destination.to_string_lossy().into_owned());
    result.success = true;
    result
}

fn discover_assetto_corsa_setups(
    app: &tauri::AppHandle,
    setups_folder: &Path,
) -> Result<SetupDiscoveryResult, String> {
    if !setups_folder.is_dir() {
        return Err("choose an existing Assetto Corsa setups folder".into());
    }
    let imported_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("failed to timestamp setup discovery: {error}"))?;
    let mut store = open_metadata_store(app)?;
    let mut result = SetupDiscoveryResult {
        indexed: 0,
        ignored: 0,
        errors: Vec::new(),
        limited: false,
    };
    'cars: for car_entry in read_directories(setups_folder)? {
        let Some(car) = safe_directory_name(&car_entry) else {
            result.ignored += 1;
            continue;
        };
        for track_entry in read_directories(&car_entry)? {
            let Some(track) = safe_directory_name(&track_entry) else {
                result.ignored += 1;
                continue;
            };
            let entries = match fs::read_dir(&track_entry) {
                Ok(entries) => entries,
                Err(error) => {
                    result
                        .errors
                        .push(format!("could not read {}: {error}", track_entry.display()));
                    continue;
                }
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_file()
                    || !path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("ini"))
                {
                    result.ignored += 1;
                    continue;
                }
                if result.indexed >= MAX_DISCOVERED_SETUPS {
                    result.limited = true;
                    break 'cars;
                }
                match index_existing_file(&mut store, &path, &car, &track, &imported_at) {
                    Ok(()) => result.indexed += 1,
                    Err(error) => result.errors.push(error),
                }
            }
        }
    }
    Ok(result)
}

fn read_directories(path: &Path) -> Result<Vec<PathBuf>, String> {
    let directories = fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    Ok(directories)
}

fn safe_directory_name(path: &Path) -> Option<String> {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| valid_windows_component(value))
}

fn index_existing_file(
    store: &mut MetadataStore,
    path: &Path,
    car: &str,
    track: &str,
    imported_at: &str,
) -> Result<(), String> {
    let contents = File::open(path)
        .and_then(|mut file| read_bounded(&mut file, MAX_SETUP_BYTES))
        .map_err(|error| format!("could not index {}: {error}", path.display()))?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .ok_or_else(|| format!("setup has no file name: {}", path.display()))?;
    let content_sha256: [u8; 32] = Sha256::digest(&contents).into();
    let id = setup_id(ASSETTO_CORSA_ID, path);
    store
        .save_setup_import(&NewSetupImport {
            id,
            simulator_key: ASSETTO_CORSA_ID.into(),
            source_car_id: car.into(),
            source_track_id: track.into(),
            layout_id: None,
            name,
            installed_path: path.to_string_lossy().into_owned(),
            source_archive: None,
            content_sha256,
            imported_at: imported_at.into(),
        })
        .map_err(|error| format!("could not index {}: {error:?}", path.display()))
}

fn setup_id(simulator_id: &str, path: &Path) -> String {
    let mut identity = Sha256::new();
    identity.update(simulator_id.as_bytes());
    identity.update([0]);
    identity.update(path.to_string_lossy().as_bytes());
    format!("setup-{:x}", identity.finalize())
}

#[allow(clippy::too_many_lines)] // Kept linear so every archive validation precedes filesystem writes.
fn import_archive(path: &Path, setups_folder: &Path, overwrite: bool) -> SetupImportResult {
    let mut result = SetupImportResult::for_path(path);
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return result.fail("setup archive must have a .zip extension");
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return result.fail(format!("could not open archive: {error}")),
    };
    if !metadata.is_file() {
        return result.fail("setup archive is not a file");
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return result.fail("setup archive exceeds the 512 MiB limit");
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return result.fail(format!("could not open archive: {error}")),
    };
    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(error) => return result.fail(format!("could not read ZIP archive: {error}")),
    };
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return result.fail("setup archive contains too many entries");
    }

    let mut track = None;
    let mut car = None;
    let mut ghost_car = None;
    let mut ini_car = None;
    let mut setup_entries = Vec::new();
    let mut setup_names = BTreeSet::new();
    let mut total_setup_bytes = 0_u64;

    for index in 0..archive.len() {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => return result.fail(format!("could not inspect ZIP entry: {error}")),
        };
        if entry.is_dir() {
            continue;
        }
        let Some(name) = archive_basename(entry.name()) else {
            continue;
        };
        let extension = Path::new(&name)
            .extension()
            .and_then(|value| value.to_str());
        if extension.is_some_and(|value| value.eq_ignore_ascii_case("ld")) && track.is_none() {
            if let Some((found_track, found_car)) = parse_ld_name(&name) {
                track = Some(found_track);
                car = Some(found_car);
            }
        } else if extension.is_some_and(|value| value.eq_ignore_ascii_case("ghost"))
            && ghost_car.is_none()
        {
            ghost_car = parse_ghost_name(&name);
        } else if extension.is_some_and(|value| value.eq_ignore_ascii_case("ini")) {
            if setup_entries.len() >= MAX_SETUP_FILES {
                return result.fail("setup archive contains too many .ini files");
            }
            if entry.size() > MAX_SETUP_BYTES {
                return result.fail(format!("setup file {name} exceeds the 4 MiB limit"));
            }
            total_setup_bytes = total_setup_bytes.saturating_add(entry.size());
            if total_setup_bytes > MAX_TOTAL_SETUP_BYTES {
                return result.fail("setup files exceed the 64 MiB expanded-size limit");
            }
            if !valid_windows_component(&name) {
                return result.fail(format!("setup file has an unsafe name: {name}"));
            }
            if !setup_names.insert(name.clone()) {
                return result.fail(format!("setup archive repeats the file name {name}"));
            }
            if ini_car.is_none() {
                match read_bounded(&mut entry, MAX_SETUP_BYTES) {
                    Ok(contents) => ini_car = parse_ini_car(&contents),
                    Err(error) => {
                        return result.fail(format!("could not read setup file {name}: {error}"));
                    }
                }
            }
            setup_entries.push((index, name));
        }
    }

    let car = car
        .or(ghost_car)
        .or(ini_car)
        .map(|value| value.to_ascii_lowercase());
    let Some(car) = car.filter(|value| valid_windows_component(value)) else {
        return result
            .fail("could not determine a safe car identifier; expected track_&_car_&_....ld");
    };
    let Some(track) = track.filter(|value| valid_windows_component(value)) else {
        return result
            .fail("could not determine a safe track identifier; expected track_&_car_&_....ld");
    };
    result.car = Some(car.clone());
    result.track = Some(track.clone());
    if setup_entries.is_empty() {
        return result.fail("no .ini setup files were found in the archive");
    }

    let destination = setups_folder.join(&car).join(&track);
    if let Err(error) = fs::create_dir_all(&destination) {
        return result.fail(format!(
            "could not create {}: {error}",
            destination.display()
        ));
    }
    result.destination = Some(destination.to_string_lossy().into_owned());

    for (index, name) in setup_entries {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => return result.fail(format!("could not reopen {name}: {error}")),
        };
        let contents = match read_bounded(&mut entry, MAX_SETUP_BYTES) {
            Ok(contents) => contents,
            Err(error) => return result.fail(format!("could not extract {name}: {error}")),
        };
        let output_path = destination.join(&name);
        let output = if overwrite {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&output_path)
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output_path)
        };
        let mut output = match output {
            Ok(output) => output,
            Err(error) if !overwrite && error.kind() == io::ErrorKind::AlreadyExists => {
                result.skipped.push(name);
                continue;
            }
            Err(error) => return result.fail(format!("could not write {name}: {error}")),
        };
        if let Err(error) = output.write_all(&contents) {
            return result.fail(format!("could not extract {name}: {error}"));
        }
        if let Err(error) = output.flush() {
            return result.fail(format!("could not finish {name}: {error}"));
        }
        result.files.push(name);
    }
    result.success = true;
    result
}

fn archive_basename(name: &str) -> Option<String> {
    name.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn parse_ld_name(name: &str) -> Option<(String, String)> {
    let base = name
        .rsplit_once('.')
        .map_or(name, |(base, _)| base)
        .to_ascii_lowercase();
    let mut parts = base.split("_&_").map(str::trim);
    let track = parts.next()?.to_owned();
    let car = parts.next()?.to_owned();
    (!track.is_empty() && !car.is_empty()).then_some((track, car))
}

fn parse_ghost_name(name: &str) -> Option<String> {
    let base = name.rsplit_once('.').map_or(name, |(base, _)| base);
    if !base.to_ascii_lowercase().starts_with("ghost_car_") {
        return None;
    }
    base.rsplit_once('_')
        .map(|(_, car)| car.trim().to_ascii_lowercase())
        .filter(|car| !car.is_empty())
}

fn parse_ini_car(contents: &[u8]) -> Option<String> {
    let mut in_car = false;
    for line in String::from_utf8_lossy(contents).lines().map(str::trim) {
        if line.eq_ignore_ascii_case("[CAR]") {
            in_car = true;
            continue;
        }
        if line.starts_with('[') {
            in_car = false;
        } else if in_car
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("MODEL")
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn read_bounded(reader: &mut impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut contents = Vec::new();
    reader.take(limit + 1).read_to_end(&mut contents)?;
    if u64::try_from(contents.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "entry is too large",
        ));
    }
    Ok(contents)
}

fn valid_windows_component(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 128
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
    {
        return false;
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    #[test]
    fn parses_assetto_corsa_identity_fallbacks() {
        assert_eq!(
            parse_ld_name("ks_vallelunga_&_tatuusfa1_&_E. Cavalli_&_stint_22.ld"),
            Some(("ks_vallelunga".into(), "tatuusfa1".into()))
        );
        assert_eq!(
            parse_ghost_name("GHOST_CAR_E. Cavalli_tatuusfa1.ghost"),
            Some("tatuusfa1".into())
        );
        assert_eq!(
            parse_ini_car(b"[CAR]\r\nMODEL=ks_mazda_mx5_cup\r\n"),
            Some("ks_mazda_mx5_cup".into())
        );
    }

    #[test]
    fn imports_setup_inis_and_skips_existing_files() {
        let root = test_directory();
        let archive_path = root.join("shared-setups.zip");
        let setups = root.join("setups");
        write_archive(
            &archive_path,
            &[
                (
                    "data/ks_vallelunga_&_tatuusfa1_&_driver_&_stint_1.ld",
                    b"telemetry",
                ),
                ("setups/qualifying.ini", b"[TYRES]\nPRESSURE_LF=20\n"),
                ("setups/race.ini", b"[CAR]\nMODEL=tatuusfa1\n"),
            ],
        );

        let imported = import_archive(&archive_path, &setups, false);
        assert!(imported.success, "{:?}", imported.error);
        assert_eq!(imported.files, vec!["qualifying.ini", "race.ini"]);
        assert_eq!(imported.car.as_deref(), Some("tatuusfa1"));
        assert_eq!(imported.track.as_deref(), Some("ks_vallelunga"));
        assert!(
            setups
                .join("tatuusfa1/ks_vallelunga/qualifying.ini")
                .is_file()
        );

        let repeated = import_archive(&archive_path, &setups, false);
        assert!(repeated.success);
        assert!(repeated.files.is_empty());
        assert_eq!(repeated.skipped, vec!["qualifying.ini", "race.ini"]);
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn imports_standalone_setup_with_detected_car_identity() {
        let root = test_directory();
        let input = root.join("race.ini");
        let setups = root.join("setups");
        fs::write(
            &input,
            b"[CAR]\nMODEL=ks_mazda_mx5_cup\n[TYRES]\nPRESSURE_LF=20\n",
        )
        .expect("write standalone setup");

        let imported = import_setup_file(&input, &setups, None, "ks_zandvoort", false);

        assert!(imported.success, "{:?}", imported.error);
        assert_eq!(imported.car.as_deref(), Some("ks_mazda_mx5_cup"));
        assert_eq!(imported.track.as_deref(), Some("ks_zandvoort"));
        assert!(
            setups
                .join("ks_mazda_mx5_cup/ks_zandvoort/race.ini")
                .is_file()
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn standalone_setup_rejects_conflicting_explicit_car() {
        let root = test_directory();
        let input = root.join("race.ini");
        fs::write(&input, b"[CAR]\nMODEL=ks_mazda_mx5_cup\n").expect("write setup");

        let imported = import_setup_file(
            &input,
            &root.join("setups"),
            Some("tatuusfa1"),
            "ks_zandvoort",
            false,
        );

        assert!(!imported.success);
        assert!(
            imported
                .error
                .as_deref()
                .is_some_and(|error| error.contains("does not match"))
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn rejects_unsafe_identity_components() {
        assert!(!valid_windows_component(".."));
        assert!(!valid_windows_component("track/escape"));
        assert!(!valid_windows_component("CON"));
        assert!(valid_windows_component("ks_mazda_mx5_cup"));
    }

    fn test_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("trace-setup-import-{nonce}"));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn write_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in entries {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .expect("start file");
            archive.write_all(contents).expect("write file");
        }
        let bytes = archive.finish().expect("finish archive").into_inner();
        fs::write(path, bytes).expect("write archive");
    }
}
