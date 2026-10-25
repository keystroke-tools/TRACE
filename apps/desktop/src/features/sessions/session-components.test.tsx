import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RecordedLapMetrics } from "../../data-source";
import { ElectronicsSummary, friendlySetupLabel } from "./session-components";

describe("session telemetry summaries", () => {
	it("shows LMU electronics changes and intervention activity", () => {
		const metrics: RecordedLapMetrics = {
			lapIndex: 3,
			tyreWearStart: [null, null, null, null],
			tyreWearEnd: [null, null, null, null],
			tyreWearMinimum: [null, null, null, null],
			electronics: {
				settings: [
					{
						key: "tc",
						label: "TC",
						start: 4,
						end: 3,
						minimum: 3,
						maximum: 4,
						availableMaximum: 10,
						changes: 1,
					},
				],
				tcActivePercent: 12.4,
				absActivePercent: 0,
				rearFlapActivePercent: 32.1,
			},
		};

		render(<ElectronicsSummary metrics={metrics} />);

		expect(screen.getByText("TC 4→3")).toBeInTheDocument();
		expect(screen.getByText("TC ACTIVE 12%")).toBeInTheDocument();
		expect(screen.getByText("DRS OPEN 32%")).toBeInTheDocument();
		expect(screen.queryByText(/ABS ACTIVE/)).not.toBeInTheDocument();
	});

	it("turns LMU setting keys into readable labels", () => {
		expect(friendlySetupLabel("TractionControlMapSetting")).toBe("Traction Control Map");
	});
});
