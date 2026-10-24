import { useEffect, useMemo, useRef, useState } from "react";
import { sessionOverview } from "@trace/ui";
import { createPortal } from "react-dom";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
	telemetryDataSource,
	type CompatibleSetup,
	type LiveBroadcastStatus,
	type RecordedLapMetrics,
	type RecordedSessionSummary,
	type SetupComparison,
} from "../../data-source";
import { Metric, SectionHeading } from "../../components/layout";
import { Tooltip } from "../../Tooltip";
import { useToast } from "../../Toast";
import { ConsistencyChart } from "./ConsistencyChart";
import { LapPackageDialog, type LapPackageAction } from "./LapPackageDialog";
import { fasterReferenceSuggestions, type ComparisonSelection, validComparisonLaps } from "../compare/comparison-selection";
import { savedSetupFolder } from "../setups/setup-preferences";
import {
	DeleteConfirmation,
	EmptySessions,
	ExportOption,
	FuelUsage,
	LapMetricValue,
	OwnershipBadge,
	SectorBars,
	SectorLegend,
	SessionDetailsEditor,
	formatCompactSessionDate,
	formatFuelUsed,
	formatLapDurationNs,
	formatSessionDate,
	friendlyConditionName,
	friendlySessionType,
	lapDuration,
	lapInvalidityDetail,
	lapIsInvalid,
	sessionSourceGroup,
	sessionSourceLabel,
	setupDifferenceLabel,
} from "./session-components";

export function SessionDetail({
	session,
	sessions,
	onOpenLap,
	onCompare,
	liveBroadcast,
	onShowSummary,
	onStartLive,
	onStopLive,
	onCopyLiveLink,
	onOpenLiveLink,
}: {
	session: RecordedSessionSummary;
	sessions: RecordedSessionSummary[];
	onOpenLap: (lapIndex: number) => void;
	onCompare: (selection: ComparisonSelection) => void;
	liveBroadcast: LiveBroadcastStatus | null;
	onShowSummary: () => void;
	onStartLive: () => void;
	onStopLive: () => void;
	onCopyLiveLink: () => void;
	onOpenLiveLink: () => void;
}) {
	const showToast = useToast();
	const [metrics, setMetrics] = useState<RecordedLapMetrics[]>([]);
	const [metricsState, setMetricsState] = useState<"loading" | "ready" | "error">("loading");
	const [compatibleSetups, setCompatibleSetups] = useState<CompatibleSetup[]>([]);
	const [setupsState, setSetupsState] = useState<"loading" | "ready" | "error">("loading");
	const [savingSetupId, setSavingSetupId] = useState<string | null>(null);
	const [attachingSetup, setAttachingSetup] = useState(false);
	const [setupComparison, setSetupComparison] = useState<SetupComparison | null>(null);
	const [comparingSetupId, setComparingSetupId] = useState<string | null>(null);
	const [packageAction, setPackageAction] = useState<LapPackageAction | null>(null);
	const [actionsOpen, setActionsOpen] = useState(false);
	const [compareOpen, setCompareOpen] = useState(false);
	const actionsMenu = useRef<HTMLDivElement>(null);
	const metricsByLap = useMemo(() => new Map(metrics.map((value) => [value.lapIndex, value])), [metrics]);
	const overview = sessionOverview(
		session.laps.map((lap) => ({
			id: String(lap.index),
			index: lap.index,
			durationNs: Number.isFinite(lapDuration(lap)) ? lapDuration(lap) : null,
			invalid: lapIsInvalid(lap),
			sectors: lap.sectors.map((sector) => ({ index: sector.index, durationNs: sector.durationNs })),
		})),
	);
	const hasSectorTiming = overview.sectorIndices.length > 0;
	const sectorIndices = overview.sectorIndices;
	const bestLap = session.laps.find((lap) => String(lap.index) === overview.bestLapId);
	const fastestDuration = overview.bestLapDurationNs ?? Number.POSITIVE_INFINITY;
	const theoreticalBest = overview.theoreticalBestDurationNs == null ? null : formatLapDurationNs(overview.theoreticalBestDurationNs);
	const confirmedSetup = compatibleSetups.find((value) => value.confirmed) ?? null;
	const thisSessionIsLive = liveBroadcast?.sourceSessionId === session.id;
	const broadcastBusy = liveBroadcast?.phase === "ending";

	useEffect(() => {
		function dismiss(event: PointerEvent) {
			if (actionsMenu.current && !actionsMenu.current.contains(event.target as Node)) setActionsOpen(false);
		}
		document.addEventListener("pointerdown", dismiss);
		return () => document.removeEventListener("pointerdown", dismiss);
	}, []);

	useEffect(() => {
		let active = true;
		setMetricsState("loading");
		void telemetryDataSource
			.getSessionLapMetrics(session.id)
			.then((values) => {
				if (!active) return;
				setMetrics(values);
				setMetricsState("ready");
			})
			.catch(() => {
				if (active) setMetricsState("error");
			});
		return () => {
			active = false;
		};
	}, [session.id]);

	async function confirmSetup(setup: CompatibleSetup) {
		setSavingSetupId(setup.id);
		try {
			const values = await telemetryDataSource.confirmSessionSetup(session.id, setup.id);
			setCompatibleSetups(values);
			setSetupComparison(null);
			showToast({
				kind: "success",
				title: "Session setup confirmed",
				message: `${setup.name} will be included when this session is exported as .trace.`,
				timeoutMs: 5_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Could not confirm setup", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setSavingSetupId(null);
		}
	}

	async function clearSetup() {
		setSavingSetupId("clear");
		try {
			setCompatibleSetups(await telemetryDataSource.clearSessionSetup(session.id));
			setSetupComparison(null);
			showToast({
				kind: "success",
				title: "Session setup cleared",
				message: "Future .trace exports will not include a setup until another is confirmed.",
				timeoutMs: 5_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Could not clear setup", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setSavingSetupId(null);
		}
	}

	async function compareSetup(baseline: CompatibleSetup, alternative: CompatibleSetup) {
		setComparingSetupId(alternative.id);
		try {
			setSetupComparison(await telemetryDataSource.compareSetups(baseline.id, alternative.id));
		} catch (error) {
			showToast({ kind: "error", title: "Could not compare setups", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setComparingSetupId(null);
		}
	}

	async function attachSetup() {
		setAttachingSetup(true);
		try {
			const savedFolder = savedSetupFolder(session.simulatorId);
			const folder = savedFolder ? { path: savedFolder } : await telemetryDataSource.detectSetupFolder(session.simulatorId);
			if (!folder.path) throw new Error(`Choose the ${session.simulatorName} setups folder on the Setups page first.`);
			const selected = await openDialog({
				multiple: false,
				directory: false,
				title: `Attach a setup to ${session.title ?? session.track}`,
				filters: [{ name: "Setup files", extensions: ["ini"] }],
			});
			if (typeof selected !== "string") return;
			const values = await telemetryDataSource.attachSessionSetup({
				sessionId: session.id,
				setupPath: selected,
				setupsFolder: folder.path,
				overwrite: false,
			});
			setCompatibleSetups(values);
			setSetupComparison(null);
			showToast({
				kind: "success",
				title: "Setup attached",
				message: "The setup is marked as used and will be included in .trace exports.",
				timeoutMs: 6_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Could not attach setup", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setAttachingSetup(false);
		}
	}

	useEffect(() => {
		let active = true;
		setSetupsState("loading");
		void telemetryDataSource
			.getCompatibleSetups(session.id)
			.then((values) => {
				if (!active) return;
				setCompatibleSetups(values);
				setSetupsState("ready");
			})
			.catch(() => {
				if (active) setSetupsState("error");
			});
		return () => {
			active = false;
		};
	}, [session.id]);

	return (
		<>
			<div className="flex items-end justify-between gap-6">
				<div className="min-w-0">
					<SectionHeading index="02">SESSION OVERVIEW</SectionHeading>
					<h1 className="mt-3 truncate text-2xl font-black tracking-[-.02em]">{session.title ?? session.track}</h1>
					<div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
						<p className="text-[13px] text-trace-muted">
							{session.car} · {friendlySessionType(session)} · {formatSessionDate(session.startedAt)}
						</p>
						{(session.ownership !== "unknown" || session.driver) && (
							<span className="hidden h-4 w-px bg-trace-divider sm:block" aria-hidden="true" />
						)}
						{session.ownership !== "unknown" && <OwnershipBadge ownership={session.ownership} />}
						{session.driver && <span className="text-[12px] text-trace-soft">{session.driver}</span>}
					</div>
				</div>
				<div className="flex shrink-0 items-center gap-2">
					<Tooltip content="Save a local .trace package">
						<button
							type="button"
							aria-label={`Save ${session.track} as a TRACE package`}
							disabled={!session.exportable}
							onClick={() => setPackageAction("export")}
							className="grid size-9 place-items-center border border-trace-accent/50 bg-trace-accent-wash text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
						>
							<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
								<path d="M8 2.5v8m0 0 2.8-2.8M8 10.5 5.2 7.7M3 12.5h10" />
							</svg>
						</button>
					</Tooltip>
					<div className="relative" ref={actionsMenu}>
						<Tooltip content="Session actions">
							<button
								type="button"
								aria-label={`More actions for ${session.track}`}
								aria-expanded={actionsOpen}
								onClick={() => setActionsOpen((value) => !value)}
								className="grid size-9 place-items-center border border-trace-divider bg-trace-deep text-trace-muted hover:bg-trace-raised hover:text-white"
							>
								<svg className="size-4 fill-current" viewBox="0 0 16 16" aria-hidden="true">
									<circle cx="3" cy="8" r="1.2" />
									<circle cx="8" cy="8" r="1.2" />
									<circle cx="13" cy="8" r="1.2" />
								</svg>
							</button>
						</Tooltip>
						{actionsOpen && (
							<div className="absolute right-0 top-[calc(100%+6px)] z-30 w-56 border border-trace-divider bg-trace-black p-2 shadow-[0_12px_30px_#000]">
								<span className="block px-2 pb-2 pt-1 text-[12px] font-bold text-trace-soft">Session actions</span>
								<button
									type="button"
									disabled={!session.exportable}
									onClick={() => {
										setActionsOpen(false);
										setPackageAction("share");
									}}
									className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-text hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent"
								>
									Share to cloud…
								</button>
								<button
									type="button"
									disabled={!session.exportable}
									onClick={() => {
										setActionsOpen(false);
										setPackageAction("upload");
									}}
									className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-text hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent"
								>
									Add to private library…
								</button>
							</div>
						)}
					</div>
					<button
						type="button"
						disabled={!bestLap}
						onClick={() => setCompareOpen(true)}
						className="h-9 border border-trace-purple/60 bg-trace-purple/10 px-3 font-mono text-[10px] font-bold tracking-[.08em] text-trace-purple hover:bg-trace-purple hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
					>
						COMPARE
					</button>
					<button
						type="button"
						onClick={onShowSummary}
						className="h-9 border border-trace-divider bg-trace-deep px-3 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft hover:border-trace-purple/70 hover:text-trace-purple"
					>
						SUMMARY
					</button>
					{thisSessionIsLive && liveBroadcast?.spectatorUrl && (
						<>
							<button
								type="button"
								onClick={onCopyLiveLink}
								className="h-9 border border-trace-divider bg-trace-deep px-3 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft hover:border-trace-soft hover:text-white"
							>
								COPY LIVE LINK
							</button>
							<button
								type="button"
								onClick={onOpenLiveLink}
								className="h-9 border border-trace-divider bg-trace-deep px-3 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft hover:border-trace-soft hover:text-white"
							>
								OPEN →
							</button>
						</>
					)}
					<button
						type="button"
						disabled={
							broadcastBusy ||
							(!session.exportable && !thisSessionIsLive) ||
							(!!liveBroadcast && ["live", "reconnecting"].includes(liveBroadcast.phase) && !thisSessionIsLive)
						}
						onClick={
							thisSessionIsLive && ["live", "connecting", "reconnecting"].includes(liveBroadcast?.phase ?? "idle") ? onStopLive : onStartLive
						}
						className={`h-9 border px-3 font-mono text-[10px] font-black tracking-[.08em] disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim ${thisSessionIsLive && liveBroadcast?.phase === "live" ? "border-trace-warning/60 bg-trace-warning/10 text-trace-warning hover:bg-trace-warning hover:text-trace-black" : "border-trace-accent/60 bg-trace-accent-wash text-trace-accent hover:bg-trace-accent hover:text-trace-black"}`}
					>
						{thisSessionIsLive
							? liveBroadcast?.phase === "connecting"
								? "CANCEL"
								: liveBroadcast?.phase === "reconnecting"
									? "RECONNECTING…"
									: liveBroadcast?.phase === "ending"
										? "ENDING…"
										: liveBroadcast?.phase === "live"
											? "STOP LIVE"
											: "STREAM RECORDING"
							: "STREAM RECORDING"}
					</button>
				</div>
			</div>
			{compareOpen && (
				<SessionComparisonPicker
					session={session}
					sessions={sessions}
					onClose={() => setCompareOpen(false)}
					onSelect={(selection) => {
						setCompareOpen(false);
						onCompare(selection);
					}}
				/>
			)}
			{packageAction && <LapPackageDialog session={session} action={packageAction} onClose={() => setPackageAction(null)} />}

			{thisSessionIsLive && liveBroadcast && liveBroadcast.phase !== "idle" && (
				<div className="mt-3 border border-trace-divider bg-trace-deep px-4 py-3">
					<div className="flex items-center justify-between gap-4 font-mono text-[10px] font-bold tracking-[.08em]">
						<span className={liveBroadcast.phase === "error" ? "text-trace-warning" : "text-trace-accent"}>
							{liveBroadcast.phase === "live" ? "LIVE REPLAY" : liveBroadcast.phase.toUpperCase()}
						</span>
						<span className="text-trace-dim">
							{formatBroadcastDuration(liveBroadcast.elapsedNs)} / {formatBroadcastDuration(liveBroadcast.durationNs)}
						</span>
					</div>
					<div className="mt-2 h-1 overflow-hidden bg-trace-divider">
						<div
							className="h-full bg-trace-accent transition-[width] duration-200"
							style={{
								width: `${liveBroadcast.durationNs > 0 ? Math.min(100, (liveBroadcast.elapsedNs / liveBroadcast.durationNs) * 100) : 0}%`,
							}}
						/>
					</div>
					{liveBroadcast.error && <p className="mt-2 text-[11px] leading-4 text-trace-warning">{liveBroadcast.error}</p>}
				</div>
			)}

			<div className="mt-6 grid grid-cols-5 border border-trace-divider bg-trace-surface">
				<Metric label="LAPS" value={String(session.laps.length)} accent />
				<Metric label="FASTEST LAP" value={bestLap?.time ?? "—"} />
				<Metric
					label="THEORETICAL BEST"
					value={theoreticalBest ?? "—"}
					detail="The quickest valid time recorded in each sector, added together."
					purple
				/>
				<Metric label="SOURCE" value={sessionSourceLabel(session).toUpperCase()} />
				<Metric label="SIMULATOR" value={session.simulatorName.toUpperCase()} />
			</div>

			{(session.ambientTemperatureC || session.roadTemperatureC || session.weatherName || session.trackGripPercent != null) && (
				<div className="mt-3 flex flex-wrap items-center gap-x-7 gap-y-2 border border-trace-divider bg-trace-surface px-5 py-3 font-mono text-[11px] text-trace-dim">
					<strong className="tracking-[.1em] text-trace-soft">SESSION CONDITIONS</strong>
					{session.ambientTemperatureC && (
						<span>
							AIR <strong className="text-trace-text">{session.ambientTemperatureC}°C</strong>
						</span>
					)}
					{session.roadTemperatureC && (
						<span>
							TRACK <strong className="text-trace-text">{session.roadTemperatureC}°C</strong>
						</span>
					)}
					{session.trackGripPercent != null && (
						<span>
							STARTING GRIP <strong className="text-trace-text">{session.trackGripPercent}%</strong>
						</span>
					)}
					{session.weatherName && (
						<span>
							WEATHER <strong className="text-trace-text">{friendlyConditionName(session.weatherName)}</strong>
						</span>
					)}
				</div>
			)}

			<CompatibleSetupsDock
				setups={compatibleSetups}
				state={setupsState}
				confirmedSetup={confirmedSetup}
				savingSetupId={savingSetupId}
				comparingSetupId={comparingSetupId}
				comparison={setupComparison}
				attachingSetup={attachingSetup}
				onConfirm={confirmSetup}
				onClear={clearSetup}
				onCompare={compareSetup}
				onAttach={attachSetup}
				onCloseComparison={() => setSetupComparison(null)}
			/>

			{!hasSectorTiming && (
				<div className="mt-4 border border-trace-divider bg-trace-surface px-4 py-3 text-[12px] leading-5 text-trace-muted">
					<strong className="text-trace-soft">No sector timing was emitted for this session.</strong> Lap telemetry and derived metrics remain
					available.
				</div>
			)}
			{hasSectorTiming && session.tags.includes("Estimated sectors") && (
				<div className="mt-4 border-l-2 border-trace-sector-yellow bg-trace-surface px-4 py-3 text-[12px] leading-5 text-trace-muted">
					<strong className="text-trace-soft">Estimated sector splits.</strong> This MoTeC log did not contain official sector crossings, so TRACE
					divided the GPS lap into three equal-distance sections.
				</div>
			)}

			{metricsState === "error" && (
				<div className="mt-4 border border-trace-warning/40 bg-trace-warning/10 px-4 py-3 text-[12px] text-trace-warning">
					Lap times are available, but the additional fuel and speed summaries could not be loaded.
				</div>
			)}

			<div className="mt-4">
				<ConsistencyChart series={[{ label: "This session", session }]} />
			</div>

			<div className="mt-4 border border-trace-divider bg-trace-surface">
				<div className="flex items-center justify-between border-b border-trace-divider px-5 py-4">
					<h2 className="text-[13px] font-black tracking-[.04em]">LAPS</h2>
					<span className="font-mono text-[12px] text-trace-faint">{session.laps.length} TOTAL</span>
				</div>
				<div className="grid grid-cols-[60px_100px_minmax(280px,1fr)_168px_126px_20px] items-center gap-6 border-b border-trace-divider bg-trace-deep px-6 py-3 font-mono text-[12px] font-bold tracking-[.08em] text-trace-dim">
					<span>LAP</span>
					<span>TIME</span>
					<span>SECTORS</span>
					<span className="border-l border-trace-divider pl-6">FUEL</span>
					<span className="border-l border-trace-divider pl-6">TOP SPEED</span>
					<span aria-hidden="true" />
				</div>
				{session.laps.length === 0 ? (
					<div className="p-8 text-center text-[12px] text-trace-dim">No complete laps are available.</div>
				) : (
					session.laps.map((lap) => {
						const lapMetrics = metricsByLap.get(lap.index);
						const invalid = lapIsInvalid(lap);
						const fastest = !invalid && lap.time !== "—" && lapDuration(lap) === fastestDuration;
						return (
							<div
								className={`grid min-h-[108px] grid-cols-[60px_100px_minmax(280px,1fr)_168px_126px_20px] items-center gap-6 border-b border-l-2 border-b-trace-divider px-6 py-4 font-mono text-[12px] last:border-b-0 ${invalid ? "border-l-trace-danger bg-trace-danger/15" : fastest ? "border-l-trace-purple bg-trace-purple/10 shadow-[inset_0_0_28px_rgba(184,124,255,0.04)]" : "border-l-transparent"}`}
								key={lap.index}
								role="button"
								tabIndex={0}
								onClick={() => onOpenLap(lap.index)}
								onKeyDown={(event) => {
									if (event.key === "Enter" || event.key === " ") {
										event.preventDefault();
										onOpenLap(lap.index);
									}
								}}
								aria-label={`Visualize lap ${lap.index}`}
							>
								<Tooltip content={invalid ? lapInvalidityDetail(lap) : null}>
									<span className={invalid ? "text-red-300" : fastest ? "text-trace-purple" : "text-trace-faint"}>
										{String(lap.index).padStart(2, "0")}
									</span>
								</Tooltip>
								<strong className={invalid ? "text-red-200" : fastest ? "text-trace-purple" : "text-trace-text"}>{lap.time}</strong>
								{hasSectorTiming ? (
									<SectorBars lap={lap} laps={session.laps} sectorIndices={sectorIndices} />
								) : (
									<span className="text-[12px] text-trace-dim">UNAVAILABLE</span>
								)}
								<div className="flex min-h-16 items-center border-l border-trace-divider pl-6">
									<FuelUsage state={metricsState} metrics={lapMetrics} />
								</div>
								<div className="flex min-h-16 items-center border-l border-trace-divider pl-6">
									<LapMetricValue
										state={metricsState}
										value={lapMetrics?.maxSpeedKmh != null ? `${lapMetrics.maxSpeedKmh.toFixed(1)} km/h` : null}
									/>
								</div>
								<svg className="size-4 shrink-0 fill-none stroke-current text-trace-dim" viewBox="0 0 16 16" aria-hidden="true">
									<path d="m6 3 5 5-5 5" />
								</svg>
							</div>
						);
					})
				)}
			</div>
			<div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 font-mono text-[12px] text-trace-dim">
				<SectorLegend colour="bg-trace-purple" label="Session best" />
				<SectorLegend colour="bg-trace-accent" label="Improved" />
				<SectorLegend colour="bg-trace-sector-yellow" label="Slower" />
			</div>
		</>
	);
}

function SessionComparisonPicker({
	session,
	sessions,
	onClose,
	onSelect,
}: {
	session: RecordedSessionSummary;
	sessions: RecordedSessionSummary[];
	onClose: () => void;
	onSelect: (selection: ComparisonSelection) => void;
}) {
	const laps = validComparisonLaps(session);
	const [analysedLapIndex, setAnalysedLapIndex] = useState(laps[0]?.index ?? null);
	const analysedLap = laps.find((lap) => lap.index === analysedLapIndex) ?? null;
	const suggestions = fasterReferenceSuggestions(sessions, session, analysedLap, session.id, analysedLapIndex);

	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") onClose();
		};
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);

	return createPortal(
		<div className="fixed inset-0 z-[100] grid place-items-center bg-black/75 p-6" role="presentation" onMouseDown={onClose}>
			<section
				role="dialog"
				aria-modal="true"
				aria-labelledby="session-compare-title"
				className="w-full max-w-3xl border border-trace-divider bg-trace-surface shadow-2xl"
				onMouseDown={(event) => event.stopPropagation()}
			>
				<header className="flex items-start justify-between gap-5 border-b border-trace-divider px-5 py-4">
					<div>
						<p className="font-mono text-[10px] font-bold tracking-[.12em] text-trace-purple">START A COMPARISON</p>
						<h2 id="session-compare-title" className="mt-1 text-lg font-black text-trace-text">
							Choose a faster reference
						</h2>
						<p className="mt-1 text-[12px] leading-5 text-trace-muted">
							Only valid laps from the same simulator, car, track, and layout are shown.
						</p>
					</div>
					<button
						type="button"
						onClick={onClose}
						aria-label="Close comparison picker"
						className="grid size-8 shrink-0 place-items-center border border-trace-divider font-mono text-sm text-trace-muted hover:border-trace-soft hover:text-white"
					>
						×
					</button>
				</header>

				<div className="border-b border-trace-divider bg-trace-deep px-5 py-4">
					<p className="font-mono text-[10px] font-bold tracking-[.1em] text-trace-dim">ANALYSED LAP</p>
					<div className="mt-2 flex flex-wrap gap-2">
						{laps.map((lap) => (
							<button
								type="button"
								key={lap.index}
								onClick={() => setAnalysedLapIndex(lap.index)}
								className={`flex items-center gap-3 border px-3 py-2 font-mono text-[11px] ${lap.index === analysedLapIndex ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider text-trace-muted hover:border-trace-soft hover:text-white"}`}
							>
								<span className="text-trace-dim">LAP {lap.index}</span>
								<strong>{lap.time}</strong>
							</button>
						))}
					</div>
				</div>

				<div className="max-h-[420px] overflow-y-auto">
					{suggestions.length === 0 ? (
						<div className="px-5 py-10 text-center">
							<strong className="text-[13px] text-trace-soft">No faster compatible lap found</strong>
							<p className="mx-auto mt-2 max-w-md text-[12px] leading-5 text-trace-muted">
								Import another driver’s .trace session or record a quicker lap, then it will appear here automatically.
							</p>
						</div>
					) : (
						suggestions.map((suggestion) => {
							const identity = suggestion.session.driver ?? suggestion.session.title ?? "Recorded driver";
							return (
								<button
									type="button"
									key={`${suggestion.session.id}-${suggestion.lap.index}`}
									onClick={() =>
										onSelect({
											referenceSessionId: suggestion.session.id,
											referenceLapIndex: suggestion.lap.index,
											analysedSessionId: session.id,
											analysedLapIndex: analysedLapIndex ?? suggestion.lap.index,
										})
									}
									className="grid w-full grid-cols-[minmax(0,1fr)_110px_90px] items-center gap-4 border-b border-trace-divider px-5 py-4 text-left last:border-b-0 hover:bg-trace-purple-wash"
								>
									<span className="min-w-0">
										<strong className="block truncate text-[13px] text-trace-text">{identity}</strong>
										<span className="mt-1 block truncate font-mono text-[10px] text-trace-dim">
											{suggestion.imported ? "IMPORTED · " : ""}
											{friendlySessionType(suggestion.session)} · {formatCompactSessionDate(suggestion.session.startedAt)}
										</span>
									</span>
									<strong className="font-mono text-[13px] text-trace-purple">{suggestion.lap.time}</strong>
									<span className="font-mono text-right text-[11px] text-trace-accent">−{suggestion.gainSeconds.toFixed(3)}s</span>
								</button>
							);
						})
					)}
				</div>
			</section>
		</div>,
		document.body,
	);
}

function formatBroadcastDuration(nanoseconds: number) {
	if (!Number.isFinite(nanoseconds) || nanoseconds <= 0) return "0:00";
	const seconds = Math.floor(nanoseconds / 1_000_000_000);
	return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function CompatibleSetupsDock({
	setups,
	state,
	confirmedSetup,
	savingSetupId,
	comparingSetupId,
	comparison,
	attachingSetup,
	onConfirm,
	onClear,
	onCompare,
	onAttach,
	onCloseComparison,
}: {
	setups: CompatibleSetup[];
	state: "loading" | "ready" | "error";
	confirmedSetup: CompatibleSetup | null;
	savingSetupId: string | null;
	comparingSetupId: string | null;
	comparison: SetupComparison | null;
	attachingSetup: boolean;
	onConfirm: (setup: CompatibleSetup) => Promise<void>;
	onClear: () => Promise<void>;
	onCompare: (baseline: CompatibleSetup, alternative: CompatibleSetup) => Promise<void>;
	onAttach: () => Promise<void>;
	onCloseComparison: () => void;
}) {
	const [open, setOpen] = useState(false);
	const dock = useRef<HTMLDivElement>(null);

	useEffect(() => {
		if (!open) return;
		const dismiss = (event: PointerEvent) => {
			if (!dock.current?.contains(event.target as Node)) setOpen(false);
		};
		const dismissOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") setOpen(false);
		};
		document.addEventListener("pointerdown", dismiss);
		document.addEventListener("keydown", dismissOnEscape);
		return () => {
			document.removeEventListener("pointerdown", dismiss);
			document.removeEventListener("keydown", dismissOnEscape);
		};
	}, [open]);

	const count = state === "loading" ? "…" : state === "error" ? "!" : String(setups.length);

	return (
		<div className="pointer-events-none sticky top-0 z-40 mt-3 flex h-10 justify-end" ref={dock}>
			<div className="pointer-events-auto relative">
				<button
					type="button"
					onClick={() => setOpen((value) => !value)}
					className={`flex h-10 max-w-[460px] items-center gap-2 border bg-trace-black px-3 font-mono text-[10px] font-bold tracking-[.08em] shadow-[0_8px_20px_rgba(0,0,0,.28)] hover:bg-trace-raised ${open ? "border-trace-accent/60 text-white" : "border-trace-divider text-trace-soft"}`}
					aria-expanded={open}
					aria-controls="compatible-setups-dock"
				>
					<span>COMPATIBLE SETUPS</span>
					<span className={setups.length > 0 ? "text-trace-accent" : state === "error" ? "text-trace-warning" : "text-trace-dim"}>[{count}]</span>
					{confirmedSetup && (
						<span className="min-w-0 truncate border-l border-trace-divider pl-2 font-sans text-[11px] font-semibold normal-case tracking-normal text-white">
							{confirmedSetup.name}
						</span>
					)}
					<svg
						className={`size-3 shrink-0 fill-none stroke-current transition-transform ${open ? "rotate-180" : ""}`}
						viewBox="0 0 12 12"
						aria-hidden="true"
					>
						<path d="m2.5 4 3.5 3.5L9.5 4" />
					</svg>
				</button>

				{open && (
					<aside
						id="compatible-setups-dock"
						className="absolute right-0 top-[calc(100%+.5rem)] flex max-h-[calc(100vh-150px)] w-[min(520px,calc(100vw-var(--trace-sidebar)-56px))] min-w-0 flex-col border border-trace-divider bg-trace-black shadow-[0_18px_48px_rgba(0,0,0,.62)]"
						aria-label="Compatible setups"
					>
						<div className="flex items-start justify-between gap-4 border-b border-trace-divider px-4 py-3">
							<div className="min-w-0">
								<strong className="font-mono text-[11px] tracking-[.08em] text-white">SETUPS FOR THIS SESSION</strong>
								<p className="mt-1 text-[11px] leading-4 text-trace-dim">
									Exact simulator, car, track, and layout matches. Only mark one as used when you know it was loaded.
								</p>
							</div>
							<div className="flex shrink-0 items-center gap-2">
								<button
									type="button"
									disabled={attachingSetup}
									onClick={() => void onAttach()}
									className="h-8 border border-trace-accent bg-trace-accent px-3 font-mono text-[9px] font-black tracking-[.05em] text-trace-black hover:bg-white disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
								>
									{attachingSetup ? "ATTACHING…" : "ATTACH SETUP"}
								</button>
								<button
									type="button"
									onClick={() => setOpen(false)}
									className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-base leading-none text-trace-muted hover:text-white"
									aria-label="Close compatible setups"
								>
									×
								</button>
							</div>
						</div>
						<div className="min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto">
							{state === "ready" && setups.length > 0 ? (
								<div className="divide-y divide-trace-divider">
									{setups.map((setup) => (
										<article
											className={`min-w-0 border-l-2 px-4 py-3 ${setup.confirmed ? "border-l-trace-accent bg-trace-black" : "border-l-transparent bg-trace-deep"}`}
											key={setup.id}
										>
											<div className="flex min-w-0 items-start justify-between gap-3">
												<div className="min-w-0">
													<strong
														className={`block break-words text-[12px] leading-4 ${setup.confirmed ? "text-white" : "text-trace-soft"}`}
													>
														{setup.name}
													</strong>
													<p
														className={`mt-0.5 break-words text-[11px] leading-4 ${setup.confirmed ? "text-trace-soft" : "text-trace-muted"}`}
													>
														{setup.sourceArchive ?? "Local setup"} · imported {formatCompactSessionDate(setup.importedAt)}
													</p>
												</div>
												<span
													className={`shrink-0 border px-1.5 py-1 font-mono text-[9px] font-black leading-none ${setup.confirmed ? "border-trace-accent/70 bg-trace-black text-trace-accent" : "border-trace-divider bg-trace-surface text-trace-muted"}`}
												>
													{setup.confirmed
														? setup.confirmationSource === "package_confirmed"
															? "SHARED AS USED"
															: "USED FOR SESSION"
														: "COMPATIBLE"}
												</span>
											</div>
											<div className="mt-2 flex flex-wrap justify-end gap-1.5">
												{!setup.confirmed && confirmedSetup && (
													<button
														type="button"
														disabled={comparingSetupId != null}
														onClick={() => void onCompare(confirmedSetup, setup)}
														className="h-7 border border-trace-divider bg-transparent px-2.5 font-mono text-[9px] font-bold tracking-[.05em] text-trace-soft hover:border-trace-soft hover:text-white disabled:text-trace-dim"
													>
														{comparingSetupId === setup.id ? "COMPARING…" : "COMPARE TO USED"}
													</button>
												)}
												<button
													type="button"
													disabled={savingSetupId != null}
													onClick={() => (setup.confirmed ? void onClear() : void onConfirm(setup))}
													className={`h-7 border px-2.5 font-mono text-[9px] font-black tracking-[.05em] disabled:text-trace-dim ${setup.confirmed ? "border-trace-divider bg-trace-deep text-white hover:border-trace-soft" : "border-trace-accent bg-trace-accent text-trace-black hover:bg-white"}`}
												>
													{savingSetupId === setup.id || (setup.confirmed && savingSetupId === "clear")
														? "SAVING…"
														: setup.confirmed
															? "CLEAR"
															: "MARK AS USED"}
												</button>
											</div>
										</article>
									))}
								</div>
							) : state === "ready" ? (
								<p className="px-5 py-6 text-[11px] leading-5 text-trace-dim">
									No imported setup matches this session. Import one from the Setups page and it will appear here.
								</p>
							) : state === "error" ? (
								<p className="px-5 py-6 text-[11px] leading-5 text-trace-warning">TRACE could not read the local setup library.</p>
							) : (
								<p className="px-5 py-6 font-mono text-[10px] text-trace-dim">CHECKING SETUP LIBRARY…</p>
							)}
						</div>
						{comparison && (
							<aside
								className="absolute right-full top-0 flex max-h-[calc(100vh-150px)] w-[min(480px,calc(100vw-760px))] min-w-0 flex-col overflow-x-hidden overflow-y-auto border border-r-0 border-trace-divider bg-trace-surface shadow-[-14px_18px_48px_rgba(0,0,0,.5)]"
								aria-label="Setup differences"
							>
								<div className="sticky top-0 z-10 flex items-start justify-between gap-4 border-b border-trace-divider bg-trace-surface px-4 py-3">
									<div className="min-w-0 flex-1">
										<strong className="text-[11px] text-white">SETUP DIFFERENCES</strong>
										<div className="mt-2 grid grid-cols-[minmax(0,1fr)_16px_minmax(0,1fr)] items-start gap-2 text-[10px] leading-4">
											<span className="min-w-0 break-words text-trace-accent">
												<small className="block font-mono text-[8px] font-bold tracking-[.08em] text-trace-dim">USED</small>
												{comparison.baselineName}
											</span>
											<span className="pt-4 text-center text-trace-dim">→</span>
											<span className="min-w-0 break-words text-trace-purple">
												<small className="block font-mono text-[8px] font-bold tracking-[.08em] text-trace-dim">COMPARED</small>
												{comparison.alternativeName}
											</span>
										</div>
									</div>
									<div className="flex shrink-0 items-center gap-2">
										<span className="font-mono text-[9px] text-trace-dim">{comparison.changedValues} CHANGED</span>
										<button
											type="button"
											onClick={onCloseComparison}
											className="grid size-7 place-items-center border border-trace-divider bg-trace-deep text-sm text-trace-muted hover:text-white"
											aria-label="Close setup comparison"
										>
											×
										</button>
									</div>
								</div>
								{comparison.sections.length > 0 ? (
									comparison.sections
										.flatMap((section) => section.changes.map((change) => ({ section: section.name, change })))
										.map(({ section, change }) => (
											<div
												className="flex min-w-0 items-start justify-between gap-5 border-b border-trace-divider px-4 py-2.5 text-[10px]"
												key={`${section}:${change.key}`}
											>
												<span className="min-w-0 flex-1 break-words font-mono font-bold leading-4 text-trace-soft">
													{setupDifferenceLabel(section, change.key)}
												</span>
												<span className="grid w-52 max-w-[60%] min-w-0 shrink grid-cols-[minmax(0,1fr)_14px_minmax(0,1fr)] items-start gap-1 font-mono leading-4">
													<code className="min-w-0 text-right text-trace-accent [overflow-wrap:anywhere]">
														{change.baselineValue ?? "—"}
													</code>
													<span className="text-center text-trace-dim">→</span>
													<code className="min-w-0 text-left text-trace-purple [overflow-wrap:anywhere]">
														{change.alternativeValue ?? "—"}
													</code>
												</span>
											</div>
										))
								) : (
									<p className="px-5 py-4 text-[11px] text-trace-dim">These setups contain the same readable values.</p>
								)}
								<p className="border-t border-trace-divider px-4 py-3 text-[10px] leading-4 text-trace-dim">
									Literal INI differences only. A changed value does not prove why either lap was faster.
								</p>
							</aside>
						)}
					</aside>
				)}
			</div>
		</div>
	);
}
