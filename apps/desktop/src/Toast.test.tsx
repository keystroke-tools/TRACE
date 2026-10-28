import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ToastProvider, useToast } from "./Toast";

function ActionToast({ action }: { action: () => void }) {
	const showToast = useToast();
	return (
		<button
			type="button"
			onClick={() =>
				showToast({
					title: "Upload complete",
					message: "Session added.",
					action: { label: "OPEN LIBRARY", onClick: action },
				})
			}
		>
			SHOW
		</button>
	);
}

describe("Toast", () => {
	it("runs and dismisses an optional action", async () => {
		const action = vi.fn();
		render(
			<ToastProvider>
				<ActionToast action={action} />
			</ToastProvider>,
		);

		await userEvent.click(screen.getByRole("button", { name: "SHOW" }));
		await userEvent.click(screen.getByRole("button", { name: "OPEN LIBRARY" }));

		expect(action).toHaveBeenCalledOnce();
		await waitFor(() => expect(screen.queryByRole("button", { name: "OPEN LIBRARY" })).not.toBeInTheDocument());
	});
});
