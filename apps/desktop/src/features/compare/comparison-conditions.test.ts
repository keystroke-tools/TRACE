import { describe, expect, it } from "vitest";
import type { RecordedSessionSummary } from "../../data-source";
import { comparisonConditionWarning } from "./comparison-conditions";

const session = (conditions: Partial<RecordedSessionSummary>): RecordedSessionSummary => ({
	id: "session",
	simulatorId: "assetto-corsa",
	simulatorName: "Assetto Corsa",
	ownership: "mine",
	tags: [],
	track: "Spa",
	car: "MX-5 Cup",
	sessionType: "practice",
	startedAt: "2026-11-11T12:00:00Z",
	source: "recorded",
	exportable: true,
	deletable: true,
	laps: [],
	...conditions,
});

describe("comparisonConditionWarning", () => {
	it("warns clearly when wet and dry weather are compared", () => {
		const warning = comparisonConditionWarning(session({ weatherName: "clear" }), session({ weatherName: "heavy_rain" }));
		expect(warning).toEqual({
			title: "Wet and dry laps compared",
			details: ["Weather Clear → Heavy Rain"],
		});
	});

	it("includes only material temperature and grip differences", () => {
		const warning = comparisonConditionWarning(
			session({ ambientTemperatureC: "20", roadTemperatureC: "31", trackGripPercent: 96 }),
			session({ ambientTemperatureC: "24", roadTemperatureC: "39", trackGripPercent: 100 }),
		);
		expect(warning).toEqual({
			title: "Session conditions differ",
			details: ["Track 31°C → 39°C", "Air 20°C → 24°C"],
		});
	});

	it("does not warn for matching or incomplete conditions", () => {
		expect(comparisonConditionWarning(session({ roadTemperatureC: "30" }), session({ roadTemperatureC: "32" }))).toBeNull();
		expect(comparisonConditionWarning(session({ weatherName: "rain" }), session({}))).toBeNull();
	});
});
