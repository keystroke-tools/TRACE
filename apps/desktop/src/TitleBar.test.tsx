import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { telemetryDataSource } from "./data-source";
import { TitleBar } from "./TitleBar";

function renderTitleBar() {
	render(<TitleBar status={null} liveBroadcast={null} liveMode="hosted" onLiveModeChange={() => {}} onStopLive={() => {}} />);
}

afterEach(() => {
	vi.restoreAllMocks();
});

describe("TitleBar window controls", () => {
	it("asks for confirmation when the close button is right-clicked", async () => {
		vi.spyOn(telemetryDataSource, "getAppBehaviorSettings").mockResolvedValue({ closeToTrayEnabled: true, confirmExitEnabled: true });
		renderTitleBar();

		fireEvent.contextMenu(screen.getByRole("button", { name: "Close" }));

		expect(await screen.findByRole("dialog", { name: "Quit TRACE completely?" })).toBeVisible();
	});

	it("quits immediately when confirmation is disabled", async () => {
		vi.spyOn(telemetryDataSource, "getAppBehaviorSettings").mockResolvedValue({ closeToTrayEnabled: true, confirmExitEnabled: false });
		const quit = vi.spyOn(telemetryDataSource, "quitApp").mockResolvedValue();
		renderTitleBar();

		fireEvent.contextMenu(screen.getByRole("button", { name: "Close" }));

		await waitFor(() => expect(quit).toHaveBeenCalledOnce());
		expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
	});
});
