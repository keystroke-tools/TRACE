import type { ReactNode } from "react";
import { Tooltip } from "../Tooltip";

export function PageIntro({ index, eyebrow, title, description }: { index: string; eyebrow: string; title: string; description: string }) {
	return (
		<div className="max-w-4xl">
			<SectionHeading index={index}>{eyebrow}</SectionHeading>
			<h1 className="mt-3 text-2xl font-black tracking-[-.02em]">{title}</h1>
			<p className="mt-2 max-w-2xl text-[14px] leading-6 text-trace-muted">{description}</p>
		</div>
	);
}

export function SectionHeading({ index, children }: { index: string; children: ReactNode }) {
	return (
		<div className="text-[12px] font-extrabold tracking-[.14em] text-trace-soft">
			<span className="mr-2.5 text-trace-accent">{index}</span>
			{children}
		</div>
	);
}

export function PanelTitle({ children }: { children: ReactNode }) {
	return <div className="border-b border-trace-divider px-4 py-[14px] text-[12px] font-extrabold tracking-[.14em] text-trace-soft">{children}</div>;
}

export function Metric({
	label,
	value,
	detail,
	accent = false,
	purple = false,
}: {
	label: string;
	value: string;
	detail?: string;
	accent?: boolean;
	purple?: boolean;
}) {
	return (
		<div className="min-h-[92px] border-r border-trace-divider bg-trace-surface p-[18px] last:border-r-0 max-[900px]:[&:nth-child(-n+2)]:border-b max-[900px]:[&:nth-child(even)]:border-r-0">
			<span className="block text-[12px] font-extrabold tracking-[.12em] text-trace-muted">
				{detail ? <Tooltip content={detail}>{label}</Tooltip> : label}
			</span>
			<strong className={`mt-[15px] block font-mono text-base font-bold ${purple ? "text-trace-purple" : accent ? "text-trace-accent" : ""}`}>
				{value}
			</strong>
		</div>
	);
}
