import { StrictMode, useCallback, useEffect, useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import { SectionHeading } from "@trace/ui";
import "./compare-picker";
import "./lap-list";
import "./review-header";
import "./sessions";
import "./share-panel";
import "./telemetry-workspace";

type Cleanup = {
	session_count: number;
	size_bytes: number;
};

type Settings = {
	registration_enabled: boolean;
	library_retention_days: number;
	library_max_sessions_per_account: number;
	cleanup: Cleanup;
};

type EditableSettings = Omit<Settings, "cleanup">;

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
	const response = await fetch(path, { credentials: "same-origin", ...options });
	const body = (await response.json()) as T & { error?: string };
	if (!response.ok) throw new Error(body.error || `Request failed (${response.status})`);
	return body;
}

function formatBytes(bytes: number) {
	if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}

function emitToast(message: string, kind: "success" | "error" = "success") {
	window.dispatchEvent(new CustomEvent("trace:toast", { detail: { message, kind } }));
}

function AdminSettings() {
	const [settings, setSettings] = useState<EditableSettings | null>(null);
	const [cleanup, setCleanup] = useState<Cleanup>({ session_count: 0, size_bytes: 0 });
	const [busy, setBusy] = useState(false);

	const load = useCallback(async () => {
		try {
			const loaded = await request<Settings>("/api/v1/admin/settings");
			setSettings(loaded);
			setCleanup(loaded.cleanup);
		} catch (error) {
			emitToast(error instanceof Error ? error.message : "Could not load instance settings.", "error");
		}
	}, []);

	useEffect(() => {
		const handleOpen = () => void load();
		window.addEventListener("trace:admin-open", handleOpen);
		return () => window.removeEventListener("trace:admin-open", handleOpen);
	}, [load]);

	useEffect(() => {
		if (!settings) return;
		const timer = window.setTimeout(async () => {
			try {
				setCleanup(
					await request<Cleanup>("/api/v1/admin/library-retention-preview", {
						method: "POST",
						headers: { "content-type": "application/json" },
						body: JSON.stringify(settings),
					}),
				);
			} catch (error) {
				emitToast(error instanceof Error ? error.message : "Could not preview retention.", "error");
			}
		}, 250);
		return () => window.clearTimeout(timer);
	}, [settings]);

	async function save(event: FormEvent) {
		event.preventDefault();
		if (!settings) return;
		setBusy(true);
		try {
			const saved = await request<Settings>("/api/v1/admin/settings", {
				method: "PUT",
				headers: { "content-type": "application/json" },
				body: JSON.stringify(settings),
			});
			setSettings(saved);
			setCleanup({ session_count: 0, size_bytes: 0 });
			window.dispatchEvent(new Event("trace:library-changed"));
			emitToast(
				saved.cleanup.session_count
					? `Settings saved. ${saved.cleanup.session_count} ${saved.cleanup.session_count === 1 ? "session" : "sessions"} removed.`
					: "Instance settings saved.",
			);
		} catch (error) {
			emitToast(error instanceof Error ? error.message : "Could not save instance settings.", "error");
		} finally {
			setBusy(false);
		}
	}

	if (!settings) return <p className="admin-loading">Loading instance settings…</p>;

	return (
		<form className="admin-settings-form" onSubmit={save}>
			<div>
				<SectionHeading index="INSTANCE">Settings</SectionHeading>
				<strong>Access and library retention</strong>
				<small>Zero means unlimited. Retention applies to private account libraries, not the live replay buffer.</small>
			</div>
			<label className="admin-toggle">
				<input
					type="checkbox"
					checked={settings.registration_enabled}
					onChange={(event) => setSettings({ ...settings, registration_enabled: event.target.checked })}
				/>
				<span>Allow new account registrations</span>
			</label>
			<div className="admin-retention-fields">
				<label>
					Delete after days
					<input
						type="number"
						min="0"
						max="3650"
						value={settings.library_retention_days}
						onChange={(event) => setSettings({ ...settings, library_retention_days: Number(event.target.value) || 0 })}
					/>
				</label>
				<label>
					Sessions per user
					<input
						type="number"
						min="0"
						max="100000"
						value={settings.library_max_sessions_per_account}
						onChange={(event) => setSettings({ ...settings, library_max_sessions_per_account: Number(event.target.value) || 0 })}
					/>
				</label>
			</div>
			<div className="admin-retention-action">
				<span className={cleanup.session_count ? "danger" : undefined}>
					{cleanup.session_count
						? `${cleanup.session_count} ${cleanup.session_count === 1 ? "session" : "sessions"} (${formatBytes(cleanup.size_bytes)}) will be removed when saved.`
						: "No sessions would be removed."}
				</span>
				<button className="primary" type="submit" disabled={busy}>
					{busy ? "Saving…" : "Save settings"}
				</button>
			</div>
		</form>
	);
}

const root = document.querySelector("#admin-panel");
if (root)
	createRoot(root).render(
		<StrictMode>
			<AdminSettings />
		</StrictMode>,
	);
