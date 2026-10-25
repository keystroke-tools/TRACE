export interface ImportProgressOverlayProps {
	eyebrow?: string;
	title: string;
	detail: string;
	stage?: string;
	current?: number;
	total?: number;
}

export function ImportProgressOverlay({ eyebrow = "IMPORTING TELEMETRY", title, detail, stage = "IMPORTING", current, total }: ImportProgressOverlayProps) {
	const percentage = current != null && total != null && total > 0 ? Math.min(100, Math.round((current / total) * 100)) : null;
	return (
		<div className="fixed inset-0 z-[120] grid place-items-center bg-black/75 p-6" role="status" aria-live="polite" aria-busy="true">
			<section className="w-full max-w-md border border-trace-divider bg-[#151515] p-6">
				<div className="flex items-center gap-3">
					<span className="size-2 animate-pulse rounded-full bg-trace-accent" aria-hidden="true" />
					<span className="font-mono text-[10px] font-black tracking-[.14em] text-trace-accent">{eyebrow}</span>
				</div>
				<h2 className="mt-4 text-xl font-black">{title}</h2>
				<p className="mt-2 text-[13px] leading-5 text-trace-muted">{detail}</p>
				<div className="mt-5 h-1.5 overflow-hidden bg-trace-raised">
					<div
						className={`h-full bg-trace-accent transition-[width] duration-200 ${percentage == null ? "w-1/3 animate-pulse" : ""}`}
						style={percentage == null ? undefined : { width: `${percentage}%` }}
					/>
				</div>
				<div className="mt-3 flex justify-between font-mono text-[10px] text-trace-dim">
					<span>{stage}</span>
					<span>{percentage == null ? "PLEASE WAIT" : `${percentage}%`}</span>
				</div>
			</section>
		</div>
	);
}
