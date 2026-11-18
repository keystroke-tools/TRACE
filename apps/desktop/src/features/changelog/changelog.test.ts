import { describe, expect, it } from "vitest";
import { CHANGELOG_STORAGE_KEY, markChangelogSeen, normalizeVersion, shouldShowChangelog } from "./changelog";

describe("in-app changelog", () => {
	it("normalizes release version labels", () => {
		expect(normalizeVersion(" v0.8.20 ")).toBe("0.8.20");
	});

	it("opens once for every installed version", () => {
		const values = new Map<string, string>();
		const storage = {
			getItem: (key: string) => values.get(key) ?? null,
			setItem: (key: string, value: string) => values.set(key, value),
		};

		expect(shouldShowChangelog("v0.8.20", storage)).toBe(true);
		markChangelogSeen("v0.8.20", storage);
		expect(values.get(CHANGELOG_STORAGE_KEY)).toBe("0.8.20");
		expect(shouldShowChangelog("0.8.20", storage)).toBe(false);
		expect(shouldShowChangelog("v0.8.21", storage)).toBe(true);
	});
});
