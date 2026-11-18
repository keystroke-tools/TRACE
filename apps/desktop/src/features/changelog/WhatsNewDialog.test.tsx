import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { WhatsNewDialog } from "./WhatsNewDialog";

describe("What's new dialog", () => {
	it("shows the installed version and dismisses from the primary action", () => {
		const onClose = vi.fn();
		render(<WhatsNewDialog version="v0.8.20" onClose={onClose} />);

		expect(screen.getByRole("dialog")).toHaveTextContent("TRACE v0.8.20");
		expect(screen.getByRole("heading", { name: "Here's what changed" })).toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: "CONTINUE" }));
		expect(onClose).toHaveBeenCalledOnce();
	});

	it("dismisses with Escape", () => {
		const onClose = vi.fn();
		render(<WhatsNewDialog version="0.8.20" onClose={onClose} />);
		fireEvent.keyDown(document, { key: "Escape" });
		expect(onClose).toHaveBeenCalledOnce();
	});
});
