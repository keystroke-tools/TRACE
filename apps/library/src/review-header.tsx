import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import { formatSessionDate, formatSessionType } from "@trace/ui";

type Summary = {
	session_id: string;
	title: string | null;
	driver_name: string | null;
	simulator: string;
	track: string | null;
	car: string | null;
	session_type: string | null;
	started_at: string;
	tags: string[];
	share_id?: string | null;
};

type Lap = {
	duration_ns: number | null;
};

type ReviewSnapshot = {
	summary: Summary;
	laps: Lap[];
	publicShareId: string | null;
};

declare global {
	interface Window {
		traceReviewSnapshot?: ReviewSnapshot | null;
	}
}

async function request<T>(path: string, options: RequestInit): Promise<T> {
	const response = await fetch(path, { credentials: "same-origin", ...options });
	const body = (await response.json()) as T & { error?: string };
	if (!response.ok) throw new Error(body.error || `Request failed (${response.status})`);
	return body;
}

function emit(name: string, detail?: unknown) {
	window.dispatchEvent(new CustomEvent(name, { detail }));
}

function toast(message: string, kind: "success" | "error" = "success") {
	emit("trace:toast", { message, kind });
}

function formatDuration(nanoseconds: number) {
	const milliseconds = Math.round(nanoseconds / 1_000_000);
	const minutes = Math.floor(milliseconds / 60_000);
	const seconds = Math.floor((milliseconds % 60_000) / 1000);
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds % 1000).padStart(3, "0")}`;
}

function ReviewHeader() {
	const [snapshot, setSnapshot] = useState<ReviewSnapshot | null>(window.traceReviewSnapshot || null);
	const [editing, setEditing] = useState(false);
	const [busy, setBusy] = useState(false);
	const [title, setTitle] = useState("");
	const [driver, setDriver] = useState("");
	const [tags, setTags] = useState("");
	const titleInput = useRef<HTMLInputElement>(null);

	useEffect(() => {
		const receive = (event: Event) => {
			setSnapshot((event as CustomEvent<ReviewSnapshot | null>).detail);
			setEditing(false);
		};
		window.addEventListener("trace:review-load", receive);
		return () => window.removeEventListener("trace:review-load", receive);
	}, []);

	const bestLap = useMemo(() => {
		const timed = snapshot?.laps.map((lap) => lap.duration_ns).filter((duration): duration is number => duration != null) || [];
		return timed.length ? Math.min(...timed) : null;
	}, [snapshot]);

	if (!snapshot) return null;
	const { summary, publicShareId } = snapshot;
	const downloadPath = publicShareId
		? `/api/v1/public-sessions/${encodeURIComponent(publicShareId)}/download`
		: `/api/v1/library/sessions/${encodeURIComponent(summary.session_id)}/download`;
	const downloadUrl = new URL(downloadPath, window.location.origin).href;
	const appLink = `trace://shared-session?url=${encodeURIComponent(downloadUrl)}`;

	function openEditor() {
		setTitle(summary.title || "");
		setDriver(summary.driver_name || "");
		setTags((summary.tags || []).join(", "));
		setEditing(true);
		window.setTimeout(() => titleInput.current?.focus());
	}

	async function save(event: FormEvent) {
		event.preventDefault();
		setBusy(true);
		try {
			const updated = await request<Summary>(`/api/v1/library/sessions/${encodeURIComponent(summary.session_id)}`, {
				method: "PUT",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({
					title,
					driver_name: driver,
					tags: tags
						.split(",")
						.map((tag) => tag.trim())
						.filter(Boolean),
				}),
			});
			setSnapshot((current) => (current ? { ...current, summary: updated } : current));
			setEditing(false);
			emit("trace:review-summary-updated", { summary: updated });
			toast("Session details updated.");
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not update session details.", "error");
		} finally {
			setBusy(false);
		}
	}

	return (
		<>
			<div className="review-head">
				{!publicShareId && (
					<button className="back-button" type="button" onClick={() => emit("trace:review-back")}>
						← Library
					</button>
				)}
				<div className="review-actions">
					{!publicShareId && (
						<>
							<button type="button" onClick={() => emit("trace:compare-open")}>
								Compare a lap
							</button>
							<button type="button" onClick={openEditor}>
								Edit details
							</button>
							<button id="share-open" type="button" onClick={() => emit("trace:share-open")}>
								{summary.share_id ? "Shared" : "Share"}
							</button>
						</>
					)}
					{publicShareId && (
						<>
							<a id="review-open-trace" className="button-link" href={appLink}>
								Open in TRACE
							</a>
							<button type="button" onClick={() => emit("trace:copy-app-link")}>
								Copy app link
							</button>
						</>
					)}
					<a className="button-link" href={downloadPath}>
						Download
					</a>
				</div>
			</div>

			{editing && (
				<form className="metadata-panel" onSubmit={save}>
					<div className="metadata-heading">
						<div>
							<p className="eyebrow">SESSION DETAILS</p>
							<strong>Make this session easier to identify.</strong>
						</div>
						<button className="quiet" type="button" onClick={() => setEditing(false)}>
							Cancel
						</button>
					</div>
					<div className="metadata-fields">
						<label>
							Title
							<input
								ref={titleInput}
								maxLength={120}
								placeholder="Race 2 — wet setup test"
								value={title}
								onChange={(event) => setTitle(event.target.value)}
							/>
						</label>
						<label>
							Driver
							<input maxLength={80} placeholder="Driver name" value={driver} onChange={(event) => setDriver(event.target.value)} />
						</label>
						<label className="metadata-tags">
							Tags
							<input maxLength={491} placeholder="league, wet, endurance" value={tags} onChange={(event) => setTags(event.target.value)} />
						</label>
						<button className="primary" type="submit" disabled={busy}>
							{busy ? "Saving…" : "Save details"}
						</button>
					</div>
					<small>Separate tags with commas. These edits apply to this web library and its shared view.</small>
				</form>
			)}

			<div className="review-title">
				<div>
					<p className="eyebrow">
						{publicShareId ? "Shared session · " : ""}
						{formatSessionType(summary.session_type)} · {summary.simulator || "Unknown simulator"}
					</p>
					<h1>{summary.title || summary.driver_name || "Recorded session"}</h1>
					<p>
						{summary.track || "Unknown track"} · {summary.car || "Unknown car"} · {formatSessionDate(summary.started_at)}
					</p>
				</div>
				{bestLap != null && (
					<div className="best-lap">
						<small>BEST LAP</small>
						<strong>{formatDuration(bestLap)}</strong>
					</div>
				)}
			</div>
		</>
	);
}

const root = document.querySelector("#review-header-root");
if (root) createRoot(root).render(<ReviewHeader />);

export { ReviewHeader };
