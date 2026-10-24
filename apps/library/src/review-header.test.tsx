import { cleanup, render, screen, waitFor } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReviewHeader } from "./review-header";

const summary = {
	session_id: "session-one",
	title: "Race two",
	driver_name: "Alex",
	simulator: "Le Mans Ultimate",
	track: "Interlagos",
	car: "LMP3",
	session_type: "race",
	started_at: "2026-10-24T12:00:00Z",
	tags: ["league"],
	share_id: null,
};

afterEach(() => {
	cleanup();
	delete window.traceReviewSnapshot;
	vi.unstubAllGlobals();
});

describe("review header", () => {
	it("shows the session identity and fastest timed lap", () => {
		window.traceReviewSnapshot = {
			summary,
			laps: [
				{
					id: "lap-one",
					index: 1,
					duration_ns: 92_456_000_000,
					validity: "valid",
					sectors: [
						{ index: 1, duration_ns: 29_000_000_000 },
						{ index: 2, duration_ns: 63_456_000_000 },
					],
				},
				{
					id: "lap-two",
					index: 2,
					duration_ns: 91_123_000_000,
					validity: "valid",
					sectors: [
						{ index: 1, duration_ns: 30_000_000_000 },
						{ index: 2, duration_ns: 61_123_000_000 },
					],
				},
			],
			publicShareId: null,
		};
		render(<ReviewHeader />);
		expect(screen.getByText("Race two")).toBeTruthy();
		expect(screen.getByText("1:31.123")).toBeTruthy();
		expect(screen.getByText("1:30.123")).toBeTruthy();
		expect(screen.getByText("Edit details")).toBeTruthy();
	});

	it("saves edited metadata and publishes the updated summary", async () => {
		window.traceReviewSnapshot = { summary, laps: [], publicShareId: null };
		const fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({ ...summary, title: "Sunday race", tags: ["wet", "league"] }),
		});
		vi.stubGlobal("fetch", fetch);
		let updatedTitle: string | undefined;
		window.addEventListener(
			"trace:review-summary-updated",
			(event) => {
				updatedTitle = (event as CustomEvent<{ summary: { title: string } }>).detail.summary.title;
			},
			{ once: true },
		);

		render(<ReviewHeader />);
		await userEvent.click(screen.getByText("Edit details"));
		const title = screen.getByPlaceholderText("Race 2 — wet setup test");
		await userEvent.clear(title);
		await userEvent.type(title, "Sunday race");
		const tags = screen.getByPlaceholderText("league, wet, endurance");
		await userEvent.clear(tags);
		await userEvent.type(tags, "wet, league");
		await userEvent.click(screen.getByText("Save details"));

		await waitFor(() => expect(updatedTitle).toBe("Sunday race"));
		expect(fetch).toHaveBeenCalledWith(
			"/api/v1/library/sessions/session-one",
			expect.objectContaining({ method: "PUT", body: expect.stringContaining('"tags":["wet","league"]') }),
		);
	});

	it("limits shared sessions to portable public actions", () => {
		window.traceReviewSnapshot = { summary, laps: [], publicShareId: "share-one" };
		render(<ReviewHeader />);
		expect(screen.queryByText("Edit details")).toBeNull();
		expect(screen.queryByText("Compare a lap")).toBeNull();
		expect(screen.getByText("Open in TRACE").getAttribute("href")).toContain("trace://shared-session");
		expect(screen.getByText("Download").getAttribute("href")).toBe("/api/v1/public-sessions/share-one/download");
	});
});
