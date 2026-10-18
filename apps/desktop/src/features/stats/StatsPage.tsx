import { useEffect, useMemo, useState } from "react";
import { PageIntro, SectionHeading } from "../../components/layout";
import { telemetryDataSource, type RecordedSessionSummary } from "../../data-source";
import { formatCompactSessionDate, formatLapDurationNs, formatSessionType } from "../sessions/session-components";
import { calculateDrivingStats, filterStatsSessions, type ComboSessionPoint, type RankedDrivingItem, type StatsTimeRange } from "./stats";

function formatDrivingTime(durationNs: number) {
	const minutes = Math.round(durationNs / 60_000_000_000);
	if (minutes < 60) return `${minutes} min`;
	const hours = Math.floor(minutes / 60);
	const remainder = minutes % 60;
	return remainder === 0 ? `${hours} hr` : `${hours} hr ${remainder} min`;
}

function Ranking({
	title,
	items,
	selectedKey,
	onSelect,
}: {
	title: string;
	items: RankedDrivingItem[];
	selectedKey?: string;
	onSelect: (item: RankedDrivingItem) => void;
}) {
	const maximum = Math.max(1, ...items.map((item) => item.lapCount));
	return (
		<div className="min-w-0 border border-trace-divider bg-trace-surface">
			<div className="border-b border-trace-divider px-4 py-3 text-[12px] font-extrabold tracking-[.12em] text-trace-soft">{title}</div>
			<div className="divide-y divide-trace-divider">
				{items.slice(0, 5).map((item, index) => (
					<button
						key={item.key}
						type="button"
						onClick={() => onSelect(item)}
						className={`relative block w-full min-w-0 overflow-hidden px-4 py-3 text-left ${selectedKey === item.key ? "bg-trace-raised" : "hover:bg-trace-raised"}`}
					>
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
					</button>
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
	const dots = points.map((point, index) => ({
		x: points.length === 1 ? 0 : (index / (points.length - 1)) * 100,
		y: 5 + ((point.bestLapNs - minimum) / range) * 30,
	}));
	return (
		<svg viewBox="0 0 100 40" preserveAspectRatio="none" className="h-10 w-32" aria-label={`${points.length} session lap-time trend`}>
			<line x1="0" y1="20" x2="100" y2="20" stroke="currentColor" strokeWidth="1" vectorEffect="non-scaling-stroke" className="text-trace-divider" />
			<polyline points={coordinates} fill="none" stroke="currentColor" strokeWidth="2" vectorEffect="non-scaling-stroke" className="text-trace-purple" />
			{dots.map((dot, index) => (
				<circle
					key={`${dot.x}-${dot.y}`}
					cx={dot.x}
					cy={dot.y}
					r={index === dots.length - 1 ? 2.8 : 2}
					fill="currentColor"
					className="text-trace-purple"
				/>
			))}
		</svg>
	);
}

type RankingFocus = { kind: "car" | "track"; key: string; label: string };

export function StatsPage({ onOpenSession }: { onOpenSession: (sessionId: string) => void }) {
	const [sessions, setSessions] = useState<RecordedSessionSummary[] | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [simulatorId, setSimulatorId] = useState("");
	const [sessionType, setSessionType] = useState("");
	const [timeRange, setTimeRange] = useState<StatsTimeRange>("all");
	const [focus, setFocus] = useState<RankingFocus | null>(null);

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

	const allStats = useMemo(() => calculateDrivingStats(sessions ?? []), [sessions]);
	const sessionTypes = useMemo(() => [...new Set(filterStatsSessions(sessions ?? [], {}).map((session) => session.sessionType))].sort(), [sessions]);
	const filteredSessions = useMemo(
		() =>
			filterStatsSessions(sessions ?? [], {
				simulatorId: simulatorId || undefined,
				sessionType: sessionType || undefined,
				timeRange,
				focusKind: focus?.kind,
				focusKey: focus?.key,
			}),
		[focus, sessionType, sessions, simulatorId, timeRange],
	);
	const stats = useMemo(() => calculateDrivingStats(filteredSessions), [filteredSessions]);
	const activeFilterCount = Number(Boolean(simulatorId)) + Number(Boolean(sessionType)) + Number(timeRange !== "all") + Number(focus != null);
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
					<div className="mt-7 border border-trace-divider bg-trace-surface">
						<div className="grid grid-cols-[minmax(180px,1fr)_minmax(180px,1fr)_minmax(170px,1fr)_auto] items-end gap-4 p-4">
							<label className="min-w-0">
								<span className="mb-2 block text-[11px] font-extrabold tracking-[.1em] text-trace-muted">SIMULATOR</span>
								<select
									value={simulatorId}
									onChange={(event) => {
										setSimulatorId(event.target.value);
										setFocus(null);
									}}
									className="trace-select h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[13px] text-trace-text outline-none focus:border-trace-accent"
								>
									<option value="">All simulators</option>
									{allStats.simulators.map((simulator) => (
										<option key={simulator.key} value={simulator.key}>
											{simulator.label}
										</option>
									))}
								</select>
							</label>
							<label className="min-w-0">
								<span className="mb-2 block text-[11px] font-extrabold tracking-[.1em] text-trace-muted">SESSION TYPE</span>
								<select
									value={sessionType}
									onChange={(event) => setSessionType(event.target.value)}
									className="trace-select h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[13px] text-trace-text outline-none focus:border-trace-accent"
								>
									<option value="">All session types</option>
									{sessionTypes.map((type) => (
										<option key={type} value={type}>
											{formatSessionType(type)}
										</option>
									))}
								</select>
							</label>
							<label className="min-w-0">
								<span className="mb-2 block text-[11px] font-extrabold tracking-[.1em] text-trace-muted">TIME RANGE</span>
								<select
									value={timeRange}
									onChange={(event) => setTimeRange(event.target.value as StatsTimeRange)}
									className="trace-select h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[13px] text-trace-text outline-none focus:border-trace-accent"
								>
									<option value="all">All time</option>
									<option value="30">Last 30 days</option>
									<option value="90">Last 90 days</option>
									<option value="365">Last year</option>
								</select>
							</label>
							<button
								type="button"
								disabled={activeFilterCount === 0}
								onClick={() => {
									setSimulatorId("");
									setSessionType("");
									setTimeRange("all");
									setFocus(null);
								}}
								className="h-10 border border-trace-divider px-4 text-[11px] font-extrabold tracking-[.08em] text-trace-soft hover:border-trace-muted hover:text-white disabled:cursor-default disabled:opacity-35"
							>
								CLEAR {activeFilterCount > 0 ? `· ${activeFilterCount}` : ""}
							</button>
						</div>
						{focus && (
							<div className="flex items-center gap-3 border-t border-trace-divider px-4 py-2.5 text-[12px]">
								<span className="text-trace-muted">Showing {focus.kind}</span>
								<strong className="text-trace-text">{focus.label}</strong>
								<button type="button" onClick={() => setFocus(null)} className="ml-auto px-2 py-1 font-bold text-trace-muted hover:text-white">
									REMOVE
								</button>
							</div>
						)}
					</div>

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
							<Ranking
								title="SIMULATORS"
								items={stats.simulators}
								selectedKey={simulatorId}
								onSelect={(item) => {
									setSimulatorId(item.key);
									setFocus(null);
								}}
							/>
							<Ranking
								title="CARS"
								items={stats.cars}
								selectedKey={focus?.kind === "car" ? focus.key : undefined}
								onSelect={(item) => {
									setSimulatorId(item.key.split("\u0000")[0]);
									setFocus({ kind: "car", key: item.key, label: item.label });
								}}
							/>
							<Ranking
								title="TRACKS"
								items={stats.tracks}
								selectedKey={focus?.kind === "track" ? focus.key : undefined}
								onSelect={(item) => {
									setSimulatorId(item.key.split("\u0000")[0]);
									setFocus({ kind: "track", key: item.key, label: item.label });
								}}
							/>
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
							<div className="grid grid-cols-[minmax(230px,1fr)_110px_110px_105px_110px_140px] items-center gap-4 border-b border-trace-divider px-4 py-3 text-[11px] font-extrabold tracking-[.1em] text-trace-dim">
								<span>CAR / TRACK</span>
								<span>FIRST</span>
								<span>LATEST</span>
								<span>CHANGE</span>
								<span>PB</span>
								<span>TREND</span>
							</div>
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
										className="grid w-full grid-cols-[minmax(230px,1fr)_110px_110px_105px_110px_140px] items-center gap-4 border-b border-trace-divider px-4 py-3 text-left last:border-b-0 hover:bg-trace-raised"
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
										<span className="font-mono text-[13px] text-trace-text">{formatLapDurationNs(latest.bestLapNs)}</span>
										<strong
											className={`font-mono text-[13px] ${improved ? "text-trace-accent" : deltaNs < 0 ? "text-trace-warning" : "text-trace-muted"}`}
										>
											{deltaNs === 0 ? "EVEN" : `${improved ? "−" : "+"}${(Math.abs(deltaNs) / 1_000_000_000).toFixed(3)}s`}
										</strong>
										<strong className="font-mono text-[13px] text-trace-purple">{formatLapDurationNs(combination.bestLapNs)}</strong>
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
