import { telemetryDataSource } from "../../data-source";

export const AUTO_INDEX_SETUPS_KEY = "trace.setup-library.auto-index";
const SETUP_FOLDER_PREFIX = "trace.setup-library.folder.";

export function autoIndexSetupsEnabled() {
	return localStorage.getItem(AUTO_INDEX_SETUPS_KEY) === "true";
}

export function setAutoIndexSetupsEnabled(enabled: boolean) {
	localStorage.setItem(AUTO_INDEX_SETUPS_KEY, String(enabled));
}

export function savedSetupFolder(simulatorId: string) {
	return localStorage.getItem(`${SETUP_FOLDER_PREFIX}${simulatorId}`)?.trim() || null;
}

export function saveSetupFolder(simulatorId: string, path: string) {
	const value = path.trim();
	if (value) localStorage.setItem(`${SETUP_FOLDER_PREFIX}${simulatorId}`, value);
	else localStorage.removeItem(`${SETUP_FOLDER_PREFIX}${simulatorId}`);
}

export async function indexDetectedSetupLibraries() {
	const importers = await telemetryDataSource.getSetupImporters();
	let indexed = 0;
	let errors = 0;
	for (const importer of importers) {
		try {
			const saved = savedSetupFolder(importer.simulatorId);
			const folder = saved ? { path: saved, found: true } : await telemetryDataSource.detectSetupFolder(importer.simulatorId);
			if (!folder.found || !folder.path) continue;
			const result = await telemetryDataSource.indexExistingSetups(importer.simulatorId, folder.path);
			indexed += result.indexed;
			errors += result.errors.length;
		} catch {
			errors += 1;
		}
	}
	return { indexed, errors };
}
