import { describe, expect, it } from "vitest";
import type { RecordedLapSummary, RecordedSessionSummary } from "../../data-source";
import { fasterReferenceSuggestions, validComparisonLaps } from "./comparison-selection";

function lap(index: number, seconds: number, validity: RecordedLapSummary["validity"] = "valid"): RecordedLapSummary {
	return {
		index,
		time: `${Math.floor(seconds / 60)}:${(seconds % 60).toFixed(3).padStart(6, "0")}`,
		durationNs: seconds * 1_000_000_000,
		validity,
		sectors: [],
	};
}

function session(id: string, laps: RecordedLapSummary[], source = "drive", track = "zandvoort"): RecordedSessionSummary {
	return {
		id,
		simulatorId: "assetto-corsa",
		simulatorName: "Assetto Corsa",
		ownership: "mine",
		tags: [],
		track,
		car: "ks_mazda_mx5_cup",
		sessionType: "hotlap",
		startedAt: "2026-10-15T12:00:00Z",
		source,
		exportable: true,
		deletable: true,
		laps,
	};
}

describe("comparison lap selection", () => {
	it("only exposes valid timed laps, ordered fastest first", () => {
		const value = session("analysed", [lap(1, 105), lap(2, 103, "invalid"), lap(3, 102), { ...lap(4, 99), time: "—" }]);
		expect(validComparisonLaps(value).map((candidate) => candidate.index)).toEqual([3, 1]);
	});

	it("suggests compatible faster laps and prioritises imported references", () => {
		const analysed = session("analysed", [lap(1, 100)]);
		const native = session("native", [lap(1, 97)]);
		const imported = session("imported", [lap(1, 99)], "motec import");
		const wrongTrack = session("other-track", [lap(1, 90)], "motec import", "silverstone");

		const suggestions = fasterReferenceSuggestions([analysed, native, imported, wrongTrack], analysed, analysed.laps[0], analysed.id, 1);

		expect(suggestions.map((candidate) => candidate.session.id)).toEqual(["imported", "native"]);
		expect(suggestions.map((candidate) => candidate.gainSeconds)).toEqual([1, 3]);
	});
});
