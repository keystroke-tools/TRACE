import type { SharedSession } from "../../data-source";

const STORAGE_KEY = "trace.shared-session-links.v1";
const MAX_LINKS = 20;

export interface StoredSharedSession extends SharedSession {
	sessionId: string;
	lapIndices: number[];
	createdAtUnixMs: number;
}

export function rememberSharedSession(sessionId: string, lapIndices: number[], share: SharedSession): StoredSharedSession {
	const entry: StoredSharedSession = {
		...share,
		sessionId,
		lapIndices: [...lapIndices].sort((left, right) => left - right),
		createdAtUnixMs: Date.now(),
	};
	const retained = readHistory().filter((item) => item.shareId !== share.shareId);
	writeHistory([entry, ...retained].slice(0, MAX_LINKS));
	return entry;
}

export function sharedSessionsFor(sessionId: string): StoredSharedSession[] {
	return readHistory().filter((item) => item.sessionId === sessionId);
}

function readHistory(): StoredSharedSession[] {
	try {
		const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]") as unknown;
		if (!Array.isArray(parsed)) return [];
		const now = Date.now();
		const retained = parsed.filter(isStoredShare).filter((item) => item.expiresAtUnixMs > now);
		if (retained.length !== parsed.length) writeHistory(retained);
		return retained;
	} catch {
		return [];
	}
}

function writeHistory(items: StoredSharedSession[]) {
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
	} catch {
		// Link history is a convenience; sharing still succeeds if storage is unavailable.
	}
}

function isStoredShare(value: unknown): value is StoredSharedSession {
	if (!value || typeof value !== "object") return false;
	const candidate = value as Partial<StoredSharedSession>;
	return (
		typeof candidate.shareId === "string" &&
		typeof candidate.shareUrl === "string" &&
		typeof candidate.sessionId === "string" &&
		typeof candidate.expiresAtUnixMs === "number" &&
		Array.isArray(candidate.lapIndices) &&
		candidate.lapIndices.every((index) => typeof index === "number")
	);
}
