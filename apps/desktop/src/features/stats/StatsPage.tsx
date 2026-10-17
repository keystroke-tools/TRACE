import { useEffect, useMemo, useState } from "react";
import { PageIntro, SectionHeading } from "../../components/layout";
import { telemetryDataSource, type RecordedSessionSummary } from "../../data-source";
import { formatCompactSessionDate, formatLapDurationNs } from "../sessions/session-components";
import { calculateDrivingStats, type ComboSessionPoint, type RankedDrivingItem } from "./stats";

function formatDrivingTime(durationNs: number) {
	const minutes = Math.round(durationNs / 60_000_000_000);
	if (minutes < 60) return `${minutes} min`;
	const hours = Math.floor(minutes / 60);
	const remainder = minutes % 60;
	return remainder === 0 ? `${hours} hr` : `${hours} hr ${remainder} min`;
}

function Ranking({ title, items }: { title: string; items: RankedDrivingItem[] }) {
	const maximum = Math.max(1, ...items.map((item) => item.lapCount));
	return (
		<div className="min-w-0 border border-trace-divider bg-trace-surface">
			<div className="border-b border-trace-divider px-4 py-3 text-[12px] font-extrabold tracking-[.12em] text-trace-soft">{title}</div>
			<div className="divide-y divide-trace-divider">
				{items.slice(0, 5).map((item, index) => (
					<div key={item.key} className="relative min-w-0 overflow-hidden px-4 py-3">
						<div className="absolute inset-y-0 left-0 bg-trace-accent-wash" style={{ width: `${Math.max(3, (item.lapCount / maximum) * 100)}%` }} />
						<div className="relative flex min-w-0 items-center gap-3">
							<span className="w-5 shrink-0 font-mono text-[11px] text-trace-dim">{String(index + 1).padStart(2, "0")}</span>
							<div className="min-w-0 flex-1">
								<div className="truncate text-[13px] font-bold text-trace-text">{item.label}</div>
								{item.detail && <div className="mt-0.5 truncate text-[11px] text-trace-muted">{item.detail}</div>}
							</div>
							<div className="shrink-0 text-right font-mono text-[12px] text-trace-soft">
								{item.lapCount} <span className="text-trace-dim">LAPS</span>
							</div>
						</div>
					</div>
				))}
				{items.length === 0 && <div className="px-4 py-8 text-center text-[13px] text-trace-muted">No recorded driving yet.</div>}
			</div>
		</div>
	);
}

function Trend({ points }: { points: ComboSessionPoint[] }) {
	if (points.length < 2) return <span className="text-trace-dim">ONE SESSION</span>;
	const values = points.map((point) => point.bestLapNs);
	const minimum = Math.min(...values);
	const maximum = Math.max(...values);
	const range = Math.max(1, maximum - minimum);
	const coordinates = points
		.map((point, index) => {
			const x = points.length === 1 ? 0 : (index / (points.length - 1)) * 100;
			const y = 5 + ((point.bestLapNs - minimum) / range) * 30;
			return `${x},${y}`;
		})
		.join(" ");
	return (
		<svg viewBox="0 0 100 40" preserveAspectRatio="none" className="h-10 w-28" aria-label={`${points.length} session lap-time trend`}>
			<polyline points={coordinates} fill="none" stroke="currentColor" strokeWidth="2" vectorEffect="non-scaling-stroke" className="text-trace-purple" />
		</svg>
	);
}

export function StatsPage({ onOpenSession }: { onOpenSession: (sessionId: string) => void }) {
	const [sessions, setSessions] = useState<RecordedSessionSummary[] | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		let active = true;
		void telemetryDataSource
			.getStatsSessions()
			.then((next) => {
				if (active) setSessions(next);
			})
			.catch((reason) => {
				if (active) setError(reason instanceof Error ? reason.message : String(reason));
			});
		return () => {
			active = false;
		};
	}, []);

	const stats = useMemo(() => calculateDrivingStats(sessions ?? []), [sessions]);
	const improving = stats.combinations
		.filter((combination) => combination.points.length > 1)
		.sort((left, right) => right.points.length - left.points.length || right.lapCount - left.lapCount);

	return (
		<div className="mx-auto w-full max-w-[1500px] pb-16">
			<PageIntro
				index="03"
				eyebrow="DRIVING STATS"
				title="Your time behind the wheel"
				description="Explore where you drive most, your quickest car and track combinations, and how your pace changes from one session to the next."
			/>

			{error && <div className="mt-6 border border-trace-danger bg-[#2a1715] px-4 py-3 text-[13px] text-[#ffb4aa]">Could not load stats: {error}</div>}
			{sessions == null && !error && <div className="mt-8 text-[13px] text-trace-muted">Calculating your driving history…</div>}

			{sessions != null && (
				<>
					<div className="mt-7 grid grid-cols-4 border border-trace-divider bg-trace-surface max-[1050px]:grid-cols-2">
						{[
							["SESSIONS", String(stats.sessionCount)],
							["TIMED LAPS", String(stats.lapCount)],
							["DRIVING TIME", formatDrivingTime(stats.drivingTimeNs)],
							["CAR / TRACK COMBOS", String(stats.combinationCount)],
						].map(([label, value]) => (
							<div
								key={label}
								className="min-h-24 border-r border-trace-divider p-4 last:border-r-0 max-[1050px]:even:border-r-0 max-[1050px]:[&:nth-child(-n+2)]:border-b"
							>
								<div className="text-[11px] font-extrabold tracking-[.12em] text-trace-muted">{label}</div>
								<div className="mt-4 font-mono text-xl font-bold text-trace-accent">{value}</div>
							</div>
						))}
					</div>

					<div className="mt-9">
						<SectionHeading index="01">MOST DRIVEN</SectionHeading>
						<div className="mt-3 grid grid-cols-3 gap-4 max-[1050px]:grid-cols-1">
							<Ranking title="SIMULATORS" items={stats.simulators} />
							<Ranking title="CARS" items={stats.cars} />
							<Ranking title="TRACKS" items={stats.tracks} />
						</div>
					</div>

					<div className="mt-9">
						<SectionHeading index="02">FASTEST LAPS BY CAR / TRACK</SectionHeading>
						<div className="mt-3 overflow-hidden border border-trace-divider bg-trace-surface">
							<div className="grid grid-cols-[minmax(180px,1.1fr)_minmax(170px,1fr)_120px_150px_110px] gap-5 border-b border-trace-divider px-4 py-3 text-[11px] font-extrabold tracking-[.1em] text-trace-dim">
								<span>CAR / SIM</span>
								<span>TRACK</span>
								<span>BEST LAP</span>
								<span>SET</span>
								<span className="text-right">HISTORY</span>
							</div>
							<div className="divide-y divide-trace-divider">
								{stats.combinations.map((combination) => (
									<button
										key={combination.key}
										type="button"
										onClick={() => onOpenSession(combination.bestSessionId)}
										className="grid w-full grid-cols-[minmax(180px,1.1fr)_minmax(170px,1fr)_120px_150px_110px] items-center gap-5 px-4 py-3 text-left hover:bg-trace-raised"
									>
										<span className="min-w-0">
											<strong className="block truncate text-[13px]">{combination.car}</strong>
											<span className="mt-0.5 block truncate text-[11px] text-trace-muted">{combination.simulatorName}</span>
										</span>
										<span className="truncate text-[13px] text-trace-soft">{combination.track}</span>
										<strong className="font-mono text-[14px] text-trace-purple">{formatLapDurationNs(combination.bestLapNs)}</strong>
										<span className="text-[11px] text-trace-muted">{formatCompactSessionDate(combination.bestStartedAt)}</span>
										<span className="text-right font-mono text-[11px] text-trace-muted">
											{combination.sessionCount} SESS · {combination.lapCount} LAPS
										</span>
									</button>
								))}
								{stats.combinations.length === 0 && (
									<div className="px-4 py-10 text-center text-[13px] text-trace-muted">
										Complete a timed lap to build your personal best table.
									</div>
								)}
							</div>
						</div>
					</div>

					<div className="mt-9">
						<SectionHeading index="03">SESSION-TO-SESSION PACE</SectionHeading>
						<div className="mt-3 overflow-hidden border border-trace-divider bg-trace-surface">
							{improving.map((combination) => {
								const first = combination.points[0];
								const latest = combination.points.at(-1) as ComboSessionPoint;
								const deltaNs = first.bestLapNs - latest.bestLapNs;
								const improved = deltaNs > 0;
								return (
									<button
										key={combination.key}
										type="button"
										onClick={() => onOpenSession(latest.sessionId)}
										className="grid w-full grid-cols-[minmax(230px,1fr)_130px_24px_130px_110px_130px] items-center gap-4 border-b border-trace-divider px-4 py-3 text-left last:border-b-0 hover:bg-trace-raised"
									>
										<span className="min-w-0">
											<strong className="block truncate text-[13px]">
												{combination.car} · {combination.track}
											</strong>
											<span className="mt-0.5 block text-[11px] text-trace-muted">
												{combination.simulatorName} · {combination.points.length} sessions
											</span>
										</span>
										<span className="font-mono text-[13px] text-trace-soft">{formatLapDurationNs(first.bestLapNs)}</span>
										<span className="text-trace-dim">→</span>
										<span className="font-mono text-[13px] text-trace-text">{formatLapDurationNs(latest.bestLapNs)}</span>
										<strong
											className={`font-mono text-[13px] ${improved ? "text-trace-accent" : deltaNs < 0 ? "text-trace-warning" : "text-trace-muted"}`}
										>
											{deltaNs === 0 ? "EVEN" : `${improved ? "−" : "+"}${(Math.abs(deltaNs) / 1_000_000_000).toFixed(3)}s`}
										</strong>
										<Trend points={combination.points} />
									</button>
								);
							})}
							{improving.length === 0 && (
								<div className="px-4 py-10 text-center text-[13px] text-trace-muted">
									Drive the same car and track in another session to see your pace trend.
								</div>
							)}
						</div>
					</div>
				</>
			)}
		</div>
	);
}
