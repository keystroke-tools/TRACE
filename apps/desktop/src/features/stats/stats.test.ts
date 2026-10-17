import { describe, expect, it } from "vitest";
import type { RecordedSessionSummary } from "../../data-source";
import { calculateDrivingStats } from "./stats";

function session(overrides: Partial<RecordedSessionSummary> & Pick<RecordedSessionSummary, "id" | "startedAt">): RecordedSessionSummary {
	return {
		simulatorId: "le-mans-ultimate",
		simulatorName: "Le Mans Ultimate",
		title: null,
		driver: "Driver",
		ownership: "mine",
		tags: [],
		track: "Spa-Francorchamps",
		car: "Ligier JS P325",
		sessionType: "RACE",
		source: "NATIVE CAPTURE",
		exportable: true,
		deletable: true,
		laps: [],
		...overrides,
	};
}

function lap(index: number, seconds: number, validity: "valid" | "invalid" | "unknown" = "valid") {
	return {
		index,
		time: `0:${seconds}`,
		durationNs: seconds * 1_000_000_000,
		validity,
		sectors: [],
	};
}

describe("calculateDrivingStats", () => {
	it("ranks recorded driving and excludes imported and replay sessions", () => {
		const stats = calculateDrivingStats([
			session({ id: "race-1", startedAt: "2026-10-17T20:00:00Z", laps: [lap(1, 132), lap(2, 131)] }),
			session({ id: "race-2", startedAt: "2026-10-17T21:00:00Z", track: "Le Mans", laps: [lap(1, 220)] }),
			session({ id: "import", startedAt: "2026-10-17T22:00:00Z", source: "IMPORTED", laps: [lap(1, 100)] }),
			session({ id: "replay", startedAt: "2026-10-17T23:00:00Z", source: "SIMULATOR REPLAY", laps: [lap(1, 90)] }),
		]);

		expect(stats.sessionCount).toBe(2);
		expect(stats.lapCount).toBe(3);
		expect(stats.simulators[0]).toMatchObject({ label: "Le Mans Ultimate", sessionCount: 2, lapCount: 3 });
		expect(stats.tracks.map((item) => item.label)).toEqual(["Spa-Francorchamps", "Le Mans"]);
	});

	it("keeps per-session best laps in chronological order and reports the overall best", () => {
		const stats = calculateDrivingStats([
			session({ id: "later", startedAt: "2026-10-18T12:00:00Z", laps: [lap(1, 130), lap(2, 129), lap(3, 80, "invalid")] }),
			session({ id: "first", startedAt: "2026-10-17T12:00:00Z", laps: [lap(1, 132)] }),
		]);
		const combination = stats.combinations[0];

		expect(combination.bestLapNs).toBe(129_000_000_000);
		expect(combination.bestSessionId).toBe("later");
		expect(combination.lapCount).toBe(4);
		expect(combination.points.map((point) => point.sessionId)).toEqual(["first", "later"]);
	});
});
