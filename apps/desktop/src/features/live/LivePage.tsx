import type { ReactNode } from "react";
import type { LiveBroadcastStatus, TelemetryStatus } from "../../data-source";
import { Metric, PageIntro, PanelTitle } from "../../components/layout";
import { Tooltip } from "../../Tooltip";
import { useToast } from "../../Toast";

export function LivePage({
	status,
	liveBroadcast,
	onStartLive,
	onStopLive,
	onCopyLiveLink,
	onOpenSessions,
	onSelectSimulator,
}: {
	status: TelemetryStatus | null;
	liveBroadcast: LiveBroadcastStatus | null;
	onStartLive: () => void;
	onStopLive: () => void;
	onCopyLiveLink: () => void;
	onOpenSessions: () => void;
	onSelectSimulator: (simulatorId: string) => Promise<void>;
}) {
	const recording = status?.connection === "recording" || status?.connection === "replay";
	const simulatorName = status?.simulatorName ?? "YOUR SIMULATOR";
	const simulatorShortName = status?.simulatorShortName ?? "SIM";
	const availableChannels = status?.channels.filter((channel) => channel.available) ?? [];
	const unavailableChannels = status?.channels.filter((channel) => !channel.available) ?? [];
	const categories = Array.from(new Set(availableChannels.map((channel) => channel.category)));
	const liveActive = liveBroadcast?.sourceSessionId === "active-capture" && ["connecting", "reconnecting", "live", "ending"].includes(liveBroadcast.phase);

	return (
		<>
			<PageIntro
				index="01"
				eyebrow="LIVE CAPTURE"
				title={recording ? `RECORDING ${simulatorName.toUpperCase()}` : `READY WHEN ${simulatorName.toUpperCase()} IS`}
				description={
					recording
						? "TRACE is recording the current drive or replay automatically. Keep it running until the session ends."
						: `Start a drive or play a replay in ${simulatorName}. TRACE detects it and records locally—there is no record button to press.`
				}
			/>
			<SimulatorPicker status={status} onSelect={onSelectSimulator} />
			<div className="mt-3 flex flex-wrap items-center gap-2 border border-trace-divider bg-trace-surface p-3">
				<button
					type="button"
					disabled={!recording || (!!liveBroadcast && !["idle", "ended", "error"].includes(liveBroadcast.phase) && !liveActive)}
					onClick={liveActive ? onStopLive : onStartLive}
					className="h-10 border border-trace-accent/60 bg-trace-accent-wash px-4 text-[12px] font-black tracking-[.08em] text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
				>
					{liveActive
						? liveBroadcast?.phase === "ending"
							? "ENDING…"
							: liveBroadcast?.phase === "reconnecting"
								? "RECONNECTING…"
								: "STOP LIVE"
						: "GO LIVE"}
				</button>
				{liveActive && liveBroadcast?.spectatorUrl && (
					<button
						type="button"
						onClick={onCopyLiveLink}
						className="h-10 border border-trace-divider bg-trace-deep px-4 text-[12px] font-bold text-trace-soft hover:text-white"
					>
						COPY LIVE LINK
					</button>
				)}
				<span className="ml-auto text-[11px] text-trace-dim">
					{recording ? "Publish this capture without interrupting local recording." : "Start driving or play a replay to enable streaming."}
				</span>
			</div>
			<div className="my-[14px] mb-6 grid grid-cols-4 border border-trace-divider max-[900px]:grid-cols-2">
				<Metric label="SOURCE" value={status?.source ?? "INITIALISING"} accent />
				<Metric label="STATE" value={status?.connection.toUpperCase() ?? "WAIT"} />
				<Metric label="SAMPLE RATE" value={status?.sampleRateHz ? `${status.sampleRateHz} HZ` : "—"} />
				<Metric label="STORAGE" value="LOCAL" />
			</div>

			<div className="grid grid-cols-[1.4fr_1fr] border border-trace-divider bg-trace-surface max-[1000px]:grid-cols-1">
				<div className="border-r border-trace-divider max-[1000px]:border-b max-[1000px]:border-r-0">
					<PanelTitle>WHAT TRACE RECORDS</PanelTitle>
					<p className="px-5 pt-4 text-[13px] leading-5 text-trace-faint">
						TRACE saves portable analysis-ready channels plus the selected adapter's complete documented native data. {simulatorShortName}-native
						values remain in source units for future analysis; this is recording coverage, not a live sensor test.
					</p>
					<div className="grid grid-cols-2 gap-px p-4 max-[900px]:grid-cols-1">
						{categories.map((category) => (
							<div className="border border-trace-divider bg-trace-deep p-4" key={category}>
								<strong
									className={`font-mono text-[12px] tracking-[.1em] ${category.includes("NATIVE") ? "text-trace-purple" : "text-trace-accent"}`}
								>
									{category}
								</strong>
								<div className="mt-3 flex flex-wrap gap-2">
									{availableChannels
										.filter((channel) => channel.category === category)
										.map((channel) => (
											<Tooltip
												className="border border-trace-divider bg-trace-surface px-2.5 py-1.5 text-[12px] text-trace-soft"
												content={channel.detail}
												key={channel.id}
											>
												{channel.label}
											</Tooltip>
										))}
								</div>
							</div>
						))}
					</div>
				</div>
				<div>
					<PanelTitle>HOW CAPTURE WORKS</PanelTitle>
					<ol className="space-y-5 p-5">
						<WorkflowStep number="1" title={`Open ${simulatorName}`}>
							Start driving or play a replay at normal speed.
						</WorkflowStep>
						<WorkflowStep number="2" title="TRACE records automatically">
							The status light pulses while samples are being saved.
						</WorkflowStep>
						<WorkflowStep number="3" title="Review the session">
							End normally, then open Sessions to inspect laps or export data.
						</WorkflowStep>
					</ol>
					<div className="mx-5 mb-5 flex flex-wrap gap-2">
						<button
							type="button"
							onClick={onOpenSessions}
							className="border border-trace-accent-muted bg-trace-accent-wash px-4 py-3 text-[12px] font-black tracking-[.1em] text-trace-accent hover:border-trace-accent"
						>
							OPEN SESSIONS
						</button>
					</div>
				</div>
			</div>

			{unavailableChannels.length > 0 && (
				<details className="border border-t-0 border-trace-divider bg-trace-deep text-[12px]">
					<summary className="cursor-pointer px-5 py-4 font-bold tracking-[.06em] text-trace-muted hover:text-trace-text">
						WHY SOME {simulatorShortName} DATA IS NOT AVAILABLE
					</summary>
					<div className="grid gap-px border-t border-trace-divider bg-trace-divider sm:grid-cols-2">
						{unavailableChannels.map((channel) => (
							<div className="bg-trace-deep px-5 py-4" key={channel.id}>
								<strong className="text-trace-soft">{channel.label}</strong>
								<p className="mt-1 leading-5 text-trace-dim">
									{channel.detail}. TRACE leaves uncertain data out instead of assigning it a misleading meaning.
								</p>
							</div>
						))}
					</div>
				</details>
			)}
		</>
	);
}

function SimulatorPicker({ status, onSelect }: { status: TelemetryStatus | null; onSelect: (simulatorId: string) => Promise<void> }) {
	const showToast = useToast();
	const simulators = status?.simulators ?? [];
	const selectable = simulators.filter((simulator) => simulator.available);

	async function changeSimulator(simulatorId: string) {
		try {
			await onSelect(simulatorId);
		} catch (error) {
			showToast({ kind: "error", title: "Simulator not changed", message: error instanceof Error ? error.message : String(error), timeoutMs: 7_000 });
		}
	}

	return (
		<div className="mt-5 flex min-h-14 items-center border border-trace-divider bg-trace-surface">
			<label className="flex h-14 min-w-64 items-center gap-3 border-r border-trace-divider px-4">
				<span className="font-mono text-[12px] font-bold tracking-[.1em] text-trace-dim">SIMULATOR</span>
				<select
					aria-label="Capture simulator"
					value={status?.simulatorId ?? ""}
					disabled={selectable.length <= 1}
					onChange={(event) => void changeSimulator(event.target.value)}
					className="trace-select min-w-0 flex-1 border-0 bg-transparent pl-2 text-[12px] font-bold text-trace-text outline-none disabled:cursor-default disabled:opacity-100"
				>
					{simulators.map((simulator) => (
						<option value={simulator.id} disabled={!simulator.available} key={simulator.id}>
							{simulator.name}
							{simulator.available ? "" : " · unavailable"}
						</option>
					))}
				</select>
			</label>
			<span className="px-4 text-[12px] text-trace-dim">
				{selectable.length} capture adapter{selectable.length === 1 ? "" : "s"} installed
			</span>
		</div>
	);
}

function WorkflowStep({ number, title, children }: { number: string; title: string; children: ReactNode }) {
	return (
		<li className="flex gap-3">
			<span className="grid size-7 shrink-0 place-items-center border border-trace-accent-muted font-mono text-[12px] text-trace-accent">{number}</span>
			<div>
				<strong className="block text-[13px] text-trace-text">{title}</strong>
				<span className="mt-1 block text-[12px] leading-5 text-trace-faint">{children}</span>
			</div>
		</li>
	);
}
