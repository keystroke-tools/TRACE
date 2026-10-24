import { cleanup, render, screen } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nearestTelemetryPlaybackIndex, telemetryPlaybackTimeline } from "@trace/ui";
import { ComparisonSummary, TelemetryWorkspace } from "./telemetry-workspace";

const lap = { id: "lap-one", index: 4, duration_ns: 91_250_000_000 };
const points = [
	{
		elapsed_seconds: 0,
		sector_index: 1,
		lap_position: 0,
		speed_kph: 100,
		throttle: 0.5,
		brake: 0,
		engine_rpm: 5000,
		steering_angle_rad: 0,
		gear: 3,
		position_x_m: 0,
		position_z_m: 0,
	},
	{
		elapsed_seconds: 1,
		sector_index: 2,
		lap_position: 1,
		speed_kph: 210.4,
		throttle: 1,
		brake: 0.2,
		engine_rpm: 7499.6,
		steering_angle_rad: 0.2,
		gear: 4,
		position_x_m: 10,
		position_z_m: 5,
	},
];

beforeEach(() => {
	vi.stubGlobal(
		"ResizeObserver",
		class {
			observe() {}
			disconnect() {}
		},
	);
	vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
		scale: vi.fn(),
		beginPath: vi.fn(),
		moveTo: vi.fn(),
		lineTo: vi.fn(),
		stroke: vi.fn(),
		fillText: vi.fn(),
	} as unknown as CanvasRenderingContext2D);
	vi.spyOn(HTMLCanvasElement.prototype, "getBoundingClientRect").mockReturnValue({ width: 400, height: 230 } as DOMRect);
});

afterEach(() => {
	cleanup();
	delete window.traceTelemetrySnapshot;
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
});

describe("telemetry workspace", () => {
	it("uses recorded elapsed time and distance fallback for playback", () => {
		const timeline = telemetryPlaybackTimeline([
			{ distanceM: 0, referenceElapsedSeconds: 0, referenceSpeedKmh: 72 },
			{ distanceM: 10, referenceElapsedSeconds: 0.5, referenceSpeedKmh: 72 },
			{ distanceM: 15, referenceSpeedKmh: 72 },
		]);
		expect(timeline).toEqual([0, 0.5, 0.75]);
		expect(nearestTelemetryPlaybackIndex(timeline, 0.6)).toBe(1);
	});

	it("renders selected-lap statistics and keeps shared chart zoom synchronized", async () => {
		window.traceTelemetrySnapshot = {
			detail: { summary: { title: null, driver_name: "Alex", track: "Zandvoort", car: "Mazda MX-5 Cup", session_type: "Hotlap" } },
			lap,
			telemetry: { points },
			comparison: null,
			message: "",
		};
		render(<TelemetryWorkspace />);
		expect(screen.getByText("LAP 4")).toBeTruthy();
		expect(screen.getByText("1:31.250")).toBeTruthy();
		expect(screen.getByText("210 km/h")).toBeTruthy();
		expect(screen.getByText("7500 rpm")).toBeTruthy();
		expect(screen.getByText("Zandvoort · Mazda MX-5 Cup")).toBeTruthy();
		expect(screen.getByRole("button", { name: "Zoom map in" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "Zoom map out" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "Reset map pan and spatial zoom" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "Play telemetry playback" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "SECTOR 1" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "SECTOR 2" })).toBeTruthy();
		expect(screen.getByRole("slider", { name: "Seek through lap distance" })).toBeTruthy();
		expect((screen.getByRole("combobox", { name: "Telemetry playback speed" }) as HTMLSelectElement).value).toBe("1");
		expect(screen.getByRole("img", { name: "SPEED telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "THROTTLE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "BRAKE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "STEERING ANGLE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "GEAR telemetry by lap distance" })).toBeTruthy();
		expect(screen.queryByText("TIME DELTA · ANALYSED VS REFERENCE")).toBeNull();
		await userEvent.click(screen.getAllByRole("button", { name: /Zoom all telemetry in around 50 %/ })[0]);
		expect(screen.getByText(/RESET ZOOM/)).toBeTruthy();
	});

	it("renders comparison identity and bridges closing it", async () => {
		const reference = { detail: { summary: { title: null, driver_name: "Sam" } }, lap: { ...lap, duration_ns: 90_000_000_000 }, telemetry: { points } };
		const analysed = { detail: { summary: { title: null, driver_name: "Alex" } }, lap, telemetry: { points } };
		window.traceTelemetrySnapshot = {
			detail: null,
			lap,
			telemetry: { points },
			comparison: { reference, analysed, samples: [], alignment: "distance" },
			message: "",
		};
		let closed = false;
		window.addEventListener(
			"trace:comparison-clear",
			() => {
				closed = true;
			},
			{ once: true },
		);
		render(<ComparisonSummary />);
		expect(screen.getByText("Sam")).toBeTruthy();
		expect(screen.getByText("Alex")).toBeTruthy();
		expect(screen.getByText("+1.250")).toBeTruthy();
		expect(screen.getByText("DISTANCE ALIGNED")).toBeTruthy();
		await userEvent.click(screen.getByText("Close comparison"));
		expect(closed).toBe(true);
	});

	it("renders the shared comparison HUD, analysis, and complete channel set", async () => {
		const reference = {
			detail: { summary: { title: null, driver_name: "Sam", track: "Spa", car: "LMP3" } },
			lap: { ...lap, id: "reference", duration_ns: 90_000_000_000, sectors: [{ index: 1, duration_ns: 90_000_000_000 }] },
			telemetry: { points },
		};
		const analysed = {
			detail: { summary: { title: null, driver_name: "Alex", track: "Spa", car: "LMP3" } },
			lap: { ...lap, id: "analysed", sectors: [{ index: 1, duration_ns: 91_250_000_000 }] },
			telemetry: { points },
		};
		window.traceTelemetrySnapshot = {
			detail: analysed.detail,
			lap: analysed.lap,
			telemetry: { points },
			comparison: {
				reference,
				analysed,
				alignment: "distance",
				samples: [
					{
						progress: 0,
						reference_sector_index: 1,
						analysed_sector_index: 1,
						reference_elapsed_seconds: 0,
						analysed_elapsed_seconds: 0,
						delta_seconds: 0,
						reference_speed_kph: 120,
						analysed_speed_kph: 116,
						reference_throttle: 1,
						analysed_throttle: 0.8,
						reference_brake: 0,
						analysed_brake: 0.1,
						reference_engine_rpm: 6_000,
						analysed_engine_rpm: 5_800,
						reference_steering_angle_rad: 0,
						analysed_steering_angle_rad: 0.1,
						reference_gear: 4,
						analysed_gear: 4,
						reference_position_x_m: 0,
						reference_position_z_m: 0,
						analysed_position_x_m: 0,
						analysed_position_z_m: 0,
					},
					{
						progress: 1,
						reference_sector_index: 1,
						analysed_sector_index: 1,
						reference_elapsed_seconds: 90,
						analysed_elapsed_seconds: 91.25,
						delta_seconds: 1.25,
						reference_speed_kph: 200,
						analysed_speed_kph: 190,
						reference_throttle: 1,
						analysed_throttle: 0.8,
						reference_brake: 0,
						analysed_brake: 0.1,
						reference_engine_rpm: 7_000,
						analysed_engine_rpm: 6_700,
						reference_steering_angle_rad: 0.2,
						analysed_steering_angle_rad: 0.3,
						reference_gear: 5,
						analysed_gear: 5,
						reference_position_x_m: 10,
						reference_position_z_m: 5,
						analysed_position_x_m: 9,
						analysed_position_z_m: 5,
					},
				],
			},
			message: "",
		};
		render(<TelemetryWorkspace />);
		expect(screen.getByText("FASTER REFERENCE")).toBeTruthy();
		expect(screen.getByText("LAP TO IMPROVE")).toBeTruthy();
		expect(screen.getByRole("button", { name: "Swap Reference and Analysed Lap" })).toBeTruthy();
		expect(screen.getByText("INSIGHTS 1")).toBeTruthy();
		expect(screen.getByRole("img", { name: "ENGINE SPEED telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "STEERING ANGLE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "GEAR telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByText(/may be incomplete or incorrect/)).toBeTruthy();
	});
});
