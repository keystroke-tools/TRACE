import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SharedSession } from "../../data-source";
import { rememberSharedSession, sharedSessionsFor } from "./share-history";

function share(id: string, expiresAtUnixMs: number): SharedSession {
	return {
		shareId: id,
		shareUrl: `https://live.simtrace.run/s/${id}`,
		downloadUrl: `https://live.simtrace.run/api/v1/shared-sessions/${id}/download`,
		expiresAtUnixMs,
		sizeBytes: 1_024,
		simulator: "assetto-corsa",
		car: "Mazda MX-5 Cup",
		track: "Zandvoort",
		sessionType: "Hotlap",
		lapCount: 1,
	};
}

describe("temporary share history", () => {
	beforeEach(() => {
		const values = new Map<string, string>();
		vi.stubGlobal("localStorage", {
			getItem: (key: string) => values.get(key) ?? null,
			setItem: (key: string, value: string) => values.set(key, value),
			removeItem: (key: string) => values.delete(key),
			clear: () => values.clear(),
		});
		vi.useRealTimers();
	});

	it("keeps selected laps with an active link", () => {
		const expires = Date.now() + 60_000;
		rememberSharedSession("session-one", [4, 2], share("share-one", expires));

		expect(sharedSessionsFor("session-one")).toMatchObject([
			{
				shareId: "share-one",
				lapIndices: [2, 4],
				expiresAtUnixMs: expires,
			},
		]);
		expect(sharedSessionsFor("another-session")).toEqual([]);
	});

	it("drops expired links", () => {
		rememberSharedSession("session-one", [1], share("expired", Date.now() - 1));
		expect(sharedSessionsFor("session-one")).toEqual([]);
	});
});
