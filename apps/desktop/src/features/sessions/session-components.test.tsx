import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RecordedLapMetrics } from "../../data-source";
import { DamageSummary, ElectronicsSummary, TyreState, enduranceEstimates, formatVirtualEnergyUsed, friendlySetupLabel } from "./session-components";

describe("session telemetry summaries", () => {
	it("only surfaces damage that was observed", () => {
		const metrics: RecordedLapMetrics = {
			lapIndex: 4,
			tyreWearStart: [null, null, null, null],
			tyreWearEnd: [null, null, null, null],
			tyreWearMinimum: [null, null, null, null],
			dentSeverityMaximum: [0, 1, 2, 0, 0, 0, 0, 0],
			overheatingObserved: true,
			impactCount: 2,
			maximumImpactMagnitude: 14.5,
		};
		render(<DamageSummary metrics={metrics} />);
		expect(screen.getByText("BODY 2/8")).toBeInTheDocument();
		expect(screen.getByText("IMPACT ×2")).toBeInTheDocument();
		expect(screen.getByText("OVERHEAT")).toBeInTheDocument();
	});

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

	it("shows LMU tyre life and converts fractional virtual energy to percentages", () => {
		const metrics: RecordedLapMetrics = {
			lapIndex: 4,
			fuelStartLitres: 40,
			fuelEndLitres: 37.5,
			fuelUsedLitres: 2.5,
			tyreWearStart: [96, 95, 97, 96],
			tyreWearEnd: [94, 93, 96, 95],
			tyreWearMinimum: [94, 93, 96, 95],
			virtualEnergyStart: 0.82,
			virtualEnergyEnd: 0.74,
			virtualEnergyUsed: 0.08,
		};

		render(<TyreState state="ready" metrics={metrics} />);

		expect(screen.getByText("94%")).toHaveClass("items-center", "justify-center", "tabular-nums");
		expect(screen.getByText("93%")).toBeInTheDocument();
		expect(formatVirtualEnergyUsed(metrics)).toBe("8.0% VE");
	});

	it("derives conservative endurance estimates from clean lap consumption", () => {
		const lap = (index: number, fuelUsedLitres: number, fuelEndLitres: number, tyreEnd: number): RecordedLapMetrics => ({
			lapIndex: index,
			fuelUsedLitres,
			fuelEndLitres,
			tyreWearStart: [tyreEnd + 1, tyreEnd + 1, tyreEnd + 1, tyreEnd + 1],
			tyreWearEnd: [tyreEnd, tyreEnd, tyreEnd, tyreEnd],
			tyreWearMinimum: [tyreEnd, tyreEnd, tyreEnd, tyreEnd],
			virtualEnergyStart: 0.8,
			virtualEnergyEnd: 0.75,
			virtualEnergyUsed: 0.05,
		});
		const estimates = enduranceEstimates([lap(1, 2.4, 30, 98), lap(2, 2.6, 27.4, 97)]);

		expect(estimates.fuelPerLap).toBe(2.5);
		expect(estimates.fuelLapsRemaining).toBeCloseTo(10.96);
		expect(estimates.virtualEnergyPerLap).toBeCloseTo(5);
		expect(estimates.tyreLapsRemaining).toBe(97);
	});
});
