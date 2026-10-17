import { useEffect, useRef } from "react";
import type { TelemetryStatus } from "../data-source";
import { useToast } from "../Toast";

export function SimulatorPicker({
	status,
	onSelect,
}: {
	status: TelemetryStatus | null;
	onSelect: (simulatorId: string) => Promise<void>;
}) {
	const showToast = useToast();
	const menu = useRef<HTMLDetailsElement>(null);
	const simulators = status?.simulators ?? [];
	const selected = simulators.find((simulator) => simulator.id === status?.simulatorId);
	const detected = simulators.find((simulator) => simulator.id === status?.activeSimulatorId);
	const displayed = status?.simulatorId === "auto" && detected ? detected : selected;

	useEffect(() => {
		function closeMenu(event: PointerEvent) {
			if (menu.current && !menu.current.contains(event.target as Node)) menu.current.removeAttribute("open");
		}
		document.addEventListener("pointerdown", closeMenu);
		return () => document.removeEventListener("pointerdown", closeMenu);
	}, []);

	async function changeSimulator(simulatorId: string) {
		menu.current?.removeAttribute("open");
		try {
			await onSelect(simulatorId);
		} catch (error) {
			showToast({
				kind: "error",
				title: "Simulator not changed",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 7_000,
			});
		}
	}

	return (
		<details ref={menu} className="group relative min-w-0 border-l border-trace-divider">
			<summary className="flex h-12 w-[190px] cursor-pointer list-none items-center gap-2.5 px-3 text-left outline-none marker:hidden hover:bg-trace-raised focus-visible:bg-trace-raised">
				<SimulatorMark simulatorId={displayed?.id ?? "auto"} />
				<span className="min-w-0 flex-1">
					<span className="block font-mono text-[9px] font-bold tracking-[.12em] text-trace-dim">{status?.simulatorId === "auto" ? "AUTO-DETECT" : "SIMULATOR"}</span>
					<span className="block truncate text-[11px] font-bold leading-4 text-trace-text">{detected && status?.simulatorId === "auto" ? `${detected.name} detected` : (selected?.name ?? "Loading…")}</span>
				</span>
				<svg aria-hidden="true" viewBox="0 0 12 12" className="size-3 shrink-0 text-trace-dim transition-transform group-open:rotate-180">
					<path d="m2.5 4.5 3.5 3 3.5-3" fill="none" stroke="currentColor" strokeWidth="1.25" />
				</svg>
			</summary>
			<div className="absolute right-0 top-full z-[120] mt-px w-[280px] border border-trace-divider bg-trace-black p-1 shadow-[0_12px_30px_rgba(0,0,0,.45)]">
				<div className="px-3 pb-2 pt-1.5 text-[10px] leading-4 text-trace-dim">Auto-detect connects to the first supported sim that starts sending telemetry.</div>
				{simulators.map((simulator) => {
					const active = simulator.id === status?.simulatorId;
					const isDetected = simulator.id === status?.activeSimulatorId;
					return (
						<button
							type="button"
							key={simulator.id}
							disabled={!simulator.available}
							onClick={() => void changeSimulator(simulator.id)}
							className={`flex w-full items-center gap-3 px-3 py-2.5 text-left disabled:cursor-not-allowed disabled:text-trace-muted ${active ? "bg-trace-accent-wash text-trace-accent" : "text-trace-soft hover:bg-trace-deep hover:text-white"}`}
						>
							<SimulatorMark simulatorId={simulator.id} />
							<span className="min-w-0 flex-1 text-[12px] font-bold">{simulator.name}</span>
							<span className="font-mono text-[9px] font-bold tracking-[.08em] text-trace-dim">
								{simulator.available ? (isDetected ? "DETECTED" : active ? "SELECTED" : simulator.shortName) : "UNAVAILABLE"}
							</span>
						</button>
					);
				})}
			</div>
		</details>
	);
}

function SimulatorMark({ simulatorId }: { simulatorId: string }) {
	if (simulatorId === "assetto-corsa") {
		return <span className="grid h-7 w-8 shrink-0 place-items-center bg-[#bd262d] font-mono text-[10px] font-black italic tracking-[-.08em] text-white">AC</span>;
	}
	if (simulatorId === "le-mans-ultimate") {
		return <span className="grid h-7 w-8 shrink-0 place-items-center border border-[#d7aa46] bg-[#d7aa46]/10 font-mono text-[9px] font-black tracking-[-.06em] text-[#e4bd65]">LMU</span>;
	}
	return <span className="grid h-7 w-8 shrink-0 place-items-center border border-trace-accent-muted bg-trace-accent-wash font-mono text-[12px] font-black text-trace-accent">//</span>;
}
