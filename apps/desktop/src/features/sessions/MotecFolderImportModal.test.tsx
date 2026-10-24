import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { telemetryDataSource, type MotecFolderScan } from "../../data-source";
import { ToastProvider } from "../../Toast";
import { MotecFolderImportModal } from "./MotecFolderImportModal";

const preview = {
	simulatorKey: "assetto-corsa",
	track: "Zandvoort",
	car: "Mazda MX-5 Cup",
	driver: "Alex",
	sessionType: "Hotlap",
	title: "",
	completeLapCount: 3,
	sampleCount: 12_000,
};

const scan: MotecFolderScan = {
	folder: "C:/Telemetry",
	limited: false,
	entries: [
		{ path: "C:/Telemetry/one.ld", fileName: "one.ld", sidecarPresent: true, preview },
		{ path: "C:/Telemetry/two.ld", fileName: "two.ld", sidecarPresent: false, preview },
		{ path: "C:/Telemetry/broken.ld", fileName: "broken.ld", sidecarPresent: false, error: "No completed lap markers were found." },
	],
};

describe("MoTeC folder import", () => {
	it("imports valid selected logs and keeps partial failures visible", async () => {
		const importSession = vi.spyOn(telemetryDataSource, "importSession");
		importSession.mockResolvedValueOnce({ sessionId: "one", lapCount: 3, sampleCount: 12_000 });
		importSession.mockRejectedValueOnce(new Error("Unsupported channel layout"));
		const onImported = vi.fn().mockResolvedValue(undefined);
		const onClose = vi.fn();
		render(
			<ToastProvider>
				<MotecFolderImportModal scan={scan} onClose={onClose} onImported={onImported} />
			</ToastProvider>,
		);

		expect(screen.getByText("2 ready · 1 unavailable")).toBeInTheDocument();
		expect(screen.getAllByText("NO LDX")).toHaveLength(2);
		await userEvent.click(screen.getByRole("button", { name: "IMPORT 2 SELECTED" }));

		await waitFor(() => expect(importSession).toHaveBeenCalledTimes(2));
		expect(importSession).toHaveBeenNthCalledWith(1, "C:/Telemetry/one.ld");
		expect(importSession).toHaveBeenNthCalledWith(2, "C:/Telemetry/two.ld");
		expect(onImported).toHaveBeenCalledOnce();
		expect(onClose).not.toHaveBeenCalled();
		expect(await screen.findByText("Unsupported channel layout")).toBeInTheDocument();
		expect(screen.getByText("IMPORTED")).toBeInTheDocument();
	});
});
