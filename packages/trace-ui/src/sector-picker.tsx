import type { LapComparisonSample } from "./telemetry-chart";

export function SectorPicker({ samples, value, onChange }: { samples: LapComparisonSample[]; value: number | null; onChange: (value: number | null) => void }) {
	const sectors = [...new Set(samples.flatMap((sample) => (sample.sectorIndex == null ? [] : [sample.sectorIndex])))].sort((left, right) => left - right);
	if (sectors.length === 0) return null;
	return (
		<div className="flex flex-wrap items-center gap-2" aria-label="Telemetry range">
			<span className="mr-2 font-mono text-[12px] font-bold tracking-[.1em] text-trace-dim">VIEW</span>
			<button
				type="button"
				onClick={() => onChange(null)}
				className={`border px-3 py-2 font-mono text-[12px] font-bold ${value == null ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}
			>
				FULL LAP
			</button>
			{sectors.map((item) => (
				<button
					type="button"
					onClick={() => onChange(item)}
					className={`border px-3 py-2 font-mono text-[12px] font-bold ${value === item ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}
					key={item}
				>
					SECTOR {item}
				</button>
			))}
		</div>
	);
}
