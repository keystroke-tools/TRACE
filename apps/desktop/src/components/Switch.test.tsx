import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SettingSwitch, Switch } from "./Switch";

describe("Switch", () => {
	it("exposes switch semantics and requests the opposite state", async () => {
		const onCheckedChange = vi.fn();
		const user = userEvent.setup();
		render(<Switch checked={false} onCheckedChange={onCheckedChange} label="Share Discord activity" />);

		const control = screen.getByRole("switch", { name: "Share Discord activity" });
		expect(control).toHaveAttribute("aria-checked", "false");
		await user.click(control);
		expect(onCheckedChange).toHaveBeenCalledWith(true);
	});

	it("supports keyboard activation through its native button", async () => {
		const onCheckedChange = vi.fn();
		const user = userEvent.setup();
		render(<Switch checked onCheckedChange={onCheckedChange} label="Automatic streaming" />);

		await user.tab();
		expect(screen.getByRole("switch")).toHaveFocus();
		await user.keyboard(" ");
		expect(onCheckedChange).toHaveBeenCalledWith(false);
	});

	it("does not change while disabled", async () => {
		const onCheckedChange = vi.fn();
		const user = userEvent.setup();
		render(<Switch checked={false} disabled onCheckedChange={onCheckedChange} label="Launch on startup" />);

		const control = screen.getByRole("switch", { name: "Launch on startup" });
		expect(control).toBeDisabled();
		await user.click(control);
		expect(onCheckedChange).not.toHaveBeenCalled();
	});
});

describe("SettingSwitch", () => {
	it("associates its visible title and description with the control", () => {
		render(
			<SettingSwitch
				title="Keep TRACE running in the system tray"
				description="Closing the window keeps recording in the background."
				checked
				onCheckedChange={() => {}}
			/>,
		);

		const control = screen.getByRole("switch", { name: "Keep TRACE running in the system tray" });
		expect(control).toHaveAccessibleDescription("Closing the window keeps recording in the background.");
	});

	it("uses the whole labelled row as its click target", async () => {
		const onCheckedChange = vi.fn();
		const user = userEvent.setup();
		render(
			<SettingSwitch
				title="Rich comparison session picker"
				description="Show the structured session picker."
				checked={false}
				onCheckedChange={onCheckedChange}
			/>,
		);

		await user.click(screen.getByText("Rich comparison session picker"));
		expect(onCheckedChange).toHaveBeenCalledOnce();
		expect(onCheckedChange).toHaveBeenCalledWith(true);
	});

	it("does not activate from its labelled row while disabled", async () => {
		const onCheckedChange = vi.fn();
		const user = userEvent.setup();
		render(
			<SettingSwitch
				title="Launch TRACE when Windows starts"
				description="Open TRACE after signing in."
				checked={false}
				disabled
				onCheckedChange={onCheckedChange}
			/>,
		);

		await user.click(screen.getByText("Open TRACE after signing in."));
		expect(onCheckedChange).not.toHaveBeenCalled();
	});
});
