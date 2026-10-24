import { useEffect, useRef, useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";

type Instance = {
	name: string;
	version: string;
	registration_enabled: boolean;
	initial_admin_required: boolean;
};

type Account = {
	account_id: string;
	username: string;
	display_name: string;
	administrator: boolean;
};

type AccountSnapshot = { instance: Instance | null; account: Account | null };

type Token = {
	token_id: string;
	label: string;
	token_prefix: string;
	created_at_unix_ms: number;
	last_used_at_unix_ms: number | null;
	token?: string;
};

declare global {
	interface Window {
		traceAccountSnapshot?: AccountSnapshot;
	}
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
	const response = await fetch(path, { credentials: "same-origin", ...options });
	if (response.status === 204) return undefined as T;
	const contentType = response.headers.get("content-type") || "";
	const body = contentType.includes("application/json") ? await response.json() : await response.text();
	if (!response.ok) {
		const detail = typeof body === "string" ? body.trim() : body?.error;
		throw new Error(detail || `Request failed (${response.status})`);
	}
	return body as T;
}

function emit(name: string, detail?: unknown) {
	window.dispatchEvent(new CustomEvent(name, { detail }));
}

function toast(message: string, kind: "success" | "error" = "success") {
	emit("trace:toast", { message, kind });
}

function useAccountSnapshot() {
	const [snapshot, setSnapshot] = useState<AccountSnapshot>(window.traceAccountSnapshot || { instance: null, account: null });
	useEffect(() => {
		const receive = (event: Event) => setSnapshot((event as CustomEvent<AccountSnapshot>).detail);
		window.addEventListener("trace:account-load", receive);
		return () => window.removeEventListener("trace:account-load", receive);
	}, []);
	return snapshot;
}

function AuthPanel() {
	const { instance } = useAccountSnapshot();
	const registrationOpen = Boolean(instance?.registration_enabled || instance?.initial_admin_required);
	const [mode, setMode] = useState<"login" | "register">(instance?.initial_admin_required ? "register" : "login");
	const [busy, setBusy] = useState(false);

	useEffect(() => {
		if (instance?.initial_admin_required) setMode("register");
		else if (!registrationOpen) setMode("login");
	}, [instance, registrationOpen]);

	async function submit(event: FormEvent<HTMLFormElement>) {
		event.preventDefault();
		const form = event.currentTarget;
		const values = Object.fromEntries(new FormData(form));
		setBusy(true);
		try {
			if (mode === "register") {
				await request("/api/v1/accounts", {
					method: "POST",
					headers: { "content-type": "application/json" },
					body: JSON.stringify(values),
				});
			}
			const account = await request<Account>("/api/v1/auth/sessions", {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({ username: values.username, password: values.password }),
			});
			form.reset();
			emit("trace:authenticated", { account });
		} catch (error) {
			toast(error instanceof Error ? error.message : "Authentication failed.", "error");
		} finally {
			setBusy(false);
		}
	}

	return (
		<>
			<div className="auth-intro">
				<p className="eyebrow">SELF-HOSTED TELEMETRY</p>
				<h1>
					Your laps,
					<br />
					<em>where you put them.</em>
				</h1>
				<p>
					Upload portable <code>.trace</code> sessions to this instance. Your library remains private to your account.
				</p>
			</div>
			<div className="auth-card">
				<div className="auth-tabs" role="tablist">
					<button className={mode === "login" ? "active" : ""} type="button" role="tab" onClick={() => setMode("login")}>
						Sign in
					</button>
					{registrationOpen && (
						<button className={mode === "register" ? "active" : ""} type="button" role="tab" onClick={() => setMode("register")}>
							{instance?.initial_admin_required ? "Create administrator" : "Create account"}
						</button>
					)}
				</div>
				<form onSubmit={submit}>
					{mode === "register" && (
						<label>
							Display name
							<input name="display_name" autoComplete="name" required maxLength={80} />
						</label>
					)}
					<label>
						Username
						<input name="username" autoComplete="username" required minLength={3} maxLength={32} />
					</label>
					<label>
						Password
						<input
							name="password"
							type="password"
							autoComplete={mode === "register" ? "new-password" : "current-password"}
							required
							minLength={mode === "register" ? 12 : undefined}
							maxLength={256}
						/>
					</label>
					{mode === "register" && <p className="field-note">Use at least 12 characters.</p>}
					<button className="primary" type="submit" disabled={busy}>
						{busy ? "Please wait…" : mode === "register" ? "Create account" : "Sign in"}
					</button>
				</form>
			</div>
		</>
	);
}

function formatDate(timestamp: number) {
	return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp));
}

function AccountShell() {
	const { account } = useAccountSnapshot();
	const [tokensOpen, setTokensOpen] = useState(false);
	const [tokens, setTokens] = useState<Token[]>([]);
	const [label, setLabel] = useState("TRACE desktop");
	const [secret, setSecret] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const secretInput = useRef<HTMLInputElement>(null);

	async function toggleTokens() {
		const open = !tokensOpen;
		setTokensOpen(open);
		if (!open) return;
		try {
			setTokens(await request<Token[]>("/api/v1/auth/tokens"));
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not load desktop tokens.", "error");
		}
	}

	async function createToken() {
		if (!label.trim()) return;
		setBusy(true);
		try {
			const created = await request<Token>("/api/v1/auth/tokens", {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({ label: label.trim() }),
			});
			setTokens((current) => [created, ...current]);
			setSecret(created.token || null);
			toast("Desktop token created.");
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not create a desktop token.", "error");
		} finally {
			setBusy(false);
		}
	}

	async function revoke(token: Token) {
		setBusy(true);
		try {
			await request<void>(`/api/v1/auth/tokens/${encodeURIComponent(token.token_id)}`, { method: "DELETE" });
			setTokens((current) => current.filter((candidate) => candidate.token_id !== token.token_id));
			toast("Desktop token revoked.");
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not revoke the desktop token.", "error");
		} finally {
			setBusy(false);
		}
	}

	async function copySecret() {
		if (!secret) return;
		try {
			await navigator.clipboard.writeText(secret);
			toast("Desktop token copied.");
		} catch {
			secretInput.current?.focus();
			secretInput.current?.select();
			const copied = document.execCommand("copy");
			toast(copied ? "Desktop token copied." : "The token is selected. Press Ctrl+C to copy it.", copied ? "success" : "error");
		}
	}

	async function logout() {
		try {
			await request<void>("/api/v1/auth/session", { method: "DELETE" });
			emit("trace:signed-out");
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not sign out.", "error");
		}
	}

	if (!account) return null;
	return (
		<>
			<div className="library-head">
				<div>
					<p className="eyebrow">02 / SESSION LIBRARY</p>
					<h1>SESSIONS</h1>
					<p>{account.display_name} · Private sessions stored on this TRACE instance.</p>
				</div>
				<div className="library-account-actions">
					{account.administrator && (
						<button type="button" onClick={() => emit("trace:admin-toggle")}>
							Instance settings
						</button>
					)}
					<button type="button" onClick={() => void toggleTokens()}>
						Connect desktop
					</button>
					<button className="quiet" type="button" onClick={() => void logout()}>
						Sign out
					</button>
				</div>
			</div>
			{tokensOpen && (
				<section className="token-panel">
					<div>
						<p className="eyebrow">DESKTOP ACCESS</p>
						<strong>Upload directly from the TRACE desktop app.</strong>
						<small>Create a revocable token, then paste it into Settings → Connectivity in TRACE.</small>
					</div>
					<div className="token-create">
						<input maxLength={40} value={label} aria-label="Token label" onChange={(event) => setLabel(event.target.value)} />
						<button className="primary" type="button" disabled={busy} onClick={() => void createToken()}>
							Create token
						</button>
					</div>
					{secret && (
						<div className="token-secret">
							<strong>Copy this token now. It will not be shown again.</strong>
							<div>
								<input ref={secretInput} readOnly value={secret} onClick={(event) => event.currentTarget.select()} />
								<button type="button" onClick={() => void copySecret()}>
									Copy
								</button>
							</div>
						</div>
					)}
					<div className="token-list">
						{tokens.map((token) => (
							<div className="token-row" key={token.token_id}>
								<span>
									<strong>{token.label}</strong>
									<small>
										{token.token_prefix}… · Created {formatDate(token.created_at_unix_ms)}
									</small>
								</span>
								<button className="danger" type="button" disabled={busy} onClick={() => void revoke(token)}>
									Revoke
								</button>
							</div>
						))}
					</div>
				</section>
			)}
		</>
	);
}

const authRoot = document.querySelector("#auth-root");
if (authRoot) createRoot(authRoot).render(<AuthPanel />);
const accountRoot = document.querySelector("#account-shell-root");
if (accountRoot) createRoot(accountRoot).render(<AccountShell />);

export { AccountShell, AuthPanel };
