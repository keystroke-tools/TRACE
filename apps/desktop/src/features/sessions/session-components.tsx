import type { ReactNode } from "react";
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

export function TyreWearGrid({ state, metrics }: { state: "loading" | "ready" | "error"; metrics?: RecordedLapMetrics }) {
	if (state === "loading") return <span className="text-[12px] text-trace-dim">LOADING…</span>;
	const tyres = [
		{ short: "FL", name: "Front left", index: 0 },
		{ short: "FR", name: "Front right", index: 1 },
		{ short: "RL", name: "Rear left", index: 2 },
		{ short: "RR", name: "Rear right", index: 3 },
	];
	return (
		<div className="grid w-fit grid-cols-2 gap-1" aria-label="Lowest tyre condition observed during this lap">
			{tyres.map((tyre) => {
				const start = metrics?.tyreWearStart[tyre.index];
				const end = metrics?.tyreWearEnd[tyre.index];
				const minimum = metrics?.tyreWearMinimum[tyre.index];
				const observed = minimum != null && Number.isFinite(minimum) ? minimum : end;
				const remaining = observed != null && Number.isFinite(observed) ? Math.min(100, Math.max(0, observed)) : null;
				const colour = remaining == null ? null : tyreConditionColour(remaining);
				const value = remaining == null ? "—" : `${wholeTyreCondition(remaining)}%`;
				const used = start != null && remaining != null && Number.isFinite(start) ? Math.max(0, start - remaining) : null;
				const detail =
					remaining == null
						? `${tyre.name}: wear telemetry unavailable`
						: `${tyre.name}: ${value} condition remaining · ${remaining.toFixed(2)}% lowest observed${start != null ? ` · ${start.toFixed(2)}% at lap start${used != null ? ` · ${used.toFixed(2)}% used this lap` : ""}` : ""}`;
				return (
					<Tooltip key={tyre.short} content={detail}>
						<span
							className="flex size-9 items-center justify-center border font-mono text-[12px] font-bold tabular-nums"
							style={{
								borderRadius: "9999px",
								borderColor: colour?.border ?? "var(--color-trace-divider)",
								backgroundColor: colour?.background ?? "var(--color-trace-deep)",
								color: colour?.text ?? "var(--color-trace-dim)",
							}}
							aria-label={`${tyre.name}: ${remaining == null ? "unavailable" : `${value} condition remaining`}`}
						>
							{value}
						</span>
					</Tooltip>
				);
			})}
		</div>
	);
}

export function wholeTyreCondition(remainingPercent: number) {
	const condition = Math.min(100, Math.max(0, remainingPercent));
	return condition >= 99.999 ? 100 : Math.floor(condition);
}

export function tyreConditionColour(remainingPercent: number) {
	const condition = Math.min(1, Math.max(0, remainingPercent / 100));
	const hue = 110 * condition;
	return {
		border: `hsl(${hue} 82% 48%)`,
		background: `hsl(${hue} 82% 48% / 0.16)`,
		text: `hsl(${hue} 88% 68%)`,
	};
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

export function OwnershipBadge({ ownership }: { ownership: Exclude<RecordedSessionSummary["ownership"], "unknown"> }) {
	const mine = ownership === "mine";
	return (
		<span
			className={`inline-flex shrink-0 items-center gap-1.5 border px-1.5 py-0.5 font-mono text-[12px] font-bold tracking-[.08em] ${mine ? "border-trace-accent/40 bg-trace-accent/10 text-trace-accent" : "border-trace-purple/40 bg-trace-purple-wash text-trace-purple"}`}
			aria-label={mine ? "Your recording" : "Another driver's recording"}
		>
			<span className={`size-1.5 rounded-full ${mine ? "bg-trace-accent" : "bg-trace-purple"}`} aria-hidden="true" />
			{mine ? "YOU" : "OTHER"}
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
	return (
		<div className="flex min-w-0 gap-1.5 overflow-x-auto" aria-label={`Sector times for lap ${lap.index}`}>
			{sectorIndices.map((index) => {
				const sector = lap.sectors.find((candidate) => candidate.index === index);
				const performance = sectorPerformance(laps, lap, index);
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

export function sectorPerformance(laps: RecordedSessionSummary["laps"], lap: RecordedSessionSummary["laps"][number], sectorIndex: number) {
	const sector = lap.sectors.find((candidate) => candidate.index === sectorIndex);
	if (!sector || lapIsInvalid(lap)) return "grey";
	const comparable = laps
		.filter((candidate) => !lapIsInvalid(candidate))
		.flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex));
	const sessionBest = Math.min(...comparable.map((candidate) => candidate.durationNs));
	if (sector.durationNs === sessionBest) return "purple";
	const priorBest = Math.min(
		...laps
			.filter((candidate) => candidate.index < lap.index && !lapIsInvalid(candidate))
			.flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex))
			.map((candidate) => candidate.durationNs),
	);
	return sector.durationNs < priorBest ? "green" : "yellow";
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
	const type = session.sessionType.toLocaleLowerCase();
	if (type === "session") return "DRIVE";
	if (type === "qualify") return "QUALIFYING";
	if (type === "time_attack") return "TIME ATTACK";
	if (type.includes("replay")) return "REPLAY";
	return session.sessionType;
}

export function lapDuration(lap: RecordedSessionSummary["laps"][number]) {
	if (lap.durationNs != null) return lap.durationNs;
	return lapTimeMs(lap.time) * 1_000_000;
}

export function theoreticalBestLap(laps: RecordedSessionSummary["laps"], sectorIndices: number[]) {
	if (sectorIndices.length === 0) return null;
	const validLaps = laps.filter((lap) => !lapIsInvalid(lap));
	let totalDurationNs = 0;
	for (const index of sectorIndices) {
		const durations = validLaps
			.flatMap((lap) => lap.sectors)
			.filter((sector) => sector.index === index && Number.isFinite(sector.durationNs) && sector.durationNs > 0)
			.map((sector) => sector.durationNs);
		if (durations.length === 0) return null;
		totalDurationNs += Math.min(...durations);
	}
	const totalMilliseconds = Math.floor(totalDurationNs / 1_000_000);
	const minutes = Math.floor(totalMilliseconds / 60_000);
	const seconds = Math.floor((totalMilliseconds % 60_000) / 1_000);
	const milliseconds = totalMilliseconds % 1_000;
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds).padStart(3, "0")}`;
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
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat(undefined, {
		dateStyle: "medium",
		timeStyle: "short",
	}).format(date);
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
	return value.replaceAll("_", " ").replace(/\b\w/g, (character) => character.toUpperCase());
}

export function setupDifferenceLabel(section: string, key: string) {
	const sectionLabel = friendlySetupLabel(section);
	return key.trim().toUpperCase() === "VALUE" ? sectionLabel : `${sectionLabel} · ${friendlySetupLabel(key)}`;
}
