import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { formatLapDurationNs, sessionOverview } from "@trace/ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { telemetryDataSource, type LibraryConnectionSettings, type LibraryUploadResult, type RecordedSessionSummary } from "../../data-source";
import { useToast } from "../../Toast";
import { formatSessionDate, friendlySessionType, lapDuration, lapIsInvalid } from "./session-components";
import { LapPackageDialog } from "./LapPackageDialog";

export function SessionSummaryModal({
	session,
	sessions,
	completed = false,
	onClose,
}: {
	session: RecordedSessionSummary;
	sessions: RecordedSessionSummary[];
	completed?: boolean;
	onClose: () => void;
}) {
	const showToast = useToast();
	const [library, setLibrary] = useState<LibraryConnectionSettings | null>(null);
	const [showUploadPicker, setShowUploadPicker] = useState(false);
	const [automaticUpload, setAutomaticUpload] = useState<"idle" | "uploading" | "uploaded" | "failed">("idle");
	const [automaticError, setAutomaticError] = useState<string | null>(null);
	const [uploadedResult, setUploadedResult] = useState<LibraryUploadResult | null>(null);
	const automaticStarted = useRef<string | null>(null);
	const validLapIndices = useMemo(() => session.laps.filter((lap) => lap.durationNs != null && !lapIsInvalid(lap)).map((lap) => lap.index), [session.laps]);

	useEffect(() => {
		function closeOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape" && !showUploadPicker) onClose();
		}
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose, showUploadPicker]);

	useEffect(() => {
		let active = true;
		void telemetryDataSource
			.getLibraryConnectionSettings()
			.then((settings) => active && setLibrary(settings))
			.catch(() => active && setLibrary(null));
		return () => {
			active = false;
		};
	}, []);

	const uploadAutomatically = useCallback(async () => {
		if (validLapIndices.length === 0) return;
		setAutomaticUpload("uploading");
		setAutomaticError(null);
		try {
			const result = await telemetryDataSource.uploadLibrarySession(session.id, validLapIndices);
			setUploadedResult(result);
			setAutomaticUpload("uploaded");
			showToast({
				kind: "success",
				title: "Session synced privately",
				message: `${result.lapCount} ${result.lapCount === 1 ? "lap" : "laps"} added to ${result.endpoint}.`,
				timeoutMs: 7_000,
				action: {
					label: "OPEN LIBRARY",
					onClick: () => {
						void openUrl(result.reviewUrl).catch((error) =>
							showToast({
								kind: "error",
								title: "Could not open private library",
								message: error instanceof Error ? error.message : String(error),
								timeoutMs: 8_000,
							}),
						);
					},
				},
			});
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			setAutomaticUpload("failed");
			setAutomaticError(message);
			showToast({
				kind: "error",
				title: "Private library sync queued",
				message: `${message} TRACE kept this upload for retry.`,
				timeoutMs: 9_000,
			});
		}
	}, [session.id, showToast, validLapIndices]);

	async function openUploadedSession() {
		if (!uploadedResult) return;
		try {
			await openUrl(uploadedResult.reviewUrl);
		} catch (error) {
			showToast({
				kind: "error",
				title: "Could not open private library",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 8_000,
			});
		}
	}

	useEffect(() => {
		if (!completed || !library?.tokenConfigured || library.syncMode !== "automatic") return;
		if (automaticStarted.current === session.id) return;
		automaticStarted.current = session.id;
		void uploadAutomatically();
	}, [completed, library, session.id, uploadAutomatically]);

	const summary = useMemo(() => sessionSummary(session, sessions), [session, sessions]);

	return (
		<div
			className="fixed inset-0 z-[100] grid place-items-center bg-black/75 p-8"
			role="presentation"
			onMouseDown={(event) => {
				if (event.target === event.currentTarget) onClose();
			}}
		>
			<section
				role="dialog"
				aria-modal="true"
				aria-labelledby="session-summary-title"
				className="w-full max-w-[700px] border border-trace-divider bg-trace-deep"
			>
				<header className="flex items-start justify-between gap-6 border-b border-trace-divider px-7 py-6">
					<div className="min-w-0">
						<span className="font-mono text-[11px] font-bold tracking-[.14em] text-trace-accent">SESSION SUMMARY</span>
						<h2 id="session-summary-title" className="mt-2 truncate text-xl font-black tracking-[-.02em]">
							{session.title ?? session.track}
						</h2>
						<p className="mt-2 text-[13px] leading-5 text-trace-muted">
							{session.car} · {friendlySessionType(session)} · {formatSessionDate(session.startedAt)}
						</p>
					</div>
					<button
						type="button"
						onClick={onClose}
						aria-label="Close session summary"
						className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-surface text-lg text-trace-muted hover:border-trace-soft hover:text-white"
					>
						×
					</button>
				</header>

				<div className="px-7 py-7">
					<div className="border-l-2 border-trace-purple bg-trace-purple-wash px-6 py-5">
						<span className="font-mono text-[11px] font-bold tracking-[.12em] text-trace-soft">BEST LAP</span>
						<strong className="mt-3 block font-mono text-[42px] font-black leading-none tracking-[-.05em] text-trace-purple">
							{summary.bestLap?.time ?? "—"}
						</strong>
						<p className="mt-3 text-[12px] leading-5 text-trace-muted">
							{summary.bestLap ? `Lap ${summary.bestLap.index} · quickest valid lap` : "No valid timed lap was recorded."}
						</p>
					</div>

					<div className="mt-4 grid grid-cols-3 border border-trace-divider bg-trace-surface">
						<SummaryMetric
							label={summary.timeFound == null || summary.timeFound >= 0 ? "TIME FOUND" : "OFF PREVIOUS"}
							value={summary.timeFound == null ? "—" : formatDelta(Math.abs(summary.timeFound))}
							detail={
								summary.previousBest == null
									? "No earlier matching session"
									: summary.timeFound != null && summary.timeFound >= 0
										? `Faster than ${summary.previousBest.time}`
										: `Previous best ${summary.previousBest.time}`
							}
							accent={summary.timeFound != null && summary.timeFound > 0}
							warning={summary.timeFound != null && summary.timeFound < 0}
						/>
						<SummaryMetric label="THEORETICAL BEST" value={summary.theoreticalBest ?? "—"} detail="Best valid sectors combined" purple />
						<SummaryMetric label="VALID LAPS" value={`${summary.validLaps} / ${session.laps.length}`} detail="Completed laps in this session" />
					</div>

					{summary.potentialNs != null && summary.potentialNs > 0 && (
						<p className="mt-4 border-l border-trace-divider pl-4 text-[12px] leading-5 text-trace-muted">
							Your best sectors contain <strong className="font-mono text-trace-text">{formatDelta(summary.potentialNs)}</strong> beyond the best
							completed lap.
						</p>
					)}

					{completed && library?.tokenConfigured && library.syncMode !== "never" && (
						<section className="mt-5 border-t border-trace-divider pt-5" aria-label="Private library sync">
							<div className="flex flex-wrap items-center justify-between gap-4">
								<div className="min-w-0">
									<span className="font-mono text-[10px] font-bold tracking-[.12em] text-trace-accent">PRIVATE LIBRARY</span>
									<strong className="mt-1 block text-[13px] text-trace-text">
										{library.syncMode === "automatic"
											? validLapIndices.length === 0
												? "No valid timed laps to sync"
												: automaticUpload === "uploaded"
													? "Session synced"
													: automaticUpload === "failed"
														? "Session queued for retry"
														: "Syncing completed session…"
											: "Keep this session in your private library?"}
									</strong>
									<p className="mt-1 max-w-lg text-[11px] leading-5 text-trace-dim">
										{automaticError ??
											`${validLapIndices.length} valid timed ${validLapIndices.length === 1 ? "lap" : "laps"} · ${library.endpoint}`}
									</p>
								</div>
								{library.syncMode === "ask" ? (
									<button
										type="button"
										disabled={session.laps.every((lap) => lap.durationNs == null)}
										onClick={() => setShowUploadPicker(true)}
										className="h-9 shrink-0 border border-trace-accent bg-trace-accent-wash px-4 font-mono text-[10px] font-black tracking-[.08em] text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
									>
										CHOOSE LAPS
									</button>
								) : automaticUpload === "failed" ? (
									<button
										type="button"
										onClick={() => void uploadAutomatically()}
										className="h-9 shrink-0 border border-trace-divider px-4 font-mono text-[10px] font-black tracking-[.08em] text-trace-soft hover:border-trace-soft hover:text-white"
									>
										RETRY
									</button>
								) : automaticUpload === "uploaded" ? (
									<button
										type="button"
										onClick={() => void openUploadedSession()}
										className="h-9 shrink-0 border border-trace-divider px-4 font-mono text-[10px] font-black tracking-[.08em] text-trace-accent hover:border-trace-accent hover:bg-trace-accent hover:text-trace-black"
									>
										OPEN LIBRARY
									</button>
								) : null}
							</div>
						</section>
					)}
				</div>

				<footer className="flex justify-end border-t border-trace-divider px-7 py-4">
					<button
						type="button"
						onClick={onClose}
						className="h-10 border border-trace-accent/60 bg-trace-accent-wash px-5 font-mono text-[11px] font-black tracking-[.1em] text-trace-accent hover:bg-trace-accent hover:text-trace-black"
					>
						VIEW SESSION
					</button>
				</footer>
			</section>
			{showUploadPicker && <LapPackageDialog session={session} action="upload" onClose={() => setShowUploadPicker(false)} />}
		</div>
	);
}

function SummaryMetric({
	label,
	value,
	detail,
	accent = false,
	purple = false,
	warning = false,
}: {
	label: string;
	value: string;
	detail: string;
	accent?: boolean;
	purple?: boolean;
	warning?: boolean;
}) {
	const colour = purple ? "text-trace-purple" : accent ? "text-trace-accent" : warning ? "text-trace-warning" : "text-trace-text";
	return (
		<div className="min-w-0 border-r border-trace-divider px-5 py-4 last:border-r-0">
			<span className="block truncate font-mono text-[10px] font-bold tracking-[.1em] text-trace-muted">{label}</span>
			<strong className={`mt-3 block truncate font-mono text-lg font-black ${colour}`}>{value}</strong>
			<span className="mt-2 block text-[11px] leading-4 text-trace-dim">{detail}</span>
		</div>
	);
}

function sessionSummary(session: RecordedSessionSummary, sessions: RecordedSessionSummary[]) {
	const overview = sessionOverview(
		session.laps.map((lap) => ({
			id: String(lap.index),
			index: lap.index,
			durationNs: Number.isFinite(lapDuration(lap)) ? lapDuration(lap) : null,
			invalid: lapIsInvalid(lap),
			sectors: lap.sectors.map((sector) => ({ index: sector.index, durationNs: sector.durationNs })),
		})),
	);
	const bestLap = session.laps.find((lap) => String(lap.index) === overview.bestLapId);
	const theoreticalBestNs = overview.theoreticalBestDurationNs;
	const theoreticalBest = theoreticalBestNs == null ? null : formatLapDurationNs(theoreticalBestNs);
	const previous = sessions
		.filter(
			(candidate) =>
				candidate.id !== session.id &&
				candidate.simulatorId === session.simulatorId &&
				candidate.track === session.track &&
				candidate.car === session.car &&
				new Date(candidate.startedAt).getTime() < new Date(session.startedAt).getTime(),
		)
		.sort((left, right) => new Date(right.startedAt).getTime() - new Date(left.startedAt).getTime())[0];
	const previousBest = previous?.laps
		.filter((lap) => lap.time !== "—" && !lapIsInvalid(lap))
		.sort((left, right) => lapDuration(left) - lapDuration(right))[0];
	const bestDuration = bestLap ? lapDuration(bestLap) : null;
	const previousDuration = previousBest ? lapDuration(previousBest) : null;

	return {
		bestLap,
		previousBest,
		theoreticalBest,
		validLaps: overview.validTimedLapCount,
		timeFound: bestDuration != null && previousDuration != null ? previousDuration - bestDuration : null,
		potentialNs: bestDuration != null && theoreticalBestNs != null ? Math.max(0, bestDuration - theoreticalBestNs) : null,
	};
}

function formatDelta(durationNs: number) {
	return `${(durationNs / 1_000_000_000).toFixed(3)}s`;
}
