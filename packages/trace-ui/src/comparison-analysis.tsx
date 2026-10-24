import { useState } from "react";
import type { LapComparisonSample } from "./telemetry-chart";

export interface ComparisonAnalysisSegment {
	id: string;
	label: string;
	sectorIndex: number | null;
	startM: number;
	endM: number;
	deltaSeconds: number | null;
	averageSpeedDifferenceKmh: number | null;
	averageThrottleDifferencePercent: number | null;
	averageBrakeDifferencePercent: number | null;
}

function averageDifference(
	samples: LapComparisonSample[],
	reference: (sample: LapComparisonSample) => number | null | undefined,
	comparison: (sample: LapComparisonSample) => number | null | undefined,
) {
	const differences = samples.flatMap((sample) => {
		const referenceValue = reference(sample);
		const comparisonValue = comparison(sample);
		return referenceValue == null || comparisonValue == null || !Number.isFinite(referenceValue) || !Number.isFinite(comparisonValue)
			? []
			: [comparisonValue - referenceValue];
	});
	return differences.length ? differences.reduce((total, value) => total + value, 0) / differences.length : null;
}

export function comparisonAnalysis(samples: LapComparisonSample[]): ComparisonAnalysisSegment[] {
	if (samples.length < 2) return [];
	const sectorIndices = [...new Set(samples.flatMap((sample) => (sample.sectorIndex == null ? [] : [sample.sectorIndex])))].sort((a, b) => a - b);
	const groups = sectorIndices.length
		? sectorIndices.map((index) => ({ label: `SECTOR ${index}`, sectorIndex: index, samples: samples.filter((sample) => sample.sectorIndex === index) }))
		: Array.from({ length: Math.min(4, samples.length) }, (_, index) => {
				const start = Math.floor((index / Math.min(4, samples.length)) * samples.length);
				const end = Math.max(start + 1, Math.floor(((index + 1) / Math.min(4, samples.length)) * samples.length));
				return { label: `LAP SECTION ${index + 1}`, sectorIndex: null, samples: samples.slice(start, end) };
			});
	let previousDelta = samples.find((sample) => sample.deltaSeconds != null)?.deltaSeconds ?? 0;
	return groups.flatMap((group, index) => {
		if (group.samples.length === 0) return [];
		const finalDelta =
			group.samples
				.slice()
				.reverse()
				.find((sample) => sample.deltaSeconds != null)?.deltaSeconds ?? null;
		const deltaSeconds = finalDelta == null ? null : finalDelta - previousDelta;
		if (finalDelta != null) previousDelta = finalDelta;
		return [
			{
				id: `${group.sectorIndex ?? "section"}-${index}`,
				label: group.label,
				sectorIndex: group.sectorIndex,
				startM: group.samples[0].distanceM,
				endM: group.samples.at(-1)?.distanceM ?? group.samples[0].distanceM,
				deltaSeconds,
				averageSpeedDifferenceKmh: averageDifference(
					group.samples,
					(sample) => sample.referenceSpeedKmh,
					(sample) => sample.comparisonSpeedKmh,
				),
				averageThrottleDifferencePercent: averageDifference(
					group.samples,
					(sample) => sample.referenceThrottlePercent,
					(sample) => sample.comparisonThrottlePercent,
				),
				averageBrakeDifferencePercent: averageDifference(
					group.samples,
					(sample) => sample.referenceBrakePercent,
					(sample) => sample.comparisonBrakePercent,
				),
			},
		];
	});
}

export function ComparisonAnalysisPanel({
	segments,
	selectedId,
	onSelect,
}: {
	segments: ComparisonAnalysisSegment[];
	selectedId: string | null;
	onSelect: (segment: ComparisonAnalysisSegment | null) => void;
}) {
	const [tab, setTab] = useState<"insights" | "breakdown">("insights");
	const losses = segments
		.filter((segment) => segment.deltaSeconds != null && segment.deltaSeconds > 0.005)
		.sort((left, right) => (right.deltaSeconds ?? 0) - (left.deltaSeconds ?? 0));
	const visible = tab === "insights" ? losses.slice(0, 4) : segments;
	return (
		<aside className="min-w-0 border border-trace-divider bg-trace-surface">
			<header className="border-b border-trace-divider px-4 py-3">
				<strong className="block font-mono text-[11px] tracking-[.1em] text-trace-soft">ANALYSIS</strong>
				<span className="mt-1 block text-[10px] leading-4 text-trace-dim">Rule-based telemetry comparison · no AI</span>
			</header>
			<div className="grid grid-cols-2 border-b border-trace-divider">
				{(["insights", "breakdown"] as const).map((item) => (
					<button
						type="button"
						onClick={() => setTab(item)}
						className={`h-9 border-r border-trace-divider font-mono text-[10px] font-black tracking-[.08em] last:border-r-0 ${tab === item ? "bg-trace-accent-wash text-trace-accent" : "bg-trace-deep text-trace-muted hover:text-trace-text"}`}
						key={item}
					>
						{item === "insights" ? `INSIGHTS ${losses.length}` : `BREAKDOWN ${segments.length}`}
					</button>
				))}
			</div>
			<div className="max-h-[520px] overflow-y-auto">
				{visible.length === 0 ? (
					<p className="px-4 py-5 text-[12px] leading-5 text-trace-muted">No meaningful time-loss pattern was detected in this view.</p>
				) : (
					visible.map((segment) => {
						const selected = selectedId === segment.id;
						return (
							<button
								type="button"
								onClick={() => onSelect(selected ? null : segment)}
								className={`block w-full border-b border-trace-divider px-4 py-3 text-left last:border-b-0 ${selected ? "bg-trace-accent-wash" : "hover:bg-trace-deep"}`}
								key={segment.id}
							>
								<span className="flex items-center justify-between gap-3 font-mono">
									<strong className="text-[10px] tracking-[.08em] text-trace-soft">{segment.label}</strong>
									<strong
										className={
											segment.deltaSeconds != null && segment.deltaSeconds > 0.0005
												? "text-red-300"
												: segment.deltaSeconds != null && segment.deltaSeconds < -0.0005
													? "text-trace-accent"
													: "text-trace-dim"
										}
									>
										{formatDelta(segment.deltaSeconds)}
									</strong>
								</span>
								<span className="mt-2 block text-[11px] leading-4 text-trace-muted">{segmentDetail(segment)}</span>
							</button>
						);
					})
				)}
			</div>
			<p className="border-t border-trace-divider px-4 py-3 text-[10px] leading-4 text-trace-dim">
				Analysis is a naive comparison of recorded channels and may be incomplete or incorrect.
			</p>
		</aside>
	);
}

function formatDelta(value: number | null) {
	if (value == null) return "—";
	if (Math.abs(value) < 0.0005) return "0.000";
	return `${value > 0 ? "+" : "−"}${Math.abs(value).toFixed(3)}`;
}

function segmentDetail(segment: ComparisonAnalysisSegment) {
	const details: string[] = [];
	if (segment.averageSpeedDifferenceKmh != null && Math.abs(segment.averageSpeedDifferenceKmh) >= 1)
		details.push(
			`${Math.abs(segment.averageSpeedDifferenceKmh).toFixed(0)} km/h ${segment.averageSpeedDifferenceKmh < 0 ? "lower" : "higher"} average speed`,
		);
	if (segment.averageThrottleDifferencePercent != null && Math.abs(segment.averageThrottleDifferencePercent) >= 2)
		details.push(
			`${Math.abs(segment.averageThrottleDifferencePercent).toFixed(0)}% ${segment.averageThrottleDifferencePercent < 0 ? "less" : "more"} throttle`,
		);
	if (segment.averageBrakeDifferencePercent != null && Math.abs(segment.averageBrakeDifferencePercent) >= 2)
		details.push(`${Math.abs(segment.averageBrakeDifferencePercent).toFixed(0)}% ${segment.averageBrakeDifferencePercent < 0 ? "less" : "more"} brake`);
	return details.slice(0, 2).join(" · ") || "Inputs were broadly similar in the recorded samples.";
}
