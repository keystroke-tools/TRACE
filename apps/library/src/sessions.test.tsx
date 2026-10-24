import { cleanup, render, screen } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { Sessions } from "./sessions";

const sessions = [
	{
		session_id: "first",
		title: null,
		driver_name: "Alex",
		simulator: "le-mans-ultimate",
		track: "Interlagos",
		layout: null,
		car: "Mazda MX-5 Cup",
		session_type: "race",
		started_at: "2026-10-24T12:00:00Z",
		tags: ["league"],
		lap_count: 12,
		size_bytes: 2048,
		share_id: null,
	},
	{
		session_id: "second",
		title: "Wet practice",
		driver_name: "Sam",
		simulator: "assetto-corsa",
		track: "Zandvoort",
		layout: null,
		car: "Formula car",
		session_type: "practice",
		started_at: "2026-10-23T12:00:00Z",
		tags: ["wet"],
		lap_count: 4,
		size_bytes: 4096,
		share_id: null,
	},
];

afterEach(() => {
	cleanup();
	delete window.traceLibrarySnapshot;
});

describe("session library", () => {
	it("filters compound session identity text", async () => {
		window.traceLibrarySnapshot = { sessions, maximumUploadBytes: 128 * 1024 * 1024 };
		render(<Sessions />);
		await userEvent.type(screen.getByPlaceholderText("Driver, car, track, tag…"), "maz inter");
		expect(screen.getByText("Alex")).toBeTruthy();
		expect(screen.queryByText("Wet practice")).toBeNull();
		expect(screen.getByText("1/2")).toBeTruthy();
	});

	it("bridges session selection to the review workspace", async () => {
		window.traceLibrarySnapshot = { sessions, maximumUploadBytes: 128 * 1024 * 1024 };
		let opened: string | undefined;
		window.addEventListener(
			"trace:open-session",
			(event) => {
				opened = (event as CustomEvent<{ sessionId: string }>).detail.sessionId;
			},
			{ once: true },
		);
		render(<Sessions />);
		await userEvent.click(screen.getByText("Alex"));
		expect(opened).toBe("first");
	});
});
