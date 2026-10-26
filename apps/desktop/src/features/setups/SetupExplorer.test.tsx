import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SetupExplorer, type SetupExplorerDestination } from "./SetupExplorer";

describe("setup explorer drop destination", () => {
	it("exposes the selected simulator, car, and track and identifies the drop target", async () => {
		const onDestinationChange = vi.fn<(destination: SetupExplorerDestination | null) => void>();
		const { rerender } = render(<SetupExplorer refreshKey={0} dropActive={false} importing={false} onDestinationChange={onDestinationChange} />);

		await screen.findByText("race.ini");
		await waitFor(() =>
			expect(onDestinationChange).toHaveBeenLastCalledWith({
				simulatorId: "assetto-corsa",
				simulatorName: "Assetto Corsa",
				sourceCarId: "ks_mazda_mx5_cup",
				carName: "Mazda MX-5 Cup",
				sourceTrackId: "ks_zandvoort",
				trackName: "Zandvoort",
				layoutId: undefined,
			}),
		);

		rerender(<SetupExplorer refreshKey={0} dropActive importing={false} onDestinationChange={onDestinationChange} />);
		expect(screen.getByText("DROP INTO THIS TRACK")).toBeInTheDocument();
		expect(screen.getByText("Assetto Corsa · ks_mazda_mx5_cup · ks_zandvoort")).toBeInTheDocument();
	});
});
