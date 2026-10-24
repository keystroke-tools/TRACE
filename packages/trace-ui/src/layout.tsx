import type { ReactNode } from "react";

export function SectionHeading({ index, children }: { index: string; children: ReactNode }) {
	return (
		<div className="text-[12px] font-extrabold tracking-[.14em] text-trace-soft">
			<span className="mr-2.5 text-trace-accent">{index}</span>
			{children}
		</div>
	);
}
