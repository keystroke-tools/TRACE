import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { telemetryDataSource, type RecordedLapSummary, type RecordedSessionSummary } from "../../data-source";
import { ToastProvider } from "../../Toast";
import { SessionSummaryModal } from "./SessionSummaryModal";

function lap(index: number, validity: RecordedLapSummary["validity"]): RecordedLapSummary {
	return {
		index,
		time: `1:4${index}.000`,
		durationNs: (100 + index) * 1_000_000_000,
		validity,
		isFastest: index === 1,
		sectors: [],
	};
}

const session: RecordedSessionSummary = {
	id: "completed-session",
	simulatorId: "assetto-corsa",
	simulatorName: "Assetto Corsa",
	ownership: "mine",
	tags: [],
	track: "Zandvoort",
	car: "Mazda MX-5 Cup",
	sessionType: "hotlap",
	startedAt: "2026-10-28T12:00:00Z",
	source: "drive",
	exportable: true,
	deletable: true,
	laps: [lap(1, "valid"), lap(2, "invalid")],
};

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe("completed session private-library sync", () => {
	it("offers the existing selective lap picker in ask mode", async () => {
		vi.spyOn(telemetryDataSource, "getLibraryConnectionSettings").mockResolvedValue({
			endpoint: "https://library.example.test",
			tokenConfigured: true,
			syncMode: "ask",
		});

		render(
			<ToastProvider>
				<SessionSummaryModal session={session} sessions={[session]} completed onClose={() => {}} />
			</ToastProvider>,
		);

		expect(await screen.findByText("Keep this session in your private library?")).toBeInTheDocument();
		await userEvent.click(screen.getByRole("button", { name: "CHOOSE LAPS" }));
		expect(screen.getByRole("dialog", { name: "Upload selected laps" })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "VALID" })).toBeInTheDocument();
	});

	it("automatically uploads only valid timed laps", async () => {
		vi.spyOn(telemetryDataSource, "getLibraryConnectionSettings").mockResolvedValue({
			endpoint: "https://library.example.test",
			tokenConfigured: true,
			syncMode: "automatic",
		});
		const upload = vi.spyOn(telemetryDataSource, "uploadLibrarySession").mockResolvedValue({
			sessionId: "remote-session",
			lapCount: 1,
			endpoint: "https://library.example.test",
		});

		render(
			<ToastProvider>
				<SessionSummaryModal session={session} sessions={[session]} completed onClose={() => {}} />
			</ToastProvider>,
		);

		await waitFor(() => expect(upload).toHaveBeenCalledWith("completed-session", [1]));
		expect(await screen.findByText("Session synced")).toBeInTheDocument();
	});
});
