import { act, cleanup, render, screen, waitFor } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SharePanel } from "./share-panel";

const summary = {
	session_id: "session-one",
	title: "Sunday race",
	driver_name: "Alex",
	track: "Interlagos",
	share_id: null,
};

function response(body?: unknown, status = 200) {
	return {
		ok: status >= 200 && status < 300,
		status,
		headers: { get: () => (body ? "application/json" : "") },
		json: async () => body,
		text: async () => "",
	};
}

afterEach(() => {
	cleanup();
	delete window.traceShareSnapshot;
	vi.unstubAllGlobals();
});

describe("share panel", () => {
	it("creates a new unlisted link when sharing is opened", async () => {
		window.traceShareSnapshot = { summary, shareUrl: null, open: false };
		vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response({ share_id: "share-one", share_url: "https://trace.test/shared/one" })));
		let update: { shareId: string; open: boolean } | undefined;
		window.addEventListener(
			"trace:share-updated",
			(event) => {
				update = (event as CustomEvent<typeof update>).detail;
			},
			{ once: true },
		);
		render(<SharePanel />);
		act(() => {
			window.dispatchEvent(new Event("trace:share-open"));
		});
		await waitFor(() => expect(update).toEqual(expect.objectContaining({ shareId: "share-one", open: true })));
	});

	it("requires confirmation before revoking an existing link", async () => {
		window.traceShareSnapshot = { summary: { ...summary, share_id: "share-one" }, shareUrl: "https://trace.test/shared/one", open: true };
		const fetch = vi.fn().mockResolvedValue(response(undefined, 204));
		vi.stubGlobal("fetch", fetch);
		let revoked = false;
		window.addEventListener(
			"trace:share-updated",
			(event) => {
				revoked = (event as CustomEvent<{ shareId: string | null }>).detail.shareId === null;
			},
			{ once: true },
		);
		render(<SharePanel />);
		await userEvent.click(screen.getByText("Revoke"));
		expect(fetch).not.toHaveBeenCalled();
		await userEvent.click(screen.getByText("Confirm revoke"));
		await waitFor(() => expect(revoked).toBe(true));
		expect(fetch).toHaveBeenCalledWith("/api/v1/library/sessions/session-one/share", expect.objectContaining({ method: "DELETE" }));
	});
});
