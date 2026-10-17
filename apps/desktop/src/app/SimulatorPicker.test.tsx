import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TelemetryStatus } from "../data-source";
import { ToastProvider } from "../Toast";
import { SimulatorPicker } from "./SimulatorPicker";

const status: TelemetryStatus = {
	simulatorId: "auto",
	activeSimulatorId: "assetto-corsa",
	simulatorName: "Assetto Corsa",
	simulatorShortName: "AC",
	simulators: [
		{ id: "auto", name: "Auto-detect", shortName: "AUTO", available: true },
		{ id: "assetto-corsa", name: "Assetto Corsa", shortName: "AC", available: true },
		{ id: "le-mans-ultimate", name: "Le Mans Ultimate", shortName: "LMU", available: true },
	],
	connection: "recording",
	source: "ASSETTO CORSA",
	sampleRateHz: 60,
	session: "TEST SESSION",
	replayCaptureArmed: false,
	channels: [],
};

describe("SimulatorPicker", () => {
	it("shows the simulator detected while auto mode remains selected", () => {
		render(
			<ToastProvider>
				<SimulatorPicker status={status} onSelect={async () => {}} />
			</ToastProvider>,
		);

		expect(screen.getByText("AUTO-DETECT")).toBeVisible();
		expect(screen.getByText("Assetto Corsa detected")).toBeVisible();
	});

	it("keeps manual simulator overrides available", async () => {
		const select = vi.fn().mockResolvedValue(undefined);
		render(
			<ToastProvider>
				<SimulatorPicker status={status} onSelect={select} />
			</ToastProvider>,
		);

		fireEvent.click(screen.getByRole("button", { name: /Le Mans Ultimate/ }));

		await waitFor(() => expect(select).toHaveBeenCalledWith("le-mans-ultimate"));
	});
});
