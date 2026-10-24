import { cleanup, render, screen, waitFor } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ComparePicker } from "./compare-picker";

const currentSummary = {
	session_id: "current",
	title: "Race",
	driver_name: "Alex",
	simulator: "le-mans-ultimate",
	track: "Interlagos",
	layout: null,
	car: "LMP3",
	session_type: "race",
	started_at: "2026-10-24T12:00:00Z",
	tags: ["league"],
};

const candidateSummary = {
	...currentSummary,
	session_id: "candidate",
	driver_name: "Sam",
	started_at: "2026-10-23T12:00:00Z",
	tags: ["reference"],
};

const candidateLap = {
	id: "candidate-lap",
	index: 3,
	duration_ns: 90_000_000_000,
	validity: "valid",
	max_tyres_out: 0,
	sectors: [{ index: 1, duration_ns: 30_000_000_000 }],
};

function openPicker() {
	window.traceCompareSnapshot = {
		sessions: [
			candidateSummary,
			{ ...currentSummary, session_id: "other", driver_name: "Taylor", simulator: "assetto-corsa", track: "Zandvoort", car: "MX-5" },
		],
		currentReview: { summary: currentSummary, laps: [], source_track_id: "interlagos", source_car_id: "lmp3" },
		selectedLap: { ...candidateLap, id: "current-lap", index: 2, duration_ns: 91_000_000_000 },
	};
}

function jsonResponse(body: unknown) {
	return {
		ok: true,
		status: 200,
		headers: { get: () => "application/json" },
		json: async () => body,
		text: async () => JSON.stringify(body),
	};
}

afterEach(() => {
	cleanup();
	delete window.traceCompareSnapshot;
	vi.unstubAllGlobals();
});

describe("comparison picker", () => {
	it("supports compound filtering across session identity fields", async () => {
		openPicker();
		render(<ComparePicker />);
		await userEvent.type(screen.getByPlaceholderText("Driver, car, track…"), "sam inter");
		expect(screen.getByText("Sam")).toBeTruthy();
		expect(screen.queryByText("Taylor")).toBeNull();
	});

	it("loads a compatible lap and bridges its telemetry into comparison", async () => {
		openPicker();
		const telemetry = { points: [{ elapsed_seconds: 0 }] };
		const fetch = vi
			.fn()
			.mockResolvedValueOnce(jsonResponse({ summary: candidateSummary, laps: [candidateLap], source_track_id: "interlagos", source_car_id: "lmp3" }))
			.mockResolvedValueOnce(jsonResponse(telemetry));
		vi.stubGlobal("fetch", fetch);
		let selection: { candidateLap: { id: string }; candidateTelemetry: unknown } | undefined;
		window.addEventListener(
			"trace:comparison-create",
			(event) => {
				selection = (event as CustomEvent<typeof selection>).detail;
			},
			{ once: true },
		);

		render(<ComparePicker />);
		await userEvent.click(screen.getByText("Sam"));
		await waitFor(() => expect(screen.getByText("Lap 3")).toBeTruthy());
		await userEvent.click(screen.getByText("Lap 3"));
		await waitFor(() => expect(selection?.candidateLap.id).toBe("candidate-lap"));
		expect(selection?.candidateTelemetry).toEqual(telemetry);
	});
});
