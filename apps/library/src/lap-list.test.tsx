import { cleanup, render, screen } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { LapList } from "./lap-list";

const laps = [
	{
		id: "lap-one",
		index: 1,
		duration_ns: 92_000_000_000,
		validity: "invalid",
		sectors: [
			{ index: 1, duration_ns: 30_000_000_000 },
			{ index: 2, duration_ns: 25_000_000_000 },
			{ index: 3, duration_ns: 20_000_000_000 },
			{ index: 4, duration_ns: 17_000_000_000 },
		],
	},
	{
		id: "lap-two",
		index: 2,
		duration_ns: 91_123_000_000,
		validity: "valid",
		sectors: [],
	},
];

afterEach(() => {
	cleanup();
	delete window.traceLapListSnapshot;
});

describe("lap list", () => {
	it("marks selected, fastest, and invalid laps independently", () => {
		window.traceLapListSnapshot = { laps, selectedLapId: "lap-one" };
		render(<LapList />);
		expect(screen.getByText("2 laps")).toBeTruthy();
		expect(screen.getByText("LAP 1").closest("button")?.className).toContain("active");
		expect(screen.getByText("LAP 1").closest("button")?.className).toContain("invalid");
		expect(screen.getByText("LAP 2").closest("button")?.className).toContain("best");
		expect(screen.getByText("S1 0:30.000")).toBeTruthy();
		expect(screen.getByText("+1 more")).toBeTruthy();
		expect(screen.queryByText("S4 0:17.000")).toBeNull();
	});

	it("bridges lap selection to the telemetry loader", async () => {
		window.traceLapListSnapshot = { laps, selectedLapId: null };
		let selected: string | undefined;
		window.addEventListener(
			"trace:lap-open",
			(event) => {
				selected = (event as CustomEvent<{ lapId: string }>).detail.lapId;
			},
			{ once: true },
		);
		render(<LapList />);
		await userEvent.click(screen.getByText("LAP 2"));
		expect(selected).toBe("lap-two");
	});
});
