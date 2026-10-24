import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { formatSessionDate } from "@trace/ui";

type Summary = {
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
};

type Sector = { index: number; duration_ns: number | null };

type Lap = {
	id: string;
	index: number;
	duration_ns: number | null;
	validity: string;
	max_tyres_out?: number | null;
	sectors: Sector[];
};

type Review = {
	summary: Summary;
	laps: Lap[];
	source_track_id?: string | null;
	source_car_id?: string | null;
};

type CompareSnapshot = {
	sessions: Summary[];
	currentReview: Review;
	selectedLap: Lap;
};

declare global {
	interface Window {
		traceCompareSnapshot?: CompareSnapshot | null;
	}
}

async function request<T>(path: string): Promise<T> {
	const response = await fetch(path, { credentials: "same-origin" });
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

function formatDuration(nanoseconds: number | null) {
	if (nanoseconds == null) return "—";
	const milliseconds = Math.round(nanoseconds / 1_000_000);
	const minutes = Math.floor(milliseconds / 60_000);
	const seconds = Math.floor((milliseconds % 60_000) / 1000);
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds % 1000).padStart(3, "0")}`;
}

function text(value: string | null | undefined) {
	return value?.trim().toLocaleLowerCase() || "";
}

function searchText(session: Summary) {
	return [session.title, session.driver_name, session.simulator, session.track, session.layout, session.car, session.session_type, ...(session.tags || [])]
		.filter(Boolean)
		.join(" ")
		.toLocaleLowerCase();
}

function summaryCompatible(current: Summary, candidate: Summary) {
	return candidate.simulator === current.simulator && text(candidate.track) === text(current.track) && text(candidate.car) === text(current.car);
}

function reviewsCompatible(current: Review, candidate: Review) {
	const sameIdentity = (
		leftSource: string | null | undefined,
		rightSource: string | null | undefined,
		leftLabel: string | null,
		rightLabel: string | null,
	) => (leftSource && rightSource ? leftSource === rightSource : text(leftLabel) === text(rightLabel));
	return (
		current.summary.simulator === candidate.summary.simulator &&
		sameIdentity(current.source_track_id, candidate.source_track_id, current.summary.track, candidate.summary.track) &&
		sameIdentity(current.source_car_id, candidate.source_car_id, current.summary.car, candidate.summary.car) &&
		text(current.summary.layout) === text(candidate.summary.layout)
	);
}

function ComparePicker() {
	const [snapshot, setSnapshot] = useState<CompareSnapshot | null>(window.traceCompareSnapshot || null);
	const [query, setQuery] = useState("");
	const [candidate, setCandidate] = useState<Review | null>(null);
	const [message, setMessage] = useState("Select a session to see its laps.");
	const [loadingSession, setLoadingSession] = useState(false);
	const [loadingLap, setLoadingLap] = useState(false);
	const requestSequence = useRef(0);
	const search = useRef<HTMLInputElement>(null);

	useEffect(() => {
		const receive = (event: Event) => {
			requestSequence.current += 1;
			setSnapshot((event as CustomEvent<CompareSnapshot | null>).detail);
			setQuery("");
			setCandidate(null);
			setMessage("Select a session to see its laps.");
			setLoadingSession(false);
			setLoadingLap(false);
			window.setTimeout(() => search.current?.focus());
		};
		window.addEventListener("trace:compare-load", receive);
		return () => window.removeEventListener("trace:compare-load", receive);
	}, []);

	const sessions = useMemo(() => {
		if (!snapshot) return [];
		const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
		return [...snapshot.sessions]
			.filter((session) => terms.every((term) => searchText(session).includes(term)))
			.sort(
				(left, right) =>
					Number(summaryCompatible(snapshot.currentReview.summary, right)) - Number(summaryCompatible(snapshot.currentReview.summary, left)),
			);
	}, [query, snapshot]);

	if (!snapshot) return null;
	const compatible = candidate ? reviewsCompatible(snapshot.currentReview, candidate) : false;

	async function chooseSession(sessionId: string) {
		const sequence = ++requestSequence.current;
		setCandidate(null);
		setLoadingSession(true);
		setMessage("Loading laps…");
		try {
			const review = await request<Review>(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/review`);
			if (sequence !== requestSequence.current) return;
			setCandidate(review);
			setMessage(review.laps.length ? "" : "This session has no laps.");
		} catch (error) {
			if (sequence === requestSequence.current) setMessage(error instanceof Error ? error.message : "Could not load this session.");
		} finally {
			if (sequence === requestSequence.current) setLoadingSession(false);
		}
	}

	async function chooseLap(lap: Lap) {
		if (!candidate) return;
		const sequence = ++requestSequence.current;
		setLoadingLap(true);
		try {
			const telemetry = await request<unknown>(
				`/api/v1/library/sessions/${encodeURIComponent(candidate.summary.session_id)}/laps/${encodeURIComponent(lap.id)}/telemetry?max_points=1800`,
			);
			if (sequence !== requestSequence.current) return;
			emit("trace:comparison-create", { candidateSession: candidate, candidateLap: lap, candidateTelemetry: telemetry });
		} catch (error) {
			emit("trace:toast", { message: error instanceof Error ? error.message : "Could not load lap telemetry.", kind: "error" });
		} finally {
			if (sequence === requestSequence.current) setLoadingLap(false);
		}
	}

	function close() {
		requestSequence.current += 1;
		emit("trace:compare-close");
	}

	return (
		<section className="compare-picker">
			<div className="picker-head">
				<div>
					<p className="eyebrow">CHOOSE A LAP</p>
					<strong>Compare against your library</strong>
				</div>
				<button className="quiet" type="button" onClick={close}>
					Close
				</button>
			</div>
			<label className="picker-search">
				<span>Search sessions</span>
				<input ref={search} type="search" placeholder="Driver, car, track…" value={query} onChange={(event) => setQuery(event.target.value)} />
			</label>
			<div className="picker-layout">
				<div className="picker-sessions">
					{sessions.map((session) => (
						<button
							className={`picker-session ${candidate?.summary.session_id === session.session_id ? "active" : ""}`}
							key={session.session_id}
							type="button"
							onClick={() => void chooseSession(session.session_id)}
						>
							<span>
								<strong>{session.driver_name || session.title || "Recorded session"}</strong>
								<small>{summaryCompatible(snapshot.currentReview.summary, session) ? "MATCH" : session.simulator || "OTHER"}</small>
							</span>
							<span>
								{session.track || "Unknown track"} · {session.car || "Unknown car"}
							</span>
							<time>{formatSessionDate(session.started_at)}</time>
						</button>
					))}
					{sessions.length === 0 && <p>No matching sessions.</p>}
				</div>
				<div className={`picker-laps ${loadingLap ? "loading" : ""}`}>
					{candidate && (
						<div className={`picker-compatibility ${compatible ? "compatible" : "warning"}`}>
							{compatible ? "Compatible car and track" : "Different car, track, layout, or simulator"}
						</div>
					)}
					{candidate?.laps.map((lap) => {
						const sameLap = candidate.summary.session_id === snapshot.currentReview.summary.session_id && lap.id === snapshot.selectedLap.id;
						const unavailable = lap.duration_ns == null || lap.validity === "invalid" || (lap.max_tyres_out ?? 0) >= 3;
						return (
							<button
								className="picker-lap"
								disabled={sameLap || unavailable || !compatible || loadingLap}
								key={lap.id}
								type="button"
								onClick={() => void chooseLap(lap)}
							>
								<span>Lap {lap.index}</span>
								<strong>{formatDuration(lap.duration_ns)}</strong>
								<small>{sameLap ? "CURRENT" : unavailable ? "UNAVAILABLE" : `${lap.sectors.length} SECTORS`}</small>
							</button>
						);
					})}
					{(loadingSession || message) && <p>{message}</p>}
				</div>
			</div>
		</section>
	);
}

const root = document.querySelector("#compare-picker-root");
if (root) createRoot(root).render(<ComparePicker />);

export { ComparePicker };
