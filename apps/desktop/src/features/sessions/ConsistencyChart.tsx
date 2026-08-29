import { useMemo, useState } from "react";
import type { RecordedLapSummary, RecordedSessionSummary } from "../../data-source";
import { formatLapDurationNs, lapIsInvalid } from "./session-components";

export interface ConsistencySeries {
	label: string;
	session: RecordedSessionSummary;
	colour?: "accent" | "purple";
}

interface TimedLap {
	lap: RecordedLapSummary;
	deviationSeconds: number;
	x: number;
	y: number;
}

interface ChartSeries extends ConsistencySeries {
	medianNs: number;
	standardDeviationSeconds: number;
	laps: TimedLap[];
}

const WIDTH = 1_000;
const HEIGHT = 220;
const LEFT = 58;
const RIGHT = 20;
const TOP = 18;
const BOTTOM = 34;

export function ConsistencyChart({ series }: { series: ConsistencySeries[] }) {
	const [hovered, setHovered] = useState<{ seriesIndex: number; lapIndex: number } | null>(null);
	const uniqueSeries = useMemo(
		() => series.filter((entry, index) => series.findIndex((candidate) => candidate.session.id === entry.session.id) === index),
		[series],
	);
	const chart = useMemo(() => buildChart(uniqueSeries), [uniqueSeries]);
	const validLapCount = chart.series.reduce((total, entry) => total + entry.laps.length, 0);
	const activeLap = hovered == null ? null : chart.series[hovered.seriesIndex]?.laps.find((lap) => lap.lap.index === hovered.lapIndex);
	const activeSeries = hovered == null ? null : chart.series[hovered.seriesIndex];

	return (
		<section className="border border-trace-divider bg-trace-surface" aria-labelledby="consistency-heading">
			<div className="flex flex-wrap items-start justify-between gap-4 border-b border-trace-divider px-5 py-4">
				<div>
					<h2 id="consistency-heading" className="text-[13px] font-black tracking-[.04em]">
						LAP CONSISTENCY
					</h2>
					<p className="mt-1 text-[12px] leading-5 text-trace-muted">
						Distance from each session&apos;s typical valid lap time. Flatter is more consistent.
					</p>
				</div>
				<div className="flex flex-wrap gap-x-7 gap-y-2">
					{chart.series.map((entry) => (
						<div key={entry.session.id} className="font-mono text-[11px] leading-5">
							<span className={entry.colour === "purple" ? "text-trace-purple" : "text-trace-accent"}>{entry.label.toUpperCase()}</span>
							<span className="ml-3 text-trace-dim">TYPICAL </span>
							<strong className="text-trace-text">{formatLapDurationNs(entry.medianNs)}</strong>
							<span className="ml-3 text-trace-dim">VARIATION </span>
							<strong className="text-trace-text">±{entry.standardDeviationSeconds.toFixed(2)}s</strong>
						</div>
					))}
				</div>
			</div>
			{validLapCount < 2 ? (
				<div className="px-5 py-8 text-center text-[12px] leading-5 text-trace-dim">
					At least two valid completed laps are needed to show consistency.
				</div>
			) : (
				<div className="relative px-3 py-3">
					<svg
						className="block h-[220px] w-full overflow-visible"
						viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
						role="img"
						aria-label="Lap time consistency graph"
					>
						{chart.ticks.map((tick) => (
							<g key={tick.value}>
								<line x1={LEFT} x2={WIDTH - RIGHT} y1={tick.y} y2={tick.y} stroke="var(--color-trace-divider)" strokeWidth="1" />
								<text x={LEFT - 10} y={tick.y + 4} textAnchor="end" fill="var(--color-trace-dim)" fontFamily="monospace" fontSize="11">
									{tick.value > 0 ? "+" : ""}
									{tick.value.toFixed(1)}s
								</text>
							</g>
						))}
						<line x1={LEFT} x2={WIDTH - RIGHT} y1={chart.zeroY} y2={chart.zeroY} stroke="var(--color-trace-soft)" strokeWidth="1.5" />
						{chart.series.map((entry, seriesIndex) => {
							const colour = entry.colour === "purple" ? "var(--color-trace-purple)" : "var(--color-trace-accent)";
							return (
								<g key={entry.session.id}>
									<polyline points={entry.laps.map((lap) => `${lap.x},${lap.y}`).join(" ")} fill="none" stroke={colour} strokeWidth="2.5" />
									{entry.laps.map((lap) => (
										<circle
											key={lap.lap.index}
											cx={lap.x}
											cy={lap.y}
											r={hovered?.seriesIndex === seriesIndex && hovered.lapIndex === lap.lap.index ? 6 : 4}
											fill={colour}
											stroke="var(--color-trace-black)"
											strokeWidth="2"
											tabIndex={0}
											onMouseEnter={() => setHovered({ seriesIndex, lapIndex: lap.lap.index })}
											onMouseLeave={() => setHovered(null)}
											onFocus={() => setHovered({ seriesIndex, lapIndex: lap.lap.index })}
											onBlur={() => setHovered(null)}
											aria-label={`${entry.label}, lap ${lap.lap.index}, ${lap.lap.time}, ${signedSeconds(lap.deviationSeconds)} from typical`}
										/>
									))}
								</g>
							);
						})}
						{chart.excludedLaps.map((lap) => (
							<g key={`${lap.sessionId}-${lap.index}`} aria-label={`Lap ${lap.index} excluded from consistency`}>
								<line
									x1={lap.x - 4}
									x2={lap.x + 4}
									y1={HEIGHT - BOTTOM - 4}
									y2={HEIGHT - BOTTOM + 4}
									stroke="var(--color-trace-danger)"
									strokeWidth="2"
								/>
								<line
									x1={lap.x - 4}
									x2={lap.x + 4}
									y1={HEIGHT - BOTTOM + 4}
									y2={HEIGHT - BOTTOM - 4}
									stroke="var(--color-trace-danger)"
									strokeWidth="2"
								/>
							</g>
						))}
						{chart.lapLabels.map((lap) => (
							<text
								key={lap.index}
								x={lap.x}
								y={HEIGHT - 8}
								textAnchor="middle"
								fill="var(--color-trace-dim)"
								fontFamily="monospace"
								fontSize="11"
							>
								{lap.index}
							</text>
						))}
					</svg>
					{activeLap && activeSeries && (
						<div
							className={`pointer-events-none absolute z-10 -translate-x-1/2 border px-3 py-2 font-mono text-[11px] shadow-lg ${activeSeries.colour === "purple" ? "border-trace-purple bg-trace-purple-wash" : "border-trace-accent bg-trace-accent-wash"}`}
							style={{ left: `${(activeLap.x / WIDTH) * 100}%`, top: `${Math.max(4, (activeLap.y / HEIGHT) * 100 - 8)}%` }}
						>
							<strong className="text-white">
								LAP {activeLap.lap.index} · {activeLap.lap.time}
							</strong>
							<span className="ml-3 text-trace-soft">{signedSeconds(activeLap.deviationSeconds)}</span>
						</div>
					)}
					<div className="mt-1 flex items-center justify-between px-12 font-mono text-[10px] text-trace-dim">
						<span>LAP NUMBER</span>
						<span>Invalid and incomplete laps are excluded</span>
					</div>
				</div>
			)}
		</section>
	);
}

function buildChart(series: ConsistencySeries[]) {
	const prepared = series
		.map((entry) => {
			const laps = entry.session.laps.filter((lap) => !lapIsInvalid(lap) && lap.durationNs != null && lap.durationNs > 0);
			const durations = laps.map((lap) => lap.durationNs as number).sort((left, right) => left - right);
			if (durations.length === 0) return null;
			const middle = Math.floor(durations.length / 2);
			const medianNs = durations.length % 2 === 0 ? (durations[middle - 1] + durations[middle]) / 2 : durations[middle];
			const deviations = laps.map((lap) => ((lap.durationNs as number) - medianNs) / 1_000_000_000);
			const standardDeviationSeconds = Math.sqrt(deviations.reduce((sum, value) => sum + value * value, 0) / deviations.length);
			return { ...entry, medianNs, standardDeviationSeconds, deviations, laps };
		})
		.filter((entry): entry is NonNullable<typeof entry> => entry != null);
	const allDeviations = prepared.flatMap((entry) => entry.deviations);
	const extent = Math.max(0.5, ...allDeviations.map(Math.abs));
	const roundedExtent = Math.ceil(extent * 2) / 2;
	const maxLapIndex = Math.max(1, ...series.flatMap((entry) => entry.session.laps.map((lap) => lap.index)));
	const minLapIndex = Math.min(1, ...series.flatMap((entry) => entry.session.laps.map((lap) => lap.index)));
	const xForLap = (index: number) => LEFT + ((index - minLapIndex) / Math.max(1, maxLapIndex - minLapIndex)) * (WIDTH - LEFT - RIGHT);
	const yForDeviation = (seconds: number) => TOP + ((roundedExtent - seconds) / (roundedExtent * 2)) * (HEIGHT - TOP - BOTTOM);
	const chartSeries: ChartSeries[] = prepared.map((entry) => ({
		...entry,
		laps: entry.laps.map((lap, index) => ({
			lap,
			deviationSeconds: entry.deviations[index],
			x: xForLap(lap.index),
			y: yForDeviation(entry.deviations[index]),
		})),
	}));
	const tickValues = [-roundedExtent, -roundedExtent / 2, 0, roundedExtent / 2, roundedExtent];
	const allLapIndices = [...new Set(series.flatMap((entry) => entry.session.laps.map((lap) => lap.index)))].sort((left, right) => left - right);
	const labelStep = Math.max(1, Math.ceil(allLapIndices.length / 12));
	return {
		series: chartSeries,
		zeroY: yForDeviation(0),
		excludedLaps: series.flatMap((entry) =>
			entry.session.laps
				.filter((lap) => lapIsInvalid(lap) || lap.durationNs == null || lap.durationNs <= 0)
				.map((lap) => ({ sessionId: entry.session.id, index: lap.index, x: xForLap(lap.index) })),
		),
		ticks: tickValues.map((value) => ({ value, y: yForDeviation(value) })),
		lapLabels: allLapIndices
			.filter((_, index) => index % labelStep === 0 || index === allLapIndices.length - 1)
			.map((index) => ({ index, x: xForLap(index) })),
	};
}

function signedSeconds(seconds: number) {
	if (Math.abs(seconds) < 0.005) return "TYPICAL";
	return `${seconds > 0 ? "+" : ""}${seconds.toFixed(2)}s`;
}
