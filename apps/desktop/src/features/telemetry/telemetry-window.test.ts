import { describe, expect, it } from "vitest";
import type { LapComparisonSample } from "@trace/ui";
import { nextTelemetryWindow, panTelemetryWindow } from "./telemetry-window";

const samples: LapComparisonSample[] = [0, 25, 50, 75, 100].map((distanceM) => ({ distanceM }));

describe("telemetry window navigation", () => {
	it("keeps the zoom anchor at the same relative position", () => {
		expect(nextTelemetryWindow(samples, null, 80, "in", 1)).toEqual({ startM: 24, endM: 94 });
	});

	it("pans a zoomed window without leaving the recorded range", () => {
		expect(panTelemetryWindow(samples, { startM: 20, endM: 60 }, 30)).toEqual({ startM: 50, endM: 90 });
		expect(panTelemetryWindow(samples, { startM: 20, endM: 60 }, 100)).toEqual({ startM: 60, endM: 100 });
		expect(panTelemetryWindow(samples, { startM: 20, endM: 60 }, -100)).toEqual({ startM: 0, endM: 40 });
		expect(panTelemetryWindow(samples, null, 20)).toBeNull();
	});
});
