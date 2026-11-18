export const CHANGELOG_STORAGE_KEY = "trace.whats-new.seen-version";

export interface ChangelogHighlight {
	title: string;
	description: string;
}

export const CURRENT_CHANGELOG: { intro: string; highlights: ChangelogHighlight[] } = {
	intro: "A quick look at the changes that matter before you get back on track.",
	highlights: [
		{
			title: "More reliable LMU lap times",
			description: "Lap boundaries now stay on LMU's simulator clock and tolerate its completed-lap fields arriving on different updates.",
		},
		{
			title: "Simpler LMU setup imports",
			description: "Drop setup ZIP packs directly into TRACE and keep only the latest included revision when that is all you need.",
		},
		{
			title: "Consistent timing everywhere",
			description: "Desktop and shared-session views now use the same millisecond rounding for displayed lap and sector times.",
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
