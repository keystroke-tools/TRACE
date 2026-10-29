import type { ReactNode } from "react";
import {
	formatSessionDate as formatSharedSessionDate,
	formatSessionType as formatSharedSessionType,
	sectorPerformance as sharedSectorPerformance,
	type SessionOverviewLap,
} from "@trace/ui";
export { OwnershipBadge } from "@trace/ui";
import type { RecordedLapMetrics, RecordedSessionSummary } from "../../data-source";
import { Tooltip } from "../../Tooltip";

export function LapMetricValue({ state, value, detail }: { state: "loading" | "ready" | "error"; value: string | null; detail?: string | null }) {
	const label = state === "loading" ? "LOADING…" : (value ?? "—");
	const className = `truncate text-[12px] ${value ? "text-trace-soft" : "text-trace-dim"}`;
	return detail ? (
		<Tooltip className={className} content={detail}>
			{label}
		</Tooltip>
	) : (
		<span className={className}>{label}</span>
	);
}

export function formatFuelUsed(metrics?: RecordedLapMetrics) {
	return metrics?.fuelUsedLitres != null ? `${metrics.fuelUsedLitres.toFixed(2)} L` : null;
}

export function fuelDetail(metrics?: RecordedLapMetrics) {
	return metrics?.fuelStartLitres != null && metrics.fuelEndLitres != null
		? `${metrics.fuelStartLitres.toFixed(2)} L → ${metrics.fuelEndLitres.toFixed(2)} L`
		: null;
}

export function FuelUsage({ state, metrics }: { state: "loading" | "ready" | "error"; metrics?: RecordedLapMetrics }) {
	const used = formatFuelUsed(metrics);
	const capacity = metrics?.fuelCapacityLitres;
	const remaining = metrics?.fuelEndLitres;
	if (state === "loading") return <span className="text-[12px] text-trace-dim">LOADING…</span>;
	if (capacity == null || remaining == null || !Number.isFinite(capacity) || !Number.isFinite(remaining) || capacity <= 0) {
		return <LapMetricValue state={state} value={used} detail={fuelDetail(metrics)} />;
	}
	const percentage = Math.min(100, Math.max(0, (remaining / capacity) * 100));
	const fill = percentage <= 10 ? "bg-trace-danger" : percentage <= 25 ? "bg-trace-warning" : "bg-trace-accent";
	const detail = `${remaining.toFixed(2)} L of ${capacity.toFixed(2)} L remaining${used ? ` · ${used} consumed this lap` : ""}`;
	return (
		<Tooltip className="flex w-full min-w-0 flex-col" content={detail}>
			<span className="flex w-full items-center justify-between gap-2 text-[12px]">
				<span className="truncate text-trace-soft">{used ? `${used} USED` : "—"}</span>
				<span className="shrink-0 text-trace-faint">{Math.round(percentage)}%</span>
			</span>
			<span className="mt-2 block h-1.5 w-full bg-trace-divider" aria-hidden="true">
				<span className={`block h-full ${fill}`} style={{ width: `${percentage}%` }} />
			</span>
		</Tooltip>
	);
}

function virtualEnergyScale(metrics?: RecordedLapMetrics) {
	const values = [metrics?.virtualEnergyStart, metrics?.virtualEnergyEnd].filter((value): value is number => value != null && Number.isFinite(value));
	return values.length > 0 && values.every((value) => value >= 0 && value <= 1.001) ? 100 : 1;
}

export function formatVirtualEnergyUsed(metrics?: RecordedLapMetrics) {
	if (metrics?.virtualEnergyUsed == null || !Number.isFinite(metrics.virtualEnergyUsed)) return null;
	const scale = virtualEnergyScale(metrics);
	return `${(metrics.virtualEnergyUsed * scale).toFixed(1)}${scale === 100 ? "%" : ""} VE`;
}

export function VirtualEnergyUsage({ metrics }: { metrics?: RecordedLapMetrics }) {
	const used = formatVirtualEnergyUsed(metrics);
	if (!used) return null;
	const scale = virtualEnergyScale(metrics);
	const start = metrics?.virtualEnergyStart;
	const end = metrics?.virtualEnergyEnd;
	const detail =
		start != null && end != null
			? `${(start * scale).toFixed(1)}${scale === 100 ? "%" : ""} → ${(end * scale).toFixed(1)}${scale === 100 ? "%" : ""} virtual energy`
			: null;
	return <LapMetricValue state="ready" value={`${used} USED`} detail={detail} />;
}

export function StintUsage({ state, metrics }: { state: "loading" | "ready" | "error"; metrics?: RecordedLapMetrics }) {
	return (
		<div className="flex w-full min-w-0 flex-col gap-2">
			<FuelUsage state={state} metrics={metrics} />
			{state !== "loading" && <VirtualEnergyUsage metrics={metrics} />}
		</div>
	);
}

const tyreLabels = ["FL", "FR", "RL", "RR"];

function tyreColour(remaining: number) {
	if (remaining >= 75) return "border-trace-accent text-trace-accent";
	if (remaining >= 50) return "border-trace-sector-yellow text-trace-sector-yellow";
	if (remaining >= 25) return "border-orange-500 text-orange-300";
	return "border-trace-danger text-red-300";
}

export function TyreState({ state, metrics }: { state: "loading" | "ready" | "error"; metrics?: RecordedLapMetrics }) {
	if (state === "loading") return <span className="text-[12px] text-trace-dim">LOADING…</span>;
	const values = metrics?.tyreWearEnd ?? [];
	if (!values.some((value) => value != null && Number.isFinite(value))) {
		return <LapMetricValue state={state} value={null} />;
	}
	return (
		<div className="grid grid-cols-2 gap-1.5" aria-label="Tyre life remaining at the end of the lap">
			{tyreLabels.map((label, index) => {
				const value = values[index];
				const remaining = value == null || !Number.isFinite(value) ? null : Math.min(100, Math.max(0, value));
				const flat = metrics?.tyreFlatObserved?.[index] ?? false;
				const detached = metrics?.wheelDetachedObserved?.[index] ?? false;
				const start = metrics?.tyreWearStart[index];
				const minimum = metrics?.tyreWearMinimum[index];
				const detail = detached
					? `${label}: wheel detached during this lap`
					: flat
						? `${label}: tyre was reported flat during this lap`
						: remaining == null
							? `${label}: unavailable`
							: `${label}: ${Math.round(remaining)}% remaining${start != null ? ` · started at ${Math.round(start)}%` : ""}${minimum != null ? ` · minimum ${Math.round(minimum)}%` : ""}`;
				return (
					<Tooltip
						className={`grid size-9 place-items-center rounded-full border-2 bg-trace-deep font-mono text-[9px] font-black leading-none ${detached || flat ? "border-trace-danger bg-trace-danger/15 text-red-200" : remaining == null ? "border-trace-divider text-trace-dim" : tyreColour(remaining)}`}
						content={detail}
						key={label}
					>
						{detached || flat ? "!" : remaining == null ? label : `${Math.round(remaining)}%`}
					</Tooltip>
				);
			})}
		</div>
	);
}

function median(values: number[]) {
	if (values.length === 0) return null;
	const sorted = [...values].sort((left, right) => left - right);
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

export function enduranceEstimates(metrics: RecordedLapMetrics[]) {
	const fuelPerLap = median(metrics.map((lap) => lap.fuelUsedLitres).filter((value): value is number => value != null && value > 0));
	const lastFuel = [...metrics].reverse().find((lap) => lap.fuelEndLitres != null)?.fuelEndLitres ?? null;
	const virtualEnergyPerLap = median(metrics.map((lap) => lap.virtualEnergyUsed).filter((value): value is number => value != null && value > 0));
	const lastEnergyMetrics = [...metrics].reverse().find((lap) => lap.virtualEnergyEnd != null);
	const lastVirtualEnergy = lastEnergyMetrics?.virtualEnergyEnd ?? null;
	const energyScale = virtualEnergyScale(lastEnergyMetrics);
	const tyreLosses = metrics.flatMap((lap) =>
		lap.tyreWearStart.flatMap((start, index) => {
			const end = lap.tyreWearEnd[index];
			return start != null && end != null && start > end ? [start - end] : [];
		}),
	);
	const tyreWearPerLap = median(tyreLosses);
	const lastTyreMetrics = [...metrics].reverse().find((lap) => lap.tyreWearEnd.some((value) => value != null));
	const lowestTyre = lastTyreMetrics?.tyreWearEnd.reduce<number | null>(
		(lowest, value) => (value == null ? lowest : lowest == null ? value : Math.min(lowest, value)),
		null,
	);
	return {
		fuelPerLap,
		fuelLapsRemaining: fuelPerLap && lastFuel != null ? lastFuel / fuelPerLap : null,
		lastFuel,
		virtualEnergyPerLap: virtualEnergyPerLap == null ? null : virtualEnergyPerLap * energyScale,
		virtualEnergyLapsRemaining: virtualEnergyPerLap && lastVirtualEnergy != null ? lastVirtualEnergy / virtualEnergyPerLap : null,
		lastVirtualEnergy: lastVirtualEnergy == null ? null : lastVirtualEnergy * energyScale,
		virtualEnergyIsPercentage: energyScale === 100,
		tyreWearPerLap,
		tyreLapsRemaining: tyreWearPerLap && lowestTyre != null ? lowestTyre / tyreWearPerLap : null,
		lowestTyre,
	};
}

export function EnduranceSummary({ metrics }: { metrics: RecordedLapMetrics[] }) {
	const estimates = enduranceEstimates(metrics);
	const cards = [
		estimates.fuelPerLap != null
			? {
					label: "FUEL / LAP",
					value: `${estimates.fuelPerLap.toFixed(2)} L`,
					detail:
						estimates.fuelLapsRemaining != null
							? `${estimates.lastFuel?.toFixed(1)} L remaining · approximately ${estimates.fuelLapsRemaining.toFixed(1)} laps`
							: "Median recorded fuel use",
				}
			: null,
		estimates.virtualEnergyPerLap != null
			? {
					label: "VE / LAP",
					value: `${estimates.virtualEnergyPerLap.toFixed(1)}${estimates.virtualEnergyIsPercentage ? "%" : ""}`,
					detail:
						estimates.virtualEnergyLapsRemaining != null
							? `${estimates.lastVirtualEnergy?.toFixed(1)}${estimates.virtualEnergyIsPercentage ? "%" : ""} remaining · approximately ${estimates.virtualEnergyLapsRemaining.toFixed(1)} laps`
							: "Median recorded virtual-energy use",
				}
			: null,
		estimates.lowestTyre != null
			? {
					label: "LOWEST TYRE",
					value: `${Math.round(estimates.lowestTyre)}%`,
					detail:
						estimates.tyreLapsRemaining != null
							? `${estimates.tyreWearPerLap?.toFixed(2)} percentage points per lap · approximately ${estimates.tyreLapsRemaining.toFixed(1)} laps`
							: "Lowest remaining tyre life at the end of the latest measured lap",
				}
			: null,
	].filter((card): card is { label: string; value: string; detail: string } => card != null);
	if (cards.length === 0) return null;
	return (
		<section className="mt-4 border border-trace-divider bg-trace-surface" aria-label="Endurance estimates">
			<header className="flex items-center justify-between gap-4 border-b border-trace-divider px-5 py-3">
				<strong className="font-mono text-[11px] tracking-[.08em] text-trace-soft">STINT ESTIMATE</strong>
				<span className="text-[10px] text-trace-dim">Median clean-lap use · pit and refill increases excluded</span>
			</header>
			<div className="grid divide-x divide-trace-divider" style={{ gridTemplateColumns: `repeat(${cards.length}, minmax(0, 1fr))` }}>
				{cards.map((card) => (
					<Tooltip className="min-w-0 px-5 py-3" content={card.detail} key={card.label}>
						<small className="block font-mono text-[9px] font-bold tracking-[.1em] text-trace-dim">{card.label}</small>
						<strong className="mt-1 block font-mono text-[16px] text-trace-text">{card.value}</strong>
					</Tooltip>
				))}
			</div>
		</section>
	);
}

export function lapIsInvalid(lap: RecordedSessionSummary["laps"][number]) {
	return lap.validity === "invalid" || (lap.maxTyresOut != null && lap.maxTyresOut >= 3);
}

export function SectorLegend({ colour, label }: { colour: string; label: string }) {
	return (
		<span className="inline-flex items-center gap-1.5">
			<span className={`h-1.5 w-3 ${colour}`} aria-hidden="true" />
			{label}
		</span>
	);
}

export function SectorBars({
	lap,
	laps,
	sectorIndices,
}: {
	lap: RecordedSessionSummary["laps"][number];
	laps: RecordedSessionSummary["laps"];
	sectorIndices: number[];
}) {
	const timingLaps = sessionTimingLaps(laps);
	const timingLap = timingLaps.find((candidate) => candidate.index === lap.index)!;
	return (
		<div className="flex min-w-0 gap-1.5 overflow-x-auto" aria-label={`Sector times for lap ${lap.index}`}>
			{sectorIndices.map((index) => {
				const sector = lap.sectors.find((candidate) => candidate.index === index);
				const performance = sharedSectorPerformance(timingLaps, timingLap, index);
				const colour =
					performance === "purple"
						? "bg-trace-purple"
						: performance === "green"
							? "bg-trace-accent"
							: performance === "yellow"
								? "bg-trace-sector-yellow"
								: "bg-trace-dim";
				return (
					<Tooltip className="min-w-[72px] flex-1 flex-col" content={`Sector ${index}: ${sector?.time ?? "unavailable"}`} key={index}>
						<div className={`h-1.5 ${colour}`} aria-hidden="true" />
						<span className="mt-1 block truncate text-[12px] text-trace-faint">
							S{index} {sector?.time ?? "—"}
						</span>
					</Tooltip>
				);
			})}
		</div>
	);
}

export function ElectronicsSummary({ metrics }: { metrics?: RecordedLapMetrics }) {
	const electronics = metrics?.electronics;
	if (!electronics) return null;
	const activity = [
		["TC ACTIVE", electronics.tcActivePercent],
		["ABS ACTIVE", electronics.absActivePercent],
		["DRS OPEN", electronics.rearFlapActivePercent],
	] as const;
	if (electronics.settings.length === 0 && activity.every(([, percent]) => percent == null || percent < 0.1)) return null;
	return (
		<div className="mt-2 flex min-w-0 flex-wrap gap-1.5" aria-label="Electronics used during this lap">
			{electronics.settings.map((setting) => {
				const changed = setting.changes > 0;
				const value = changed ? `${setting.start}→${setting.end}` : String(setting.start);
				const detail = [
					`${setting.label}: ${value}`,
					changed ? `${setting.changes} change${setting.changes === 1 ? "" : "s"} during the lap` : "Unchanged during the lap",
					setting.minimum !== setting.maximum ? `Observed range ${setting.minimum}–${setting.maximum}` : null,
					setting.availableMaximum != null ? `Available maximum ${setting.availableMaximum}` : null,
				]
					.filter(Boolean)
					.join(" · ");
				return (
					<Tooltip
						className={`border px-1.5 py-1 text-[9px] font-bold leading-none ${changed ? "border-trace-warning/70 text-trace-warning" : "border-trace-divider text-trace-muted"}`}
						content={detail}
						key={setting.key}
					>
						{setting.label} {value}
					</Tooltip>
				);
			})}
			{activity.map(([label, percent]) =>
				percent != null && percent >= 0.1 ? (
					<Tooltip
						className="border border-trace-accent/50 px-1.5 py-1 text-[9px] font-bold leading-none text-trace-accent"
						content={`${label.replace(" ACTIVE", "").replace(" OPEN", "")} was active for ${percent.toFixed(1)}% of recorded samples in this lap`}
						key={label}
					>
						{label} {Math.round(percent)}%
					</Tooltip>
				) : null,
			)}
		</div>
	);
}

export function DamageSummary({ metrics }: { metrics?: RecordedLapMetrics }) {
	if (!metrics) return null;
	const dent = metrics.dentSeverityMaximum ?? [];
	const damagedZones = dent.filter((severity) => severity > 0).length;
	const severeZones = dent.filter((severity) => severity >= 2).length;
	const failures = [
		metrics.overheatingObserved ? "OVERHEAT" : null,
		metrics.bodyPartDetachedObserved ? "PART LOST" : null,
		metrics.tyreFlatObserved?.some(Boolean) ? "PUNCTURE" : null,
		metrics.wheelDetachedObserved?.some(Boolean) ? "WHEEL LOST" : null,
	].filter((value): value is string => value != null);
	if (damagedZones === 0 && failures.length === 0 && !metrics.impactCount) return null;
	const dentDetail = damagedZones > 0 ? `${damagedZones}/8 body zones damaged${severeZones > 0 ? ` · ${severeZones} severe` : ""}` : null;
	const impactDetail = metrics.impactCount
		? `${metrics.impactCount} impact${metrics.impactCount === 1 ? "" : "s"} detected during this lap${metrics.maximumImpactMagnitude != null ? ` · peak magnitude ${metrics.maximumImpactMagnitude.toFixed(1)}` : ""}`
		: null;
	return (
		<div className="mt-1.5 flex min-w-0 flex-wrap gap-1.5" aria-label="Damage observed during this lap">
			{damagedZones > 0 && (
				<Tooltip className="border border-red-500/60 px-1.5 py-1 text-[9px] font-bold leading-none text-red-300" content={dentDetail}>
					BODY {damagedZones}/8
				</Tooltip>
			)}
			{metrics.impactCount ? (
				<Tooltip className="border border-trace-warning/60 px-1.5 py-1 text-[9px] font-bold leading-none text-trace-warning" content={impactDetail}>
					IMPACT ×{metrics.impactCount}
				</Tooltip>
			) : null}
			{failures.map((failure) => (
				<Tooltip
					className="border border-red-500/70 px-1.5 py-1 text-[9px] font-bold leading-none text-red-300"
					content={`${failure} was observed during this lap`}
					key={failure}
				>
					{failure}
				</Tooltip>
			))}
		</div>
	);
}

function sessionTimingLaps(laps: RecordedSessionSummary["laps"]): SessionOverviewLap[] {
	return laps.map((lap) => ({
		id: String(lap.index),
		index: lap.index,
		durationNs: Number.isFinite(lapDuration(lap)) ? lapDuration(lap) : null,
		invalid: lapIsInvalid(lap),
		sectors: lap.sectors.map((sector) => ({ index: sector.index, durationNs: sector.durationNs })),
	}));
}

export function ExportOption({ label, detail, disabled, onClick }: { label: string; detail: string; disabled: boolean; onClick: () => void }) {
	return (
		<button
			type="button"
			disabled={disabled}
			onClick={onClick}
			className="block w-full border-0 bg-transparent px-2 py-2 text-left hover:bg-trace-raised disabled:text-trace-dim"
		>
			<strong className="block font-mono text-[12px] tracking-[.08em] text-trace-text">{label}</strong>
			<span className="mt-1 block text-[12px] text-trace-dim">{detail}</span>
		</button>
	);
}

export function DeleteConfirmation({
	session,
	deleting,
	onCancel,
	onConfirm,
}: {
	session: RecordedSessionSummary;
	deleting: boolean;
	onCancel: () => void;
	onConfirm: () => void;
}) {
	return (
		<div className="p-2">
			<strong className="block text-[13px] text-trace-text">Delete this session?</strong>
			<p className="mt-2 text-[12px] leading-5 text-trace-faint">
				{session.track} and its saved telemetry will be permanently removed. This cannot be undone.
			</p>
			<div className="mt-4 grid grid-cols-2 gap-2">
				<button
					type="button"
					disabled={deleting}
					onClick={onCancel}
					className="border border-trace-divider bg-transparent px-3 py-2.5 text-[12px] font-bold text-trace-soft hover:bg-trace-raised disabled:text-trace-dim"
				>
					Cancel
				</button>
				<button
					type="button"
					disabled={deleting}
					onClick={onConfirm}
					className="border border-trace-warning bg-transparent px-3 py-2.5 text-[12px] font-bold text-trace-warning hover:bg-trace-warning hover:text-trace-black disabled:border-trace-divider disabled:text-trace-dim"
				>
					{deleting ? "Deleting…" : "Delete"}
				</button>
			</div>
		</div>
	);
}

export function SessionDetailsEditor({
	title,
	driver,
	ownership,
	tags,
	saving,
	onTitleChange,
	onDriverChange,
	onOwnershipChange,
	onTagsChange,
	onCancel,
	onSave,
}: {
	title: string;
	driver: string;
	ownership: RecordedSessionSummary["ownership"];
	tags: string;
	saving: boolean;
	onTitleChange: (value: string) => void;
	onDriverChange: (value: string) => void;
	onOwnershipChange: (value: RecordedSessionSummary["ownership"]) => void;
	onTagsChange: (value: string) => void;
	onCancel: () => void;
	onSave: () => void;
}) {
	return (
		<form
			className="p-2"
			onSubmit={(event) => {
				event.preventDefault();
				onSave();
			}}
		>
			<strong className="block text-[13px] text-trace-text">Session identity</strong>
			<label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
				DISPLAY NAME
				<input
					autoFocus
					maxLength={80}
					value={title}
					onChange={(event) => onTitleChange(event.target.value)}
					placeholder="Optional custom name"
					className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
				/>
			</label>
			<label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
				DRIVER / AUTHOR
				<input
					maxLength={80}
					value={driver}
					onChange={(event) => onDriverChange(event.target.value)}
					placeholder="Who drove this session?"
					className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
				/>
			</label>
			<label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
				OWNERSHIP
				<select
					value={ownership}
					onChange={(event) => onOwnershipChange(event.target.value as RecordedSessionSummary["ownership"])}
					className="trace-select mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep pl-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
				>
					<option value="unknown">Not specified</option>
					<option value="mine">My driving</option>
					<option value="other">Another driver</option>
				</select>
			</label>
			<label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
				TAGS
				<input
					value={tags}
					onChange={(event) => onTagsChange(event.target.value)}
					placeholder="league, wet, reference"
					className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
				/>
			</label>
			<p className="mt-2 text-[12px] leading-4 text-trace-dim">
				Separate up to 12 tags with commas. Name, driver, ownership, and tags are included in search.
			</p>
			<div className="mt-4 grid grid-cols-2 gap-2">
				<button
					type="button"
					disabled={saving}
					onClick={onCancel}
					className="border border-trace-divider bg-transparent px-3 py-2.5 text-[12px] font-bold text-trace-soft hover:bg-trace-raised disabled:text-trace-dim"
				>
					Cancel
				</button>
				<button
					type="submit"
					disabled={saving}
					className="border border-trace-accent bg-trace-accent-wash px-3 py-2.5 text-[12px] font-bold text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:text-trace-dim"
				>
					{saving ? "Saving…" : "Save"}
				</button>
			</div>
		</form>
	);
}

export function EmptySessions({ title, children }: { title: string; children: ReactNode }) {
	return (
		<div className="p-12 text-center">
			<span className="trace-crosshair mx-auto block" aria-hidden="true" />
			<strong className="mt-5 block text-base">{title}</strong>
			<p className="mx-auto mt-2 max-w-md text-[12px] leading-5 text-trace-faint">{children}</p>
		</div>
	);
}

export function lapInvalidityDetail(lap: RecordedSessionSummary["laps"][number]) {
	const partial = lap.validityReason?.includes("partial") ?? false;
	return partial
		? "TRACE joined after this lap began, so it is incomplete and excluded from comparisons."
		: lap.maxTyresOut != null && lap.maxTyresOut >= 3
			? "Three or more tyres were observed outside the track; this lap is excluded from comparisons."
			: (lap.validityReason ?? "The simulator marked this lap invalid.");
}

export function sessionSourceGroup(session: RecordedSessionSummary) {
	const source = session.source.toLocaleLowerCase();
	if (source.includes("replay")) return "replay";
	if (source.includes("import")) return "imported";
	return "native";
}

export function sessionSourceLabel(session: RecordedSessionSummary) {
	const source = sessionSourceGroup(session);
	if (source === "replay") return "Replay capture";
	if (source === "imported") return "Imported telemetry";
	return "Drive";
}

export function friendlySessionType(session: RecordedSessionSummary) {
	return formatSessionType(session.sessionType);
}

export function formatSessionType(value: string) {
	return formatSharedSessionType(value);
}

export function lapDuration(lap: RecordedSessionSummary["laps"][number]) {
	if (lap.durationNs != null) return lap.durationNs;
	return lapTimeMs(lap.time) * 1_000_000;
}

export function lapTimeMs(value: string) {
	const match = /^(\d+):(\d{2})\.(\d{3})$/.exec(value);
	if (!match) return Number.POSITIVE_INFINITY;
	return Number(match[1]) * 60_000 + Number(match[2]) * 1_000 + Number(match[3]);
}

export function formatLapDurationNs(durationNs: number) {
	const totalMilliseconds = Math.max(0, Math.round(durationNs / 1_000_000));
	const minutes = Math.floor(totalMilliseconds / 60_000);
	const seconds = Math.floor((totalMilliseconds % 60_000) / 1_000);
	const milliseconds = totalMilliseconds % 1_000;
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds).padStart(3, "0")}`;
}

export function formatSessionDate(value: string) {
	return formatSharedSessionDate(value);
}

export function formatCompactSessionDate(value: string) {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat(undefined, {
		day: "numeric",
		month: "short",
		year: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
	}).format(date);
}

export function friendlyConditionName(value: string) {
	return value.replace(/^\d+_/, "").replaceAll("_", " ").toUpperCase();
}

export function friendlySetupLabel(value: string) {
	return value
		.replace(/Setting$/i, "")
		.replace(/([a-z0-9])([A-Z])/g, "$1 $2")
		.replaceAll("_", " ")
		.replace(/\b\w/g, (character) => character.toUpperCase());
}

export function setupDifferenceLabel(section: string, key: string) {
	const sectionLabel = friendlySetupLabel(section);
	return key.trim().toUpperCase() === "VALUE" ? sectionLabel : `${sectionLabel} · ${friendlySetupLabel(key)}`;
}
