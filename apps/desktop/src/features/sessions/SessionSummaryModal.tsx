import { useEffect, useMemo } from "react";
import type { RecordedSessionSummary } from "../../data-source";
import { formatSessionDate, friendlySessionType, lapDuration, lapIsInvalid, theoreticalBestLap } from "./session-components";

export function SessionSummaryModal({
	session,
	sessions,
	onClose,
}: {
	session: RecordedSessionSummary;
	sessions: RecordedSessionSummary[];
	onClose: () => void;
}) {
	useEffect(() => {
		function closeOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape") onClose();
		}
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);

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
	const validLaps = session.laps.filter((lap) => lap.time !== "—" && !lapIsInvalid(lap));
	const bestLap = validLaps.slice().sort((left, right) => lapDuration(left) - lapDuration(right))[0];
	const sectorIndices = [...new Set(session.laps.flatMap((lap) => lap.sectors.map((sector) => sector.index)))].sort((left, right) => left - right);
	const theoreticalBest = theoreticalBestLap(session.laps, sectorIndices);
	const theoreticalBestNs = theoreticalBest == null ? null : parseLapTime(theoreticalBest);
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
		validLaps: validLaps.length,
		timeFound: bestDuration != null && previousDuration != null ? previousDuration - bestDuration : null,
		potentialNs: bestDuration != null && theoreticalBestNs != null ? Math.max(0, bestDuration - theoreticalBestNs) : null,
	};
}

function parseLapTime(value: string) {
	const match = /^(\d+):(\d{2})\.(\d{3})$/.exec(value);
	if (!match) return null;
	return (Number(match[1]) * 60_000 + Number(match[2]) * 1_000 + Number(match[3])) * 1_000_000;
}

function formatDelta(durationNs: number) {
	return `${(durationNs / 1_000_000_000).toFixed(3)}s`;
}
