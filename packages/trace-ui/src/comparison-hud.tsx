import type { ReactNode } from "react";
import { channelColours, formatGear, type LapComparisonSample } from "./telemetry-chart";
import { HudConditions, HudPedals, HudSteering, HudValue, TelemetrySeek, hasHudConditions } from "./lap-telemetry-hud";

export type ComparisonSectorDelta = { index: number; seconds: number | null };
export type ComparisonSectorTiming = { index: number; durationNs: number | null };

export function comparisonSectorDeltas(
	referenceSectors: ComparisonSectorTiming[],
	comparisonSectors: ComparisonSectorTiming[],
	samples: LapComparisonSample[],
): ComparisonSectorDelta[] {
	const indices = [
		...new Set([
			...referenceSectors.map((sector) => sector.index),
			...comparisonSectors.map((sector) => sector.index),
			...samples.flatMap((sample) => (sample.sectorIndex == null ? [] : [sample.sectorIndex])),
		]),
	].sort((left, right) => left - right);
	const cumulativeDeltas = new Map(
		indices.map((index) => [
			index,
			samples
				.slice()
				.reverse()
				.find((sample) => sample.sectorIndex === index && sample.deltaSeconds != null)?.deltaSeconds ?? null,
		]),
	);
	return indices.map((index, position) => {
		const reference = referenceSectors.find((sector) => sector.index === index)?.durationNs;
		const comparison = comparisonSectors.find((sector) => sector.index === index)?.durationNs;
		if (reference != null && comparison != null) return { index, seconds: (comparison - reference) / 1_000_000_000 };
		const cumulativeDelta = cumulativeDeltas.get(index) ?? null;
		const previousDelta = position === 0 ? 0 : (cumulativeDeltas.get(indices[position - 1]) ?? null);
		return { index, seconds: cumulativeDelta == null || previousDelta == null ? null : cumulativeDelta - previousDelta };
	});
}

export interface ComparisonTelemetryHudProps {
	samples: LapComparisonSample[];
	cursorIndex: number | null;
	onSeek: (index: number) => void;
	referenceLabel: string;
	comparisonLabel: string;
	trackCarLabel: string;
	sectorDeltas: ComparisonSectorDelta[];
	sector: number | null;
	onSector: (sector: number | null) => void;
	comparisonIsFaster?: boolean;
	referenceChoice?: ReactNode;
	comparisonChoice?: ReactNode;
	onSwap?: () => void;
	onClose?: () => void;
	layout?: "fixed" | "embedded";
	distanceUnit?: string;
	playing?: boolean;
	onTogglePlayback?: () => void;
	playbackSpeed?: number;
	onPlaybackSpeed?: (speed: number) => void;
	referenceAirTemperatureC?: number | null;
	referenceTrackTemperatureC?: number | null;
	comparisonAirTemperatureC?: number | null;
	comparisonTrackTemperatureC?: number | null;
}

export function ComparisonTelemetryHud({
	samples,
	cursorIndex,
	onSeek,
	referenceLabel,
	comparisonLabel,
	trackCarLabel,
	sectorDeltas,
	sector,
	onSector,
	comparisonIsFaster = false,
	referenceChoice,
	comparisonChoice,
	onSwap,
	onClose,
	layout = "fixed",
	distanceUnit = "M",
	playing,
	onTogglePlayback,
	playbackSpeed,
	onPlaybackSpeed,
	referenceAirTemperatureC,
	referenceTrackTemperatureC,
	comparisonAirTemperatureC,
	comparisonTrackTemperatureC,
}: ComparisonTelemetryHudProps) {
	const sample = samples[cursorIndex ?? 0] ?? null;
	const referenceHasConditions = hasHudConditions(referenceAirTemperatureC, referenceTrackTemperatureC);
	const comparisonHasConditions = hasHudConditions(comparisonAirTemperatureC, comparisonTrackTemperatureC);
	const showConditions = referenceHasConditions || comparisonHasConditions;
	const frame =
		layout === "fixed"
			? `fixed bottom-12 left-[calc(var(--trace-sidebar)+24px)] right-6 z-30 grid h-[204px] ${showConditions ? "grid-cols-[120px_72px_minmax(160px,260px)_112px_minmax(120px,1fr)_112px_minmax(160px,260px)_72px_120px]" : "grid-cols-[120px_72px_minmax(180px,320px)_minmax(120px,1fr)_minmax(180px,320px)_72px_120px]"} grid-rows-[40px_45px_44px_27px] items-center justify-center gap-x-3 gap-y-2 overflow-hidden border border-trace-divider bg-trace-black/95 px-5 py-3 shadow-[0_12px_40px_rgba(0,0,0,.55)] backdrop-blur`
			: "grid min-w-0 grid-cols-[repeat(auto-fit,minmax(110px,1fr))] items-center gap-x-3 gap-y-3 overflow-hidden border border-trace-divider bg-trace-black px-4 py-3";
	const gapTone =
		sample?.deltaSeconds == null || Math.abs(sample.deltaSeconds) < 0.0005
			? "text-trace-text"
			: sample.deltaSeconds > 0
				? "text-[#ff5263]"
				: "text-[#42db76]";
	const referenceColour = comparisonIsFaster ? "var(--trace-accent)" : channelColours.faster;
	const comparisonColour = comparisonIsFaster ? channelColours.faster : "var(--trace-accent)";
	return (
		<div className={frame}>
			<div className="col-span-full min-w-0 border-b border-trace-divider pb-2">
				<TelemetrySeek
					samples={samples}
					cursorIndex={cursorIndex}
					onSeek={onSeek}
					distanceUnit={distanceUnit}
					playing={playing}
					onTogglePlayback={onTogglePlayback}
					playbackSpeed={playbackSpeed}
					onPlaybackSpeed={onPlaybackSpeed}
				/>
			</div>
			<div className="col-span-full grid grid-cols-[1fr_52px_1fr] items-center gap-3 border-b border-trace-divider pb-2">
				{referenceChoice ?? (
					<HudIdentity label={referenceLabel} role={comparisonIsFaster ? "SLOWER BASELINE" : "FASTER REFERENCE"} colour={referenceColour} />
				)}
				<div className="flex min-w-0 items-center justify-center">
					<button
						type="button"
						disabled={!onSwap}
						onClick={onSwap}
						className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-deep text-trace-muted hover:border-trace-soft hover:text-trace-text disabled:text-trace-dim"
						aria-label="Swap Reference and Analysed Lap"
					>
						<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
							<path d="M3 5h9m0 0L9.5 2.5M12 5 9.5 7.5M13 11H4m0 0 2.5-2.5M4 11l2.5 2.5" />
						</svg>
					</button>
				</div>
				{comparisonChoice ?? (
					<HudIdentity label={comparisonLabel} role={comparisonIsFaster ? "FASTER SELECTED LAP" : "LAP TO IMPROVE"} colour={comparisonColour} />
				)}
			</div>
			<HudValue
				label="REFERENCE SPEED / GEAR"
				value={sample?.referenceSpeedKmh == null ? "—" : `${Math.round(sample.referenceSpeedKmh)} · ${formatGear(sample.referenceGear)}`}
				colour={referenceColour}
			/>
			<HudSteering value={sample?.referenceSteeringDegrees} colour={referenceColour} />
			<HudPedals throttle={sample?.referenceThrottlePercent} brake={sample?.referenceBrakePercent} />
			{showConditions &&
				(referenceHasConditions ? <HudConditions air={referenceAirTemperatureC} track={referenceTrackTemperatureC} /> : <div aria-hidden="true" />)}
			<div className="h-10 min-w-0 overflow-hidden border-x border-trace-divider px-3 text-center font-mono">
				<span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">LIVE GAP</span>
				<strong className={`mt-1 block truncate text-[13px] leading-5 tabular-nums ${gapTone}`}>
					{sample == null ? "—" : `${Math.round(sample.distanceM)} ${distanceUnit} · ${formatComparisonGap(sample.deltaSeconds)}`}
				</strong>
			</div>
			{showConditions &&
				(comparisonHasConditions ? <HudConditions air={comparisonAirTemperatureC} track={comparisonTrackTemperatureC} /> : <div aria-hidden="true" />)}
			<HudPedals throttle={sample?.comparisonThrottlePercent} brake={sample?.comparisonBrakePercent} />
			<HudSteering value={sample?.comparisonSteeringDegrees} colour={comparisonColour} />
			<HudValue
				label="ANALYSED SPEED / GEAR"
				value={sample?.comparisonSpeedKmh == null ? "—" : `${Math.round(sample.comparisonSpeedKmh)} · ${formatGear(sample.comparisonGear)}`}
				colour={comparisonColour}
			/>
			<div className="col-span-full min-w-0 border-t border-trace-divider pt-2">
				<ComparisonSectorStrip sectors={sectorDeltas} value={sector} onChange={onSector} trackCarLabel={trackCarLabel} onClose={onClose} />
			</div>
		</div>
	);
}

function HudIdentity({ label, role, colour }: { label: string; role: string; colour: string }) {
	return (
		<span className="min-w-0 font-mono">
			<strong className="block truncate text-[10px] font-black leading-3 tracking-[.1em]" style={{ color: colour }}>
				{label}
			</strong>
			<span className="mt-0.5 block truncate text-[8px] font-bold leading-3 tracking-[.07em] text-trace-dim">{role}</span>
		</span>
	);
}

function formatComparisonGap(seconds?: number | null) {
	if (seconds == null) return "GAP —";
	if (Math.abs(seconds) < 0.0005) return "EVEN";
	return `${Math.abs(seconds).toFixed(3)}s ${seconds > 0 ? "BEHIND" : "AHEAD"}`;
}

function ComparisonSectorStrip({
	sectors,
	value,
	onChange,
	trackCarLabel,
	onClose,
}: {
	sectors: ComparisonSectorDelta[];
	value: number | null;
	onChange: (value: number | null) => void;
	trackCarLabel: string;
	onClose?: () => void;
}) {
	return (
		<div className="flex h-6 min-w-0 items-stretch gap-3 font-mono" aria-label="Sector comparison and telemetry range">
			<div className="flex min-w-0 flex-1 items-stretch gap-1.5 overflow-x-auto">
				<span className="flex w-28 shrink-0 items-center text-[9px] font-black tracking-[.08em] text-trace-accent">
					{value == null ? "VIEWING FULL LAP" : `VIEWING SECTOR ${value}`}
				</span>
				<button
					type="button"
					onClick={() => onChange(null)}
					className={`shrink-0 border px-2 text-[9px] font-black leading-none tracking-[.08em] ${value == null ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}
				>
					LAP
				</button>
				{sectors.map((item) => {
					const tone =
						item.seconds != null && item.seconds < -0.0005
							? "border-trace-accent/45 bg-trace-accent/10 text-trace-accent"
							: item.seconds != null && item.seconds > 0.0005
								? "border-trace-danger/60 bg-trace-danger/25 text-red-200"
								: "border-trace-divider bg-trace-deep text-trace-muted";
					return (
						<button
							type="button"
							onClick={() => onChange(item.index)}
							className={`flex shrink-0 items-center gap-1.5 border px-2 text-[10px] font-bold leading-none tabular-nums ${tone} ${value === item.index ? "outline outline-2 -outline-offset-2 outline-trace-accent" : "hover:border-trace-soft"}`}
							aria-pressed={value === item.index}
							key={item.index}
						>
							<span className="text-[9px] opacity-75">S{item.index}</span>
							<strong>
								{item.seconds == null
									? "—"
									: Math.abs(item.seconds) < 0.0005
										? "0.000"
										: `${item.seconds > 0 ? "+" : "−"}${Math.abs(item.seconds).toFixed(3)}`}
							</strong>
						</button>
					);
				})}
			</div>
			<span className="min-w-0 max-w-[38%] shrink-0 truncate text-right text-[10px] font-bold tracking-[.06em] text-trace-soft">{trackCarLabel}</span>
			{onClose && (
				<button
					type="button"
					onClick={onClose}
					className="grid size-6 shrink-0 place-items-center border border-trace-divider bg-trace-deep text-[12px] text-trace-muted hover:border-trace-soft hover:text-trace-text"
					aria-label="Close comparison"
				>
					×
				</button>
			)}
		</div>
	);
}
