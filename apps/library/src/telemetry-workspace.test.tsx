import { cleanup, render, screen } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ComparisonSummary, TelemetryWorkspace } from "./telemetry-workspace";

const lap = { id: "lap-one", index: 4, duration_ns: 91_250_000_000 };
const points = [
	{
		elapsed_seconds: 0,
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
	it("renders selected-lap statistics and all base visualisations", () => {
		window.traceTelemetrySnapshot = { lap, telemetry: { points }, comparison: null, message: "" };
		render(<TelemetryWorkspace />);
		expect(screen.getByText("LAP 4")).toBeTruthy();
		expect(screen.getByText("1:31.250")).toBeTruthy();
		expect(screen.getByText("210 km/h")).toBeTruthy();
		expect(screen.getByText("7500 rpm")).toBeTruthy();
		expect(screen.getByText("DRIVING LINE")).toBeTruthy();
		expect(screen.getByRole("img", { name: "SPEED telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "THROTTLE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "BRAKE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "STEERING ANGLE telemetry by lap distance" })).toBeTruthy();
		expect(screen.getByRole("img", { name: "GEAR telemetry by lap distance" })).toBeTruthy();
		expect(screen.queryByText("TIME DELTA · ANALYSED VS REFERENCE")).toBeNull();
	});

	it("renders comparison identity and bridges closing it", async () => {
		const reference = { detail: { summary: { title: null, driver_name: "Sam" } }, lap: { ...lap, duration_ns: 90_000_000_000 }, telemetry: { points } };
		const analysed = { detail: { summary: { title: null, driver_name: "Alex" } }, lap, telemetry: { points } };
		window.traceTelemetrySnapshot = {
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
});
