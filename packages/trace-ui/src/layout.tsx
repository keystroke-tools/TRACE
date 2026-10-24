import type { ReactNode } from "react";

export function SectionHeading({ index, children }: { index: string; children: ReactNode }) {
	return (
		<div className="trace-section-heading text-[12px] font-extrabold tracking-[.14em] text-trace-soft">
			<span className="trace-section-heading-index mr-2.5 text-trace-accent">{index}</span>
			{children}
		</div>
	);
}

export function OwnershipBadge({ ownership }: { ownership: "mine" | "other" }) {
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

export function SessionIdentityBlock({
	sessionType,
	title,
	car,
	track,
	date,
	driver,
	ownership,
}: {
	sessionType: string;
	title: string;
	car: string;
	track?: string | null;
	date: ReactNode;
	driver?: string | null;
	ownership?: "mine" | "other" | "unknown";
}) {
	return (
		<div className="min-w-0">
			<div className="flex min-w-0 items-center gap-3">
				<span className="shrink-0 font-mono text-[12px] font-extrabold tracking-[.1em] text-trace-accent">{sessionType}</span>
				<h2 className="min-w-0 truncate text-base font-black tracking-[.02em]">{title}</h2>
				{ownership && ownership !== "unknown" && <OwnershipBadge ownership={ownership} />}
			</div>
			<span className="mt-2 flex min-w-0 items-center gap-2 text-[12px] text-trace-dim">
				<span className="shrink-0 text-trace-soft">{car}</span>
				{track && title !== track && (
					<>
						<span aria-hidden="true">·</span>
						<span className="min-w-0 truncate">{track}</span>
					</>
				)}
				<span aria-hidden="true">·</span>
				<span className="shrink-0">{date}</span>
				{driver && (
					<>
						<span aria-hidden="true">·</span>
						<span className="min-w-0 truncate text-trace-muted">{driver}</span>
					</>
				)}
			</span>
		</div>
	);
}
