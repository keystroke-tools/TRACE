import { act, cleanup, render, screen, waitFor } from "@testing-library/preact";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountShell, AuthPanel } from "./account-shell";

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
	delete window.traceAccountSnapshot;
	vi.unstubAllGlobals();
});

describe("account shell", () => {
	it("offers first-run administrator creation even when registration is disabled", () => {
		window.traceAccountSnapshot = {
			instance: { name: "TRACE", version: "0.8.6", registration_enabled: false, initial_admin_required: true },
			account: null,
		};
		render(<AuthPanel />);
		expect(screen.getAllByText("Create administrator").length).toBeGreaterThan(0);
		expect(screen.getByText("Display name")).toBeTruthy();
	});

	it("signs in and publishes the authenticated account", async () => {
		window.traceAccountSnapshot = {
			instance: { name: "TRACE", version: "0.8.6", registration_enabled: false, initial_admin_required: false },
			account: null,
		};
		const account = { account_id: "one", username: "alex", display_name: "Alex", administrator: false };
		vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response(account)));
		let authenticated: unknown;
		window.addEventListener(
			"trace:authenticated",
			(event) => {
				authenticated = (event as CustomEvent).detail;
			},
			{ once: true },
		);
		render(<AuthPanel />);
		await userEvent.type(screen.getByLabelText("Username"), "alex");
		await userEvent.type(screen.getByLabelText("Password"), "correct horse battery staple");
		await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
		await waitFor(() => expect(authenticated).toEqual({ account }));
	});

	it("loads desktop tokens when the account control is opened", async () => {
		window.traceAccountSnapshot = {
			instance: { name: "TRACE", version: "0.8.6", registration_enabled: false, initial_admin_required: false },
			account: { account_id: "one", username: "alex", display_name: "Alex", administrator: true },
		};
		const fetch = vi
			.fn()
			.mockResolvedValue(
				response([
					{ token_id: "token-one", label: "Rig", token_prefix: "trace_abc", created_at_unix_ms: 1_793_000_000_000, last_used_at_unix_ms: null },
				]),
			);
		vi.stubGlobal("fetch", fetch);
		render(<AccountShell />);
		await userEvent.click(screen.getByText("Connect desktop"));
		await waitFor(() => expect(screen.getByText("Rig")).toBeTruthy());
		expect(fetch).toHaveBeenCalledWith("/api/v1/auth/tokens", expect.objectContaining({ credentials: "same-origin" }));
	});

	it("responds to account snapshots published after startup", () => {
		window.traceAccountSnapshot = { instance: null, account: null };
		render(<AccountShell />);
		act(() => {
			window.dispatchEvent(
				new CustomEvent("trace:account-load", {
					detail: {
						instance: { name: "TRACE", version: "0.8.6", registration_enabled: false, initial_admin_required: false },
						account: { account_id: "one", username: "alex", display_name: "Alex", administrator: false },
					},
				}),
			);
		});
		expect(screen.getByText("Alex · Private sessions stored on this TRACE instance.")).toBeTruthy();
	});
});
