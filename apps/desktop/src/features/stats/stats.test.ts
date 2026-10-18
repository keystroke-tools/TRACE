import { describe, expect, it } from "vitest";
import type { RecordedSessionSummary } from "../../data-source";
import { calculateDrivingStats, filterStatsSessions } from "./stats";

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

function sectors(count: number) {
	return Array.from({ length: count }, (_, index) => ({ index: index + 1, time: "0:30.000", durationNs: 30_000_000_000 }));
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

	it("does not promote an incomplete lap fragment over laps with the normal sector set", () => {
		const stats = calculateDrivingStats([
			session({
				id: "race",
				startedAt: "2026-10-17T12:00:00Z",
				laps: [{ ...lap(1, 133), sectors: sectors(3) }, { ...lap(2, 132), sectors: sectors(3) }, lap(3, 10.8)],
			}),
		]);

		expect(stats.combinations[0].bestLapNs).toBe(132_000_000_000);
		expect(stats.combinations[0].lapCount).toBe(3);
	});
});

describe("filterStatsSessions", () => {
	it("combines simulator, session type, time range, and ranking focus", () => {
		const sessions = [
			session({ id: "match", startedAt: "2026-10-10T12:00:00Z", sessionType: "RACE" }),
			session({ id: "practice", startedAt: "2026-10-10T12:00:00Z", sessionType: "PRACTICE" }),
			session({ id: "old", startedAt: "2026-01-01T12:00:00Z", sessionType: "RACE" }),
			session({ id: "other-track", startedAt: "2026-10-10T12:00:00Z", sessionType: "RACE", track: "Le Mans" }),
		];
		const filtered = filterStatsSessions(
			sessions,
			{
				simulatorId: "le-mans-ultimate",
				sessionType: "RACE",
				timeRange: "30",
				focusKind: "track",
				focusKey: "le-mans-ultimate\u0000Spa-Francorchamps",
			},
			new Date("2026-10-18T12:00:00Z").getTime(),
		);

		expect(filtered.map((item) => item.id)).toEqual(["match"]);
	});
});
