import { useEffect, useMemo, useRef, useState, type ChangeEvent, type DragEvent, type KeyboardEvent, type MouseEvent } from "react";
import { createRoot } from "react-dom/client";
import { formatSessionDate, formatSessionType, SessionIdentityBlock } from "@trace/ui";

type Session = {
	session_id: string;
	title: string | null;
	driver_name: string | null;
	simulator: string;
	track: string | null;
	layout: string | null;
	car: string | null;
	session_type: string | null;
	started_at: string;
	tags: string[];
	lap_count: number;
	size_bytes: number;
	share_id: string | null;
};

type Snapshot = {
	sessions: Session[];
	maximumUploadBytes: number;
};

declare global {
	interface Window {
		traceLibrarySnapshot?: Snapshot;
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

function emitToast(message: string, kind: "success" | "error" = "success") {
	window.dispatchEvent(new CustomEvent("trace:toast", { detail: { message, kind } }));
}

function emitSessions(sessions: Session[]) {
	window.dispatchEvent(new CustomEvent("trace:sessions-updated", { detail: { sessions } }));
}

function formatBytes(bytes: number) {
	if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}

function searchText(session: Session) {
	return [session.title, session.driver_name, session.simulator, session.track, session.layout, session.car, session.session_type, ...session.tags]
		.filter(Boolean)
		.join(" ")
		.toLocaleLowerCase();
}

export function Sessions() {
	const initial = window.traceLibrarySnapshot;
	const [sessions, setSessions] = useState<Session[]>(initial?.sessions || []);
	const [maximumUploadBytes, setMaximumUploadBytes] = useState(initial?.maximumUploadBytes || 0);
	const [query, setQuery] = useState("");
	const [dragging, setDragging] = useState(false);
	const [uploading, setUploading] = useState(false);
	const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);
	const confirmationTimer = useRef<number | null>(null);

	useEffect(() => {
		const receive = (event: Event) => {
			const snapshot = (event as CustomEvent<Snapshot>).detail;
			setSessions(snapshot.sessions);
			setMaximumUploadBytes(snapshot.maximumUploadBytes);
		};
		window.addEventListener("trace:sessions-load", receive);
		return () => window.removeEventListener("trace:sessions-load", receive);
	}, []);

	useEffect(
		() => () => {
			if (confirmationTimer.current) window.clearTimeout(confirmationTimer.current);
		},
		[],
	);

	const filtered = useMemo(() => {
		const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
		return terms.length ? sessions.filter((session) => terms.every((term) => searchText(session).includes(term))) : sessions;
	}, [query, sessions]);

	function updateSessions(next: Session[]) {
		setSessions(next);
		emitSessions(next);
	}

	async function upload(file?: File) {
		if (!file) return;
		if (!file.name.toLocaleLowerCase().endsWith(".trace")) {
			emitToast("Choose a .trace package.", "error");
			return;
		}
		setUploading(true);
		try {
			await request<Session>("/api/v1/library/sessions", {
				method: "POST",
				headers: { "content-type": "application/vnd.simtrace.session" },
				body: file,
			});
			updateSessions(await request<Session[]>("/api/v1/library/sessions"));
			emitToast("Session uploaded to your private library.");
		} catch (error) {
			emitToast(error instanceof Error ? error.message : "Could not upload the session.", "error");
		} finally {
			setUploading(false);
			setDragging(false);
		}
	}

	async function remove(session: Session, event: MouseEvent<HTMLButtonElement>) {
		event.stopPropagation();
		if (confirmingDelete !== session.session_id) {
			setConfirmingDelete(session.session_id);
			if (confirmationTimer.current) window.clearTimeout(confirmationTimer.current);
			confirmationTimer.current = window.setTimeout(() => setConfirmingDelete(null), 3000);
			return;
		}
		try {
			await request<void>(`/api/v1/library/sessions/${encodeURIComponent(session.session_id)}`, { method: "DELETE" });
			updateSessions(sessions.filter((candidate) => candidate.session_id !== session.session_id));
			setConfirmingDelete(null);
			emitToast("Session deleted.");
		} catch (error) {
			emitToast(error instanceof Error ? error.message : "Could not delete the session.", "error");
		}
	}

	function openSession(sessionId: string) {
		window.dispatchEvent(new CustomEvent("trace:open-session", { detail: { sessionId } }));
	}

	function openWithKeyboard(event: KeyboardEvent<HTMLElement>, sessionId: string) {
		if (event.key !== "Enter" && event.key !== " ") return;
		event.preventDefault();
		openSession(sessionId);
	}

	function dropped(event: DragEvent<HTMLLabelElement>) {
		event.preventDefault();
		void upload(event.dataTransfer.files[0]);
	}

	return (
		<>
			<div className="library-tools">
				<label
					className={`upload-zone ${dragging || uploading ? "dragging" : ""}`}
					onDragOver={(event) => {
						event.preventDefault();
						setDragging(true);
					}}
					onDragLeave={() => setDragging(false)}
					onDrop={dropped}
				>
					<input
						type="file"
						accept=".trace,application/vnd.simtrace.session"
						disabled={uploading}
						onChange={(event: ChangeEvent<HTMLInputElement>) => {
							void upload(event.target.files?.[0]);
							event.target.value = "";
						}}
					/>
					<span className="upload-mark">+</span>
					<strong>{uploading ? "Uploading session…" : "Upload a TRACE session"}</strong>
					<small>Choose or drop a .trace package · up to {formatBytes(maximumUploadBytes)}</small>
				</label>
				<label className="search-field">
					<span>Search library</span>
					<input value={query} onChange={(event) => setQuery(event.target.value)} type="search" placeholder="Driver, car, track, tag…" />
				</label>
			</div>

			<div className="list-head">
				<strong>SESSIONS</strong>
				<span>{query.trim() ? `${filtered.length}/${sessions.length}` : sessions.length}</span>
			</div>
			<div className="session-list">
				{filtered.map((session) => {
					const title = session.title || session.driver_name || "Recorded session";
					const driver = session.title ? session.driver_name : null;
					const sessionType = `${session.share_id ? "UNLISTED · " : ""}${formatSessionType(session.session_type)} · ${session.lap_count} ${session.lap_count === 1 ? "LAP" : "LAPS"}`;
					return (
						<article
							className="session-row"
							key={session.session_id}
							tabIndex={0}
							role="button"
							onClick={() => openSession(session.session_id)}
							onKeyDown={(event) => openWithKeyboard(event, session.session_id)}
						>
							<SessionIdentityBlock
								sessionType={sessionType}
								title={title}
								car={session.car || "Unknown car"}
								date={formatSessionDate(session.started_at)}
								driver={driver}
							/>
							<div className="session-context">
								<strong>{session.track || "Unknown track"}</strong>
								<small>
									{session.simulator || "Unknown simulator"} · {formatBytes(session.size_bytes)}
								</small>
							</div>
							<div className="session-actions">
								<a
									href={`/api/v1/library/sessions/${encodeURIComponent(session.session_id)}/download`}
									onClick={(event) => event.stopPropagation()}
								>
									Download
								</a>
								<button className="danger" type="button" onClick={(event) => void remove(session, event)}>
									{confirmingDelete === session.session_id ? "Confirm" : "Delete"}
								</button>
							</div>
						</article>
					);
				})}
			</div>
			{sessions.length === 0 && (
				<div className="empty-state">
					<strong>No sessions here yet.</strong>
					<p>Export a session from TRACE and upload the resulting package.</p>
				</div>
			)}
			{sessions.length > 0 && filtered.length === 0 && (
				<div className="empty-state">
					<strong>No matching sessions.</strong>
					<p>Try another driver, car, track, or tag.</p>
				</div>
			)}
		</>
	);
}

const root = document.querySelector("#session-library-root");
if (root) createRoot(root).render(<Sessions />);
