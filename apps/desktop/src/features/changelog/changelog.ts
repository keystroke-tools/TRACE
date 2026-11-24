export const CHANGELOG_STORAGE_KEY = "trace.whats-new.seen-version";

export interface ChangelogHighlight {
	title: string;
	description: string;
}

export const CURRENT_CHANGELOG: { intro: string; highlights: ChangelogHighlight[] } = {
	intro: "A quick look at the changes that matter before you get back on track.",
	highlights: [
		{
			title: "Release notes stay dismissed",
			description: "What's changed now appears once for each installed version, even when TRACE closes before you dismiss it.",
		},
		{
			title: "More reliable server deployments",
			description: "Self-hosted Docker builds now avoid concurrent cache corruption and use less memory during the final server link.",
		},
	],
};

export function normalizeVersion(version: string) {
	return version.trim().replace(/^v/i, "");
}

export function shouldShowChangelog(version: string, storage: Pick<Storage, "getItem"> = window.localStorage) {
	const current = normalizeVersion(version);
	if (!current) return false;
	try {
		return normalizeVersion(storage.getItem(CHANGELOG_STORAGE_KEY) ?? "") !== current;
	} catch {
		return true;
	}
}

export function markChangelogSeen(version: string, storage: Pick<Storage, "setItem"> = window.localStorage) {
	const current = normalizeVersion(version);
	if (!current) return;
	try {
		storage.setItem(CHANGELOG_STORAGE_KEY, current);
	} catch {
		// A locked-down webview should not prevent TRACE from opening.
	}
}

export function consumeChangelog(version: string, storage: Pick<Storage, "getItem" | "setItem"> = window.localStorage) {
	if (!shouldShowChangelog(version, storage)) return false;
	markChangelogSeen(version, storage);
	return true;
}
