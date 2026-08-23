import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
	telemetryDataSource,
	type CornerAnalysis,
	type LapComparison,
	type LapComparisonSample,
	type RecordedSessionSummary,
	type SavedComparison,
} from "../../data-source";
import { PageIntro } from "../../components/layout";
import { Tooltip } from "../../Tooltip";
import { useToast } from "../../Toast";
import {
	formatCompactSessionDate,
	formatLapDurationNs,
	formatSessionDate,
	friendlySessionType,
	lapDuration,
	lapIsInvalid,
	sessionSourceGroup,
} from "../sessions/session-components";
import { channelColours, comparisonSeries, ComparisonChart, deltaRange, formatGear, singleSeries, steeringInputRange } from "../telemetry/ComparisonChart";
import { FloatingTrackMap, TrackMap, useTrackMapPip } from "../telemetry/TrackMap";
import {
	filterSamplesByDistance,
	filterSamplesBySector,
	filterSamplesByTelemetryWindow,
	nextTelemetryWindow,
	type TelemetryWindow,
} from "../telemetry/telemetry-window";

export function ComparePage({ sessions }: { sessions: RecordedSessionSummary[] }) {
	const showToast = useToast();
	const eligibleSessions = useMemo(() => sessions.filter((session) => validComparisonLaps(session).length > 0), [sessions]);
	const [referenceSessionId, setReferenceSessionId] = useState("");
	const [comparisonSessionId, setComparisonSessionId] = useState("");
	const [referenceLap, setReferenceLap] = useState<number | null>(null);
	const [comparisonLap, setComparisonLap] = useState<number | null>(null);
	const [comparison, setComparison] = useState<LapComparison | null>(null);
	const [comparisonRequestVersion, setComparisonRequestVersion] = useState(0);
	const [state, setState] = useState<"idle" | "loading" | "ready" | "error">("idle");
	const [error, setError] = useState<string | null>(null);
	const [cursorIndex, setCursorIndex] = useState<number | null>(null);
	const [sector, setSector] = useState<number | null>(null);
	const [telemetryWindow, setTelemetryWindow] = useState<TelemetryWindow | null>(null);
	const [mapZoomLinked, setMapZoomLinked] = useState(false);
	const [cornerIndex, setCornerIndex] = useState<number | null>(null);
	const [analysisCollapsed, setAnalysisCollapsed] = useState(false);
	const [savedComparisons, setSavedComparisons] = useState<SavedComparison[]>([]);
	const mapPip = useTrackMapPip(comparison != null && state === "ready");
	const skipReferenceDefaults = useRef(false);
	const skipComparisonDefaults = useRef(false);
	const referenceSession = eligibleSessions.find((candidate) => candidate.id === referenceSessionId) ?? null;
	const compatibleSessions = useMemo(
		() =>
			referenceSession == null
				? eligibleSessions
				: eligibleSessions.filter(
						(candidate) =>
							candidate.simulatorId === referenceSession.simulatorId &&
							candidate.track === referenceSession.track &&
							candidate.car === referenceSession.car,
					),
		[eligibleSessions, referenceSession],
	);
	const comparisonSession = compatibleSessions.find((candidate) => candidate.id === comparisonSessionId) ?? null;
	const referenceLaps = useMemo(() => (referenceSession ? validComparisonLaps(referenceSession) : []), [referenceSession]);
	const comparisonLaps = useMemo(() => (comparisonSession ? validComparisonLaps(comparisonSession) : []), [comparisonSession]);
	const referenceSuggestions = fasterReferenceSuggestions(
		eligibleSessions,
		comparisonSession ?? undefined,
		comparisonLaps.find((lap) => lap.index === comparisonLap) ?? null,
		comparisonSessionId,
		comparisonLap,
	);

	useEffect(() => {
		void telemetryDataSource
			.getSavedComparisons()
			.then(setSavedComparisons)
			.catch((reason) => {
				showToast({
					kind: "error",
					title: "Saved comparisons unavailable",
					message: reason instanceof Error ? reason.message : String(reason),
					timeoutMs: 8_000,
				});
			});
	}, [showToast]);

	useEffect(() => {
		if (!referenceSessionId && eligibleSessions[0]) {
			const analysedSession = eligibleSessions.find((session) => sessionSourceGroup(session) !== "imported") ?? eligibleSessions[0];
			const analysedLap = validComparisonLaps(analysedSession)[0];
			if (!analysedLap) return;
			const compatible = eligibleSessions.filter(
				(candidate) =>
					candidate.simulatorId === analysedSession.simulatorId && candidate.track === analysedSession.track && candidate.car === analysedSession.car,
			);
			const suggested = fasterReferenceSuggestions(eligibleSessions, analysedSession, analysedLap, analysedSession.id, analysedLap.index)[0];
			const fallback = compatible
				.flatMap((session) =>
					validComparisonLaps(session)
						.filter((lap) => session.id !== analysedSession.id || lap.index !== analysedLap.index)
						.map((lap) => ({ session, lap })),
				)
				.sort((left, right) => lapDuration(left.lap) - lapDuration(right.lap))[0];
			const reference = suggested ?? fallback;
			if (!reference) {
				setReferenceSessionId(analysedSession.id);
				return;
			}
			skipReferenceDefaults.current = true;
			skipComparisonDefaults.current = true;
			setReferenceSessionId(reference.session.id);
			setReferenceLap(reference.lap.index);
			setComparisonSessionId(analysedSession.id);
			setComparisonLap(analysedLap.index);
		}
	}, [eligibleSessions, referenceSessionId]);

	useEffect(() => {
		if (skipReferenceDefaults.current) {
			skipReferenceDefaults.current = false;
			return;
		}
		if (!referenceSession) return;
		const nextReferenceLaps = validComparisonLaps(referenceSession);
		const nextComparisonSession =
			compatibleSessions.find((candidate) => candidate.id === referenceSession.id && validComparisonLaps(candidate).length >= 2) ??
			compatibleSessions.find((candidate) => candidate.id !== referenceSession.id) ??
			compatibleSessions[0];
		setReferenceLap(nextReferenceLaps[0]?.index ?? null);
		setComparisonSessionId(nextComparisonSession?.id ?? "");
		setComparison(null);
		setCursorIndex(null);
	}, [referenceSessionId]);

	useEffect(() => {
		if (skipComparisonDefaults.current) {
			skipComparisonDefaults.current = false;
			return;
		}
		const next = comparisonLaps.find((lap) => comparisonSessionId !== referenceSessionId || lap.index !== referenceLap) ?? null;
		setComparisonLap(next?.index ?? null);
		setComparison(null);
		setCursorIndex(null);
	}, [comparisonSessionId]);

	useEffect(() => {
		if (
			!referenceSessionId ||
			!comparisonSessionId ||
			referenceLap == null ||
			comparisonLap == null ||
			(referenceSessionId === comparisonSessionId && referenceLap === comparisonLap)
		) {
			setState("idle");
			return;
		}
		let active = true;
		setState("loading");
		setError(null);
		void telemetryDataSource
			.compareSessionLaps(referenceSessionId, referenceLap, comparisonSessionId, comparisonLap)
			.then((value) => {
				if (!active) return;
				setComparison(value);
				setSector(null);
				setTelemetryWindow(null);
				setCornerIndex(null);
				setCursorIndex(null);
				setState("ready");
			})
			.catch((reason) => {
				if (!active) return;
				setComparison(null);
				setError(reason instanceof Error ? reason.message : String(reason));
				setState("error");
			});
		return () => {
			active = false;
		};
	}, [comparisonLap, comparisonRequestVersion, comparisonSessionId, referenceLap, referenceSessionId]);

	const corners = comparison?.cornerAnalysis.value?.corners ?? [];
	const finalDelta =
		comparison?.samples
			.slice()
			.reverse()
			.find((sample) => sample.deltaSeconds != null)?.deltaSeconds ?? null;
	const comparisonIsFaster = finalDelta != null && finalDelta < -0.0005;
	const defaultComparisonName = comparison
		? `${referenceSession?.driver ?? "Reference"} ${comparison.referenceLapTime} vs ${comparisonSession?.driver ?? "Analysed"} ${comparison.comparisonLapTime}`.slice(
				0,
				80,
			)
		: "Saved comparison";
	const selectedCorner = corners.find((corner) => corner.index === cornerIndex) ?? null;
	const baseSamples = comparison
		? selectedCorner
			? filterSamplesByDistance(comparison.samples, selectedCorner.startDistanceM, selectedCorner.endDistanceM)
			: filterSamplesBySector(comparison.samples, sector)
		: [];
	const samples = filterSamplesByTelemetryWindow(baseSamples, telemetryWindow);
	const zoomTelemetry = (anchorM: number, direction: "in" | "out") => {
		setTelemetryWindow((current) => nextTelemetryWindow(baseSamples, current, anchorM, direction));
		setCursorIndex(null);
	};

	async function saveCurrentComparison(name: string) {
		if (referenceLap == null || comparisonLap == null) return false;
		try {
			setSavedComparisons(await telemetryDataSource.saveComparison(name, referenceSessionId, referenceLap, comparisonSessionId, comparisonLap));
			showToast({ kind: "success", title: "Comparison saved", message: "You can restore this lap pair from Saved Comparisons.", timeoutMs: 4_500 });
			return true;
		} catch (reason) {
			showToast({
				kind: "error",
				title: "Could not save comparison",
				message: reason instanceof Error ? reason.message : String(reason),
				timeoutMs: 8_000,
			});
			return false;
		}
	}

	async function deleteSavedComparison(comparisonId: string) {
		try {
			setSavedComparisons(await telemetryDataSource.deleteSavedComparison(comparisonId));
			showToast({
				kind: "success",
				title: "Saved comparison removed",
				message: "The shortcut was deleted; its sessions and telemetry were not changed.",
				timeoutMs: 3_500,
			});
		} catch (reason) {
			showToast({
				kind: "error",
				title: "Could not remove comparison",
				message: reason instanceof Error ? reason.message : String(reason),
				timeoutMs: 8_000,
			});
		}
	}

	async function renameSavedComparison(comparisonId: string, name: string) {
		try {
			setSavedComparisons(await telemetryDataSource.renameSavedComparison(comparisonId, name));
			showToast({ kind: "success", title: "Favourite renamed", message: "The saved lap pair has a new name.", timeoutMs: 3_500 });
			return true;
		} catch (reason) {
			showToast({
				kind: "error",
				title: "Could not rename favourite",
				message: reason instanceof Error ? reason.message : String(reason),
				timeoutMs: 8_000,
			});
			return false;
		}
	}

	function openSavedComparison(saved: SavedComparison) {
		const alreadySelected =
			saved.referenceSessionId === referenceSessionId &&
			saved.referenceLapIndex === referenceLap &&
			saved.analysedSessionId === comparisonSessionId &&
			saved.analysedLapIndex === comparisonLap;
		if (alreadySelected) {
			if (comparison == null || state !== "ready") setComparisonRequestVersion((version) => version + 1);
			return;
		}
		skipReferenceDefaults.current = true;
		skipComparisonDefaults.current = true;
		setReferenceSessionId(saved.referenceSessionId);
		setReferenceLap(saved.referenceLapIndex);
		setComparisonSessionId(saved.analysedSessionId);
		setComparisonLap(saved.analysedLapIndex);
		setComparison(null);
		setSector(null);
		setTelemetryWindow(null);
		setCornerIndex(null);
		setCursorIndex(null);
	}

	function selectSuggestedReference(sessionId: string, lapIndex: number) {
		if (sessionId === referenceSessionId && lapIndex === referenceLap) {
			if (comparison == null || state !== "ready") setComparisonRequestVersion((version) => version + 1);
			return;
		}
		skipReferenceDefaults.current = true;
		setReferenceSessionId(sessionId);
		setReferenceLap(lapIndex);
		setComparison(null);
		setTelemetryWindow(null);
		setCornerIndex(null);
		setCursorIndex(null);
	}

	return (
		<>
			<h1 className="sr-only">Lap comparison</h1>
			{eligibleSessions.length === 0 ? (
				<div className="border border-trace-divider bg-trace-surface p-10 text-center">
					<strong className="block text-base">A clean lap is required</strong>
					<p className="mx-auto mt-2 max-w-lg text-[13px] leading-6 text-trace-muted">
						Record or import at least one complete valid lap. A second lap may come from the same session or another visit to the same track.
					</p>
				</div>
			) : (
				<>
					<SavedComparisonsDock
						savedComparisons={savedComparisons}
						suggestions={referenceSuggestions}
						sessions={sessions}
						defaultName={defaultComparisonName}
						canSave={comparison != null && state === "ready"}
						currentReferenceSessionId={referenceSessionId}
						currentReferenceLap={referenceLap}
						currentAnalysedSessionId={comparisonSessionId}
						currentAnalysedLap={comparisonLap}
						onOpen={openSavedComparison}
						onSelectSuggestion={(suggestion) => selectSuggestedReference(suggestion.session.id, suggestion.lap.index)}
						onSave={saveCurrentComparison}
						onDelete={deleteSavedComparison}
						onRename={renameSavedComparison}
					/>
					<CornerAnalysisPanel
						corners={corners}
						selectedCornerIndex={cornerIndex}
						comparisonIsFaster={comparisonIsFaster}
						collapsed={analysisCollapsed}
						onCollapsed={setAnalysisCollapsed}
						onSelect={(value) => {
							setCornerIndex(value === cornerIndex ? null : value);
							setSector(null);
							setTelemetryWindow(null);
							setCursorIndex(null);
						}}
					/>
					<div className={analysisCollapsed ? "ml-14" : "ml-[300px]"}>
						{state === "loading" && (
							<div className="border border-trace-divider bg-trace-surface p-8 font-mono text-[12px] text-trace-dim">
								ALIGNING RECORDED TELEMETRY…
							</div>
						)}
						{state === "idle" && (
							<div className="border border-trace-divider bg-trace-surface p-10 text-center">
								<strong className="text-base">Choose a Reference and an Analysed Lap below</strong>
								<p className="mt-2 text-[13px] text-trace-muted">
									Use a faster clean lap as the Reference, then choose the compatible lap you want to improve—or reopen one from Saved
									Comparisons above the HUD.
								</p>
							</div>
						)}
						{state === "error" && (
							<div className="border border-trace-warning/50 bg-trace-warning/10 p-5 text-[13px] text-trace-warning">
								<strong>Lap analysis unavailable.</strong> {error}
							</div>
						)}
					</div>
					{comparison && state === "ready" && (
						<div>
							<div className="pb-56">
								<div
									className={`grid items-start gap-3 transition-[grid-template-columns] ${analysisCollapsed ? "grid-cols-[44px_minmax(0,1fr)]" : "grid-cols-[288px_minmax(0,1fr)]"}`}
								>
									<div className="w-full"></div>
									<div className="min-w-0">
										{(sector != null || selectedCorner != null || telemetryWindow != null) && (
											<div className="mb-3 flex h-10 items-center justify-between border border-trace-accent/45 bg-trace-accent-wash px-4 font-mono">
												<span className="text-[11px] font-black tracking-[.1em] text-trace-accent">
													VIEWING {selectedCorner ? selectedCorner.label : sector != null ? `SECTOR ${sector}` : "LAP RANGE"}
													{telemetryWindow ? ` · ${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M` : ""}
												</span>
												<button
													type="button"
													onClick={() => {
														setSector(null);
														setTelemetryWindow(null);
														setCornerIndex(null);
														setCursorIndex(null);
													}}
													className="text-[10px] font-bold tracking-[.08em] text-trace-soft hover:text-trace-text"
												>
													RETURN TO FULL LAP
												</button>
											</div>
										)}
										<div className="grid grid-cols-[minmax(480px,560px)_minmax(360px,1fr)] gap-3">
											<div ref={mapPip.anchor}>
												<TrackMap
													samples={samples}
													cursorIndex={cursorIndex}
													comparison
													comparisonIsFaster={comparisonIsFaster}
													height={512}
													trackMap={comparison.trackMap}
													focusSelection={sector != null || selectedCorner != null || telemetryWindow != null}
													rangeLabel={
														selectedCorner?.label ??
														(sector != null
															? `SECTOR ${sector}`
															: telemetryWindow
																? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M`
																: undefined)
													}
													corners={corners}
													selectedCornerIndex={cornerIndex}
													onRangeZoom={zoomTelemetry}
													rangeZoomLinked={mapZoomLinked}
													onRangeZoomLinked={setMapZoomLinked}
												/>
											</div>
											<div className="grid gap-3">
												<ComparisonChart
													label="SPEED"
													unit="km/h"
													samples={samples}
													cursorIndex={cursorIndex}
													onCursor={setCursorIndex}
													onZoom={zoomTelemetry}
													series={comparisonSeries(
														"referenceSpeedKmh",
														"comparisonSpeedKmh",
														channelColours.speed,
														comparisonIsFaster,
													)}
												/>
												<ComparisonChart
													label="GEAR"
													unit=""
													samples={samples}
													cursorIndex={cursorIndex}
													onCursor={setCursorIndex}
													onZoom={zoomTelemetry}
													fixedRange={[-1, 8]}
													series={comparisonSeries("referenceGear", "comparisonGear", channelColours.gear, comparisonIsFaster)}
												/>
											</div>
										</div>
										<div className="mt-3 grid gap-3">
											<ComparisonChart
												label="THROTTLE"
												unit="%"
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												fixedRange={[0, 100]}
												series={comparisonSeries(
													"referenceThrottlePercent",
													"comparisonThrottlePercent",
													channelColours.throttle,
													comparisonIsFaster,
												)}
											/>
											<ComparisonChart
												label="BRAKE"
												unit="%"
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												fixedRange={[0, 100]}
												series={comparisonSeries(
													"referenceBrakePercent",
													"comparisonBrakePercent",
													channelColours.brake,
													comparisonIsFaster,
												)}
											/>
											<ComparisonChart
												label="STEERING INPUT"
												unit="%"
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												fixedRange={steeringInputRange(samples)}
												zeroLine
												series={comparisonSeries(
													"referenceSteeringPercent",
													"comparisonSteeringPercent",
													channelColours.steering,
													comparisonIsFaster,
												)}
											/>
										</div>
										<div className="mt-3 grid grid-cols-2 gap-3">
											<ComparisonChart
												label="ENGINE SPEED"
												unit="rpm"
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												series={comparisonSeries("referenceRpm", "comparisonRpm", channelColours.rpm, comparisonIsFaster)}
											/>
											<ComparisonChart
												label="TIME DIFFERENCE"
												unit="s"
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												fixedRange={deltaRange(samples)}
												series={[
													{
														label: "ANALYSED LAP VS REFERENCE",
														colour: channelColours.delta,
														value: (sample) => sample.deltaSeconds,
													},
												]}
												zeroLine
											/>
										</div>
									</div>
								</div>
							</div>
							{mapPip.visible && (
								<FloatingTrackMap
									samples={samples}
									cursorIndex={cursorIndex}
									comparison
									comparisonIsFaster={comparisonIsFaster}
									trackMap={comparison.trackMap}
									focusSelection={sector != null || selectedCorner != null || telemetryWindow != null}
									rangeLabel={
										selectedCorner?.label ??
										(sector != null
											? `SECTOR ${sector}`
											: telemetryWindow
												? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M`
												: undefined)
									}
									corners={corners}
									selectedCornerIndex={cornerIndex}
									onRangeZoom={zoomTelemetry}
									rangeZoomLinked={mapZoomLinked}
									onRangeZoomLinked={setMapZoomLinked}
									onDismiss={mapPip.dismiss}
								/>
							)}
						</div>
					)}
					<ComparisonHud
						comparison={comparison}
						sessions={eligibleSessions}
						compatibleSessions={compatibleSessions}
						referenceSessionId={referenceSessionId}
						onReferenceSession={setReferenceSessionId}
						referenceLaps={referenceLaps}
						referenceLap={referenceLap}
						onReferenceLap={(value) => {
							setReferenceLap(value);
							if (comparisonSessionId === referenceSessionId && comparisonLap === value)
								setComparisonLap(referenceLaps.find((lap) => lap.index !== value)?.index ?? null);
						}}
						comparisonSessionId={comparisonSessionId}
						onComparisonSession={setComparisonSessionId}
						comparisonLaps={comparisonLaps}
						comparisonLap={comparisonLap}
						onComparisonLap={setComparisonLap}
						onSwap={() => {
							if (referenceLap == null || comparisonLap == null) return;
							skipReferenceDefaults.current = true;
							skipComparisonDefaults.current = true;
							setReferenceSessionId(comparisonSessionId);
							setReferenceLap(comparisonLap);
							setComparisonSessionId(referenceSessionId);
							setComparisonLap(referenceLap);
						}}
						samples={samples}
						sector={sector}
						onSector={(value) => {
							setSector(value);
							setTelemetryWindow(null);
							setCornerIndex(null);
							setCursorIndex(null);
						}}
						cursorIndex={cursorIndex}
						onSeek={setCursorIndex}
					/>
				</>
			)}
		</>
	);
}

function validComparisonLaps(session: RecordedSessionSummary) {
	return session.laps
		.filter((lap) => !lapIsInvalid(lap) && lap.time !== "—")
		.slice()
		.sort((left, right) => lapDuration(left) - lapDuration(right));
}

function SavedComparisonsDock({
	savedComparisons,
	suggestions,
	sessions,
	defaultName,
	canSave,
	currentReferenceSessionId,
	currentReferenceLap,
	currentAnalysedSessionId,
	currentAnalysedLap,
	onOpen,
	onSelectSuggestion,
	onSave,
	onDelete,
	onRename,
}: {
	savedComparisons: SavedComparison[];
	suggestions: ReferenceSuggestion[];
	sessions: RecordedSessionSummary[];
	defaultName: string;
	canSave: boolean;
	currentReferenceSessionId: string;
	currentReferenceLap: number | null;
	currentAnalysedSessionId: string;
	currentAnalysedLap: number | null;
	onOpen: (comparison: SavedComparison) => void;
	onSelectSuggestion: (suggestion: ReferenceSuggestion) => void;
	onSave: (name: string) => Promise<boolean>;
	onDelete: (id: string) => Promise<void>;
	onRename: (id: string, name: string) => Promise<boolean>;
}) {
	const [dockCollapsed, setDockCollapsed] = useState(true);
	const [activeTab, setActiveTab] = useState<"saved" | "suggested">("suggested");
	const [saveOpen, setSaveOpen] = useState(false);
	const [draftName, setDraftName] = useState(defaultName);
	const [saving, setSaving] = useState(false);
	const [menuId, setMenuId] = useState<string | null>(null);
	const [renameId, setRenameId] = useState<string | null>(null);
	const [renameDraft, setRenameDraft] = useState("");
	const [renaming, setRenaming] = useState(false);
	useEffect(() => {
		if (!saveOpen) setDraftName(defaultName);
	}, [defaultName, saveOpen]);
	const currentSaved = savedComparisons.some(
		(saved) =>
			saved.referenceSessionId === currentReferenceSessionId &&
			saved.referenceLapIndex === currentReferenceLap &&
			saved.analysedSessionId === currentAnalysedSessionId &&
			saved.analysedLapIndex === currentAnalysedLap,
	);
	const titlebarActions = typeof document === "undefined" ? null : document.getElementById("trace-titlebar-actions");
	return (
		<>
			{titlebarActions &&
				createPortal(
					<Tooltip content={canSave ? "Save the current lap comparison" : "Choose two laps before saving a comparison"} className="h-12">
						<button
							type="button"
							disabled={!canSave}
							onClick={() => setSaveOpen((value) => !value)}
							className={`grid size-12 place-items-center border-l border-trace-divider bg-trace-black ${saveOpen ? "text-trace-accent" : "text-trace-muted hover:bg-trace-raised hover:text-trace-text"} disabled:cursor-not-allowed disabled:text-trace-dim`}
							aria-label="Save current comparison"
							aria-expanded={saveOpen}
						>
							<svg className={`size-4 stroke-current ${currentSaved ? "fill-trace-accent" : "fill-none"}`} viewBox="0 0 16 16" aria-hidden="true">
								<path d="m8 1.5 1.9 3.85 4.25.62-3.08 3 .73 4.23L8 11.2l-3.8 2 .73-4.23-3.08-3 4.25-.62Z" />
							</svg>
						</button>
					</Tooltip>,
					titlebarActions,
				)}
			{saveOpen && (
				<form
					onSubmit={async (event) => {
						event.preventDefault();
						if (!draftName.trim()) return;
						setSaving(true);
						const saved = await onSave(draftName);
						setSaving(false);
						if (saved) setSaveOpen(false);
					}}
					className="fixed right-[232px] top-12 z-[70] flex w-[380px] items-end gap-3 border border-trace-divider bg-trace-black p-3 shadow-[0_18px_45px_rgba(0,0,0,.6)]"
				>
					<label className="min-w-0 flex-1 font-mono text-[9px] font-bold tracking-[.08em] text-trace-dim" htmlFor="saved-comparison-name">
						COMPARISON NAME
						<input
							id="saved-comparison-name"
							value={draftName}
							onChange={(event) => setDraftName(event.target.value)}
							maxLength={80}
							autoFocus
							className="mt-1.5 h-9 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-sans font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
						/>
					</label>
					<button
						type="button"
						onClick={() => setSaveOpen(false)}
						className="h-9 px-2 font-mono text-[10px] font-bold text-trace-dim hover:text-trace-text"
					>
						CANCEL
					</button>
					<button
						type="submit"
						disabled={saving || !draftName.trim()}
						className="h-9 bg-trace-accent px-4 font-mono text-[10px] font-black text-trace-black disabled:opacity-40"
					>
						{saving ? "SAVING…" : "SAVE"}
					</button>
				</form>
			)}
			<aside
				className="fixed bottom-[239px] right-6 z-[31] w-[620px] max-w-[calc(100vw-248px)] border border-trace-divider bg-trace-black/95 shadow-[0_-12px_35px_rgba(0,0,0,.42)] backdrop-blur"
				aria-label="Comparison lap dock"
			>
				<div className="flex h-10 items-stretch">
					<button
						type="button"
						onClick={() => {
							setActiveTab("saved");
							setDockCollapsed(false);
							setMenuId(null);
						}}
						className={`flex min-w-0 flex-1 items-center gap-1.5 border-r border-trace-divider px-4 text-left font-mono hover:bg-trace-deep ${!dockCollapsed && activeTab === "saved" ? "bg-trace-deep text-trace-text" : "text-trace-soft"}`}
						aria-selected={!dockCollapsed && activeTab === "saved"}
						role="tab"
					>
						<strong className="truncate text-[10px] tracking-[.1em]">SAVED COMPARISONS</strong>
						<span className="shrink-0 text-[10px] font-black text-trace-accent">[{savedComparisons.length}]</span>
					</button>
					<button
						type="button"
						onClick={() => {
							setActiveTab("suggested");
							setDockCollapsed(false);
							setMenuId(null);
						}}
						className={`flex min-w-0 flex-1 items-center gap-1.5 px-4 text-left font-mono hover:bg-trace-deep ${!dockCollapsed && activeTab === "suggested" ? "bg-trace-deep text-trace-text" : "text-trace-soft"}`}
						aria-selected={!dockCollapsed && activeTab === "suggested"}
						role="tab"
					>
						<strong className="truncate text-[10px] tracking-[.1em]">SUGGESTED REFERENCES</strong>
						<span className={`shrink-0 text-[10px] font-black ${suggestions.length > 0 ? "text-trace-purple" : "text-trace-dim"}`}>
							[{suggestions.length}]
						</span>
					</button>
					<button
						type="button"
						onClick={() => {
							setDockCollapsed(!dockCollapsed);
							setMenuId(null);
						}}
						className="grid w-10 shrink-0 place-items-center border-l border-trace-divider text-trace-dim hover:bg-trace-deep hover:text-trace-text"
						aria-label={dockCollapsed ? "Open comparison lap dock" : "Collapse comparison lap dock"}
						aria-expanded={!dockCollapsed}
					>
						<svg
							className={`size-3 fill-none stroke-current transition-transform ${dockCollapsed ? "" : "rotate-180"}`}
							viewBox="0 0 12 12"
							aria-hidden="true"
						>
							<path d="m2.5 4 3.5 3.5L9.5 4" />
						</svg>
					</button>
				</div>
				{!dockCollapsed && (
					<div className="max-h-72 overflow-x-hidden overflow-y-auto border-t border-trace-divider">
						{activeTab === "saved" ? (
							<>
								{savedComparisons.length === 0 ? (
									<div className="px-5 py-8 text-center">
										<strong className="block text-[13px] text-trace-soft">No favourite comparisons yet</strong>
										<p className="mx-auto mt-2 max-w-md text-[11px] leading-5 text-trace-dim">
											Choose a useful Reference and Analysed Lap, then use the title-bar star to save it here.
										</p>
									</div>
								) : (
									<div className="grid gap-2 p-2">
										{savedComparisons.map((saved) => {
											const referenceSession = sessions.find((session) => session.id === saved.referenceSessionId);
											const analysedSession = sessions.find((session) => session.id === saved.analysedSessionId);
											const referenceDriver = savedComparisonDriver(referenceSession);
											const analysedDriver = savedComparisonDriver(analysedSession);
											const current =
												saved.referenceSessionId === currentReferenceSessionId &&
												saved.referenceLapIndex === currentReferenceLap &&
												saved.analysedSessionId === currentAnalysedSessionId &&
												saved.analysedLapIndex === currentAnalysedLap;
											return (
												<article
													className={`relative w-full border bg-trace-surface ${current ? "border-trace-accent/70 outline outline-1 -outline-offset-1 outline-trace-accent/30" : "border-trace-divider hover:border-trace-soft"}`}
													key={saved.id}
												>
													<button
														type="button"
														onClick={() => {
															onOpen(saved);
															setDockCollapsed(true);
														}}
														className="block w-full text-left"
													>
														<div className="flex items-center justify-between gap-3 border-b border-trace-divider px-3 py-1.5 pr-12">
															<span className="min-w-0">
																<strong className="block truncate text-[13px] leading-4 text-trace-text">{saved.name}</strong>
																<span className="mt-0.5 block truncate font-mono text-[10px] font-bold leading-4 text-trace-dim">
																	{saved.track} · {saved.car}
																</span>
															</span>
															{current && (
																<span className="inline-flex h-5 shrink-0 items-center justify-center border border-trace-accent/40 bg-trace-accent-wash px-1.5 font-mono text-[9px] font-black leading-none text-trace-accent">
																	CURRENT
																</span>
															)}
														</div>
														<div className="grid grid-cols-[1fr_auto_1fr] items-stretch gap-2 px-3 py-1.5">
															<SavedComparisonLap
																role="REFERENCE"
																driver={referenceDriver}
																lapIndex={saved.referenceLapIndex}
																durationNs={saved.referenceDurationNs}
																startedAt={saved.referenceStartedAt}
																accent="text-trace-purple"
															/>
															<span className="self-center font-mono text-[9px] font-black text-trace-dim">VS</span>
															<SavedComparisonLap
																role="ANALYSED"
																driver={analysedDriver}
																lapIndex={saved.analysedLapIndex}
																durationNs={saved.analysedDurationNs}
																startedAt={saved.analysedStartedAt}
																accent="text-trace-accent"
																alignRight
															/>
														</div>
														<div className="border-t border-trace-divider px-3 py-1 font-mono text-[9px] font-bold leading-4 text-trace-dim">
															SAVED {formatCompactSessionDate(saved.createdAt)}
														</div>
													</button>
													<div className="absolute right-1.5 top-1.5 z-10">
														<button
															type="button"
															onClick={() => setMenuId((value) => (value === saved.id ? null : saved.id))}
															className="grid size-8 place-items-center border border-transparent text-trace-dim hover:border-trace-divider hover:bg-trace-deep hover:text-trace-text"
															aria-label={`Actions for ${saved.name}`}
															aria-expanded={menuId === saved.id}
														>
															<svg className="h-1 w-3.5 fill-current" viewBox="0 0 14 2" aria-hidden="true">
																<circle cx="1" cy="1" r="1" />
																<circle cx="7" cy="1" r="1" />
																<circle cx="13" cy="1" r="1" />
															</svg>
														</button>
														{menuId === saved.id && (
															<div className="absolute right-0 top-9 w-28 border border-trace-divider bg-trace-black p-1 shadow-[0_10px_25px_rgba(0,0,0,.55)]">
																<button
																	type="button"
																	onClick={() => {
																		setRenameId(saved.id);
																		setRenameDraft(saved.name);
																		setMenuId(null);
																	}}
																	className="block w-full px-2 py-2 text-left font-mono text-[9px] font-bold text-trace-muted hover:bg-trace-deep hover:text-trace-text"
																>
																	RENAME
																</button>
																<button
																	type="button"
																	onClick={() => {
																		setMenuId(null);
																		void onDelete(saved.id);
																	}}
																	className="block w-full px-2 py-2 text-left font-mono text-[9px] font-bold text-[#ff5263] hover:bg-trace-danger/20"
																>
																	DELETE
																</button>
															</div>
														)}
													</div>
													{renameId === saved.id && (
														<form
															onSubmit={async (event) => {
																event.preventDefault();
																if (!renameDraft.trim()) return;
																setRenaming(true);
																const renamed = await onRename(saved.id, renameDraft);
																setRenaming(false);
																if (renamed) setRenameId(null);
															}}
															className="flex items-end gap-2 border-t border-trace-divider bg-trace-deep p-2"
															onClick={(event) => event.stopPropagation()}
														>
															<label
																className="min-w-0 flex-1 font-mono text-[8px] font-bold text-trace-dim"
																htmlFor={`rename-${saved.id}`}
															>
																NEW NAME
																<input
																	id={`rename-${saved.id}`}
																	value={renameDraft}
																	onChange={(event) => setRenameDraft(event.target.value)}
																	maxLength={80}
																	autoFocus
																	className="mt-1 h-8 w-full border border-trace-divider bg-trace-black px-2 font-sans text-[11px] font-normal text-trace-text outline-none focus:border-trace-accent"
																/>
															</label>
															<button
																type="button"
																onClick={() => setRenameId(null)}
																className="h-8 px-2 font-mono text-[9px] font-bold text-trace-dim hover:text-trace-text"
															>
																CANCEL
															</button>
															<button
																type="submit"
																disabled={renaming || !renameDraft.trim()}
																className="h-8 bg-trace-accent px-3 font-mono text-[9px] font-black text-trace-black disabled:opacity-40"
															>
																{renaming ? "…" : "SAVE"}
															</button>
														</form>
													)}
												</article>
											);
										})}
									</div>
								)}
							</>
						) : (
							<SuggestedReferencesList
								suggestions={suggestions}
								currentSessionId={currentReferenceSessionId}
								currentLapIndex={currentReferenceLap}
								onSelect={(suggestion) => {
									onSelectSuggestion(suggestion);
									setDockCollapsed(true);
								}}
							/>
						)}
					</div>
				)}
			</aside>
		</>
	);
}

function SavedComparisonLap({
	role,
	driver,
	lapIndex,
	durationNs,
	startedAt,
	accent,
	alignRight = false,
}: {
	role: string;
	driver: string;
	lapIndex: number;
	durationNs: number;
	startedAt: string;
	accent: string;
	alignRight?: boolean;
}) {
	return (
		<div className={`min-w-0 font-mono ${alignRight ? "text-right" : ""}`}>
			<span className={`block text-[10px] font-black leading-3 tracking-[.08em] ${accent}`}>{role}</span>
			<strong className="mt-0.5 block truncate font-sans text-[13px] leading-4 text-trace-text">{driver}</strong>
			<strong className="mt-0.5 block text-[15px] leading-5 tabular-nums text-trace-soft">{formatLapDurationNs(durationNs)}</strong>
			<span className="block truncate text-[10px] font-bold leading-4 text-trace-dim">
				LAP {lapIndex} · {formatCompactSessionDate(startedAt)}
			</span>
		</div>
	);
}

function savedComparisonDriver(session?: RecordedSessionSummary) {
	return session?.driver?.trim() || session?.title?.trim() || "Unknown driver";
}

function CornerAnalysisPanel({
	corners,
	selectedCornerIndex,
	comparisonIsFaster,
	collapsed,
	onCollapsed,
	onSelect,
}: {
	corners: CornerAnalysis[];
	selectedCornerIndex: number | null;
	comparisonIsFaster: boolean;
	collapsed: boolean;
	onCollapsed: (collapsed: boolean) => void;
	onSelect: (index: number) => void;
}) {
	const opportunities = corners
		.filter((corner) => corner.totalLossSeconds != null && corner.totalLossSeconds > 0.005)
		.slice()
		.sort((left, right) => (right.totalLossSeconds ?? 0) - (left.totalLossSeconds ?? 0))
		.slice(0, 4);
	return (
		<section
			className={`fixed bottom-[252px] left-[204px] top-[76px] z-50 overflow-y-auto border border-trace-divider bg-trace-surface shadow-[0_18px_55px_rgba(0,0,0,.45)] transition-[width] ${collapsed ? "w-11" : "w-72"}`}
			aria-label="Rule-based lap analysis"
		>
			<div
				className={`flex border-b border-trace-divider ${collapsed ? "h-11 items-center justify-center" : "min-h-16 items-center justify-between gap-3 p-3"}`}
			>
				{!collapsed && (
					<div className="min-w-0">
						<strong className="block font-mono text-[11px] tracking-[.1em] text-trace-soft">ANALYSIS</strong>
						<span className="mt-1 block text-[11px] leading-4 text-trace-dim">Rule-based comparison · No AI</span>
						<span className="mt-1 block text-[10px] leading-4 text-trace-faint">
							{comparisonIsFaster ? "Analysed Lap is faster than Reference" : "Analysed Lap against faster Reference"}
						</span>
					</div>
				)}
				<button
					type="button"
					onClick={() => onCollapsed(!collapsed)}
					className="grid size-8 shrink-0 place-items-center text-trace-muted hover:bg-trace-deep hover:text-trace-text"
					aria-label={collapsed ? "Show analysis" : "Hide analysis"}
					aria-expanded={!collapsed}
				>
					<svg
						className={`size-4 fill-none stroke-current transition-transform ${collapsed ? "" : "rotate-180"}`}
						viewBox="0 0 16 16"
						aria-hidden="true"
					>
						<path d="m6 3 5 5-5 5" />
					</svg>
				</button>
			</div>
			{collapsed ? (
				<button
					type="button"
					onClick={() => onCollapsed(false)}
					className="flex w-full items-center justify-center py-4 font-mono text-[10px] font-bold tracking-[.12em] text-trace-dim hover:text-trace-text"
					aria-label="Show analysis"
				>
					<span className="[writing-mode:vertical-rl]">ANALYSIS</span>
				</button>
			) : (
				<div>
					<p className="border-b border-trace-divider bg-trace-warning/5 px-3 py-2 text-[10px] leading-4 text-trace-dim">
						This analysis uses simple telemetry rules and may be incorrect. Verify its suggestions against the graphs and track map.
					</p>
					{opportunities.length === 0 ? (
						<p className="px-4 py-4 text-[12px] leading-5 text-trace-dim">The rule-based comparison did not detect any meaningful corner losses.</p>
					) : (
						<div className="divide-y divide-trace-divider">
							{selectedCornerIndex != null && (
								<button
									type="button"
									onClick={() => onSelect(selectedCornerIndex)}
									className="w-full px-4 py-3 text-left font-mono text-[10px] font-bold tracking-[.08em] text-trace-accent hover:bg-trace-deep hover:text-trace-text"
								>
									SHOW FULL LAP
								</button>
							)}
							{opportunities.map((corner) => {
								const selected = selectedCornerIndex === corner.index;
								const dominant = dominantCornerPhase(corner);
								return (
									<button
										type="button"
										onClick={() => onSelect(corner.index)}
										className={`block w-full min-w-0 px-4 py-3 text-left transition-colors ${selected ? "bg-trace-accent-wash outline outline-1 -outline-offset-1 outline-trace-accent" : "hover:bg-trace-deep"}`}
										aria-pressed={selected}
										key={corner.index}
									>
										<span className="flex items-baseline justify-between gap-3 font-mono">
											<strong className="text-[15px] text-trace-text">{corner.label}</strong>
											<strong className="text-[15px] tabular-nums text-[#ff5263]">+{corner.totalLossSeconds?.toFixed(3)}s</strong>
										</span>
										<span className="mt-2 block truncate text-[10px] font-black tracking-[.08em] text-trace-dim">
											MOST LOSS · {dominant}
										</span>
										<span className="mt-1 block truncate text-[11px] text-trace-muted">{cornerSummary(corner, dominant)}</span>
										<span
											className="mt-3 grid grid-cols-3 gap-1 border-t border-trace-divider pt-2"
											aria-label={`${corner.label} time difference by phase`}
										>
											{corner.phases.map((phase) => (
												<span className="min-w-0" key={phase.phase}>
													<span className="block truncate font-mono text-[9px] font-bold tracking-[.06em] text-trace-dim">
														{cornerPhaseLabel(phase.phase)}
													</span>
													<span
														className={`mt-0.5 block font-mono text-[11px] font-bold tabular-nums ${phase.lossSeconds == null ? "text-trace-dim" : phase.lossSeconds > 0 ? "text-[#ff5263]" : "text-[#42db76]"}`}
													>
														{formatPhaseDelta(phase.lossSeconds)}
													</span>
												</span>
											))}
										</span>
									</button>
								);
							})}
						</div>
					)}
				</div>
			)}
		</section>
	);
}

function cornerPhaseLabel(phase: CornerAnalysis["phases"][number]["phase"]) {
	return phase === "entry" ? "ENTRY" : phase === "mid" ? "MIDDLE" : "EXIT";
}

function formatPhaseDelta(seconds: number | null | undefined) {
	if (seconds == null) return "—";
	if (Math.abs(seconds) < 0.0005) return "±0.000s";
	return `${seconds > 0 ? "+" : "−"}${Math.abs(seconds).toFixed(3)}s`;
}

function dominantCornerPhase(corner: CornerAnalysis) {
	return (
		corner.phases
			.filter((phase) => phase.lossSeconds != null)
			.reduce(
				(dominant, phase) => ((phase.lossSeconds ?? Number.NEGATIVE_INFINITY) > (dominant?.lossSeconds ?? Number.NEGATIVE_INFINITY) ? phase : dominant),
				null as CornerAnalysis["phases"][number] | null,
			)
			?.phase.toUpperCase() ?? "UNAVAILABLE"
	);
}

function cornerSummary(corner: CornerAnalysis, dominantPhase: string) {
	const minimumSpeedDifference =
		corner.metrics.comparisonMinimumSpeedKmh != null && corner.metrics.referenceMinimumSpeedKmh != null
			? corner.metrics.comparisonMinimumSpeedKmh - corner.metrics.referenceMinimumSpeedKmh
			: null;
	if (minimumSpeedDifference != null && minimumSpeedDifference < -1) return `${Math.round(Math.abs(minimumSpeedDifference))} km/h lower minimum speed`;
	const throttleDifference =
		corner.metrics.comparisonThrottlePointM != null && corner.metrics.referenceThrottlePointM != null
			? corner.metrics.comparisonThrottlePointM - corner.metrics.referenceThrottlePointM
			: null;
	if (throttleDifference != null && throttleDifference > 3) return `Throttle applied ${Math.round(throttleDifference)} m later`;
	return `Loss develops through ${dominantPhase.toLowerCase()}`;
}

export function SectorPicker({ samples, value, onChange }: { samples: LapComparisonSample[]; value: number | null; onChange: (value: number | null) => void }) {
	const sectors = [...new Set(samples.flatMap((sample) => (sample.sectorIndex == null ? [] : [sample.sectorIndex])))].sort((left, right) => left - right);
	return (
		<div className="flex items-center gap-2" aria-label="Telemetry range">
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

export function TelemetryHud({
	session,
	lapIndex,
	samples,
	cursorIndex,
	onSeek,
}: {
	session: RecordedSessionSummary;
	lapIndex: number;
	samples: LapComparisonSample[];
	cursorIndex: number | null;
	onSeek: (index: number) => void;
}) {
	const sample = samples[cursorIndex ?? 0] ?? null;
	const airTemperature = sample?.referenceAirTemperatureC ?? numericCondition(session.ambientTemperatureC);
	const trackTemperature = sample?.referenceTrackTemperatureC ?? numericCondition(session.roadTemperatureC);
	const showConditions = hasHudConditions(airTemperature, trackTemperature);
	return (
		<div
			className={`fixed bottom-12 left-[200px] right-6 z-30 grid h-[108px] ${showConditions ? "grid-cols-[minmax(190px,1fr)_95px_130px_90px_100px_130px_130px_150px]" : "grid-cols-[minmax(190px,1fr)_95px_130px_90px_100px_130px_130px]"} grid-rows-[28px_48px] items-center gap-x-4 gap-y-2 overflow-hidden border border-trace-divider bg-trace-black/95 px-5 py-3 shadow-[0_12px_40px_rgba(0,0,0,.55)] backdrop-blur`}
		>
			<div className="col-span-full min-w-0 border-b border-trace-divider pb-2">
				<TelemetrySeek samples={samples} cursorIndex={cursorIndex} onSeek={onSeek} />
			</div>
			<div className="min-w-0">
				<span className="block truncate text-[13px] font-black">
					{session.track} · {session.car}
				</span>
				<span className="font-mono text-[11px] text-trace-dim">
					LAP {lapIndex} · {friendlySessionType(session)}
				</span>
			</div>
			<HudValue label="DISTANCE" value={sample ? `${Math.round(sample.distanceM)} M` : "—"} />
			<HudValue
				label="SPEED / GEAR"
				value={sample?.referenceSpeedKmh == null ? "—" : `${Math.round(sample.referenceSpeedKmh)} · ${formatGear(sample.referenceGear)}`}
				colour={channelColours.speed}
			/>
			<HudValue label="RPM" value={sample?.referenceRpm == null ? "—" : String(Math.round(sample.referenceRpm))} colour={channelColours.rpm} />
			<HudSteering value={sample?.referenceSteeringPercent} colour={channelColours.steering} />
			<HudProgress label="THROTTLE" value={sample?.referenceThrottlePercent} colour={channelColours.throttle} />
			<HudProgress label="BRAKE" value={sample?.referenceBrakePercent} colour={channelColours.brake} />
			{showConditions && <HudConditions air={airTemperature} track={trackTemperature} />}
		</div>
	);
}

type ComparisonHudProps = {
	comparison: LapComparison | null;
	sessions: RecordedSessionSummary[];
	compatibleSessions: RecordedSessionSummary[];
	referenceSessionId: string;
	onReferenceSession: (value: string) => void;
	referenceLaps: RecordedSessionSummary["laps"];
	referenceLap: number | null;
	onReferenceLap: (value: number) => void;
	comparisonSessionId: string;
	onComparisonSession: (value: string) => void;
	comparisonLaps: RecordedSessionSummary["laps"];
	comparisonLap: number | null;
	onComparisonLap: (value: number) => void;
	onSwap: () => void;
	samples: LapComparisonSample[];
	sector: number | null;
	onSector: (value: number | null) => void;
	cursorIndex: number | null;
	onSeek: (index: number) => void;
};

function ComparisonHud({
	comparison,
	sessions,
	compatibleSessions,
	referenceSessionId,
	onReferenceSession,
	referenceLaps,
	referenceLap,
	onReferenceLap,
	comparisonSessionId,
	onComparisonSession,
	comparisonLaps,
	comparisonLap,
	onComparisonLap,
	onSwap,
	samples,
	sector,
	onSector,
	cursorIndex,
	onSeek,
}: ComparisonHudProps) {
	const sample = samples[cursorIndex ?? 0] ?? null;
	const finalDelta = comparison?.samples
		.slice()
		.reverse()
		.find((candidate) => candidate.deltaSeconds != null)?.deltaSeconds;
	const comparisonIsFaster = finalDelta != null && finalDelta < -0.0005;
	const referenceSession = sessions.find((session) => session.id === referenceSessionId);
	const comparisonSession = compatibleSessions.find((session) => session.id === comparisonSessionId);
	const referenceAirTemperature = sample?.referenceAirTemperatureC ?? numericCondition(referenceSession?.ambientTemperatureC);
	const referenceTrackTemperature = sample?.referenceTrackTemperatureC ?? numericCondition(referenceSession?.roadTemperatureC);
	const comparisonAirTemperature = sample?.comparisonAirTemperatureC ?? numericCondition(comparisonSession?.ambientTemperatureC);
	const comparisonTrackTemperature = sample?.comparisonTrackTemperatureC ?? numericCondition(comparisonSession?.roadTemperatureC);
	const referenceHasConditions = hasHudConditions(referenceAirTemperature, referenceTrackTemperature);
	const comparisonHasConditions = hasHudConditions(comparisonAirTemperature, comparisonTrackTemperature);
	const showConditions = referenceHasConditions || comparisonHasConditions;
	const sectorDeltas = comparisonSectorDeltas(referenceLaps, referenceLap, comparisonLaps, comparisonLap, comparison?.samples ?? []);
	const gapTone =
		sample?.deltaSeconds == null || Math.abs(sample.deltaSeconds) < 0.0005
			? "text-trace-text"
			: sample.deltaSeconds > 0
				? "text-[#ff5263]"
				: "text-[#42db76]";
	const referenceLabel = referenceSession?.driver?.trim() || "REFERENCE";
	const comparisonLabel = comparisonSession?.driver?.trim() || "ANALYSED LAP";
	const trackCarLabel = referenceSession ? `${referenceSession.track} · ${referenceSession.car}` : "TRACK · CAR";
	return (
		<>
			<div
				className={`fixed bottom-12 left-[200px] right-6 z-30 grid h-[192px] ${showConditions ? "grid-cols-[120px_72px_minmax(160px,260px)_112px_minmax(120px,1fr)_112px_minmax(160px,260px)_72px_120px]" : "grid-cols-[120px_72px_minmax(180px,320px)_minmax(120px,1fr)_minmax(180px,320px)_72px_120px]"} grid-rows-[28px_45px_44px_27px] items-center justify-center gap-x-3 gap-y-2 overflow-hidden border border-trace-divider bg-trace-black/95 px-5 py-3 shadow-[0_12px_40px_rgba(0,0,0,.55)] backdrop-blur`}
			>
				<div className="col-span-full min-w-0 border-b border-trace-divider pb-2">
					<TelemetrySeek samples={samples} cursorIndex={cursorIndex} onSeek={onSeek} />
				</div>
				<div className="col-span-full grid grid-cols-[1fr_52px_1fr] gap-3 border-b border-trace-divider pb-2">
					<HudLapChoice
						label={referenceLabel}
						role={comparisonIsFaster ? "SLOWER BASELINE" : "FASTER REFERENCE"}
						colour={comparisonIsFaster ? "text-trace-accent" : "text-trace-purple"}
						sessions={sessions}
						sessionId={referenceSessionId}
						onSession={onReferenceSession}
						laps={referenceLaps}
						lapIndex={referenceLap}
						onLap={onReferenceLap}
					/>
					<div className="flex min-w-0 items-center justify-center">
						<button
							type="button"
							disabled={referenceLap == null || comparisonLap == null}
							onClick={onSwap}
							className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-deep text-trace-muted hover:border-trace-soft hover:text-trace-text disabled:text-trace-dim"
							aria-label="Swap Reference and Analysed Lap"
						>
							<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
								<path d="M3 5h9m0 0L9.5 2.5M12 5 9.5 7.5M13 11H4m0 0 2.5-2.5M4 11l2.5 2.5" />
							</svg>
						</button>
					</div>
					<HudLapChoice
						label={comparisonLabel}
						role={comparisonIsFaster ? "FASTER SELECTED LAP" : "LAP TO IMPROVE"}
						colour={comparisonIsFaster ? "text-trace-purple" : "text-trace-accent"}
						sessions={compatibleSessions}
						sessionId={comparisonSessionId}
						onSession={onComparisonSession}
						laps={comparisonLaps}
						lapIndex={comparisonLap}
						onLap={onComparisonLap}
						disabledLap={comparisonSessionId === referenceSessionId ? referenceLap : null}
					/>
				</div>
				<HudValue
					label="REFERENCE SPEED / GEAR"
					value={sample?.referenceSpeedKmh == null ? "—" : `${Math.round(sample.referenceSpeedKmh)} · ${formatGear(sample.referenceGear)}`}
					colour={comparisonIsFaster ? "var(--color-trace-accent)" : channelColours.faster}
				/>
				<HudSteering value={sample?.referenceSteeringPercent} colour={comparisonIsFaster ? "var(--color-trace-accent)" : channelColours.faster} />
				<HudPedals throttle={sample?.referenceThrottlePercent} brake={sample?.referenceBrakePercent} />
				{showConditions &&
					(referenceHasConditions ? <HudConditions air={referenceAirTemperature} track={referenceTrackTemperature} /> : <div aria-hidden="true" />)}
				<div className="h-10 min-w-0 overflow-hidden border-x border-trace-divider px-3 text-center font-mono">
					<span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">LIVE GAP</span>
					<strong className={`mt-1 block truncate text-[13px] leading-5 tabular-nums ${gapTone}`}>
						{sample == null ? "—" : `${Math.round(sample.distanceM)} M · ${formatComparisonGap(sample.deltaSeconds)}`}
					</strong>
				</div>
				{showConditions &&
					(comparisonHasConditions ? (
						<HudConditions air={comparisonAirTemperature} track={comparisonTrackTemperature} />
					) : (
						<div aria-hidden="true" />
					))}
				<HudPedals throttle={sample?.comparisonThrottlePercent} brake={sample?.comparisonBrakePercent} />
				<HudSteering value={sample?.comparisonSteeringPercent} colour={comparisonIsFaster ? channelColours.faster : "var(--color-trace-accent)"} />
				<HudValue
					label="ANALYSED SPEED / GEAR"
					value={sample?.comparisonSpeedKmh == null ? "—" : `${Math.round(sample.comparisonSpeedKmh)} · ${formatGear(sample.comparisonGear)}`}
					colour={comparisonIsFaster ? channelColours.faster : "var(--color-trace-accent)"}
				/>
				<div className="col-span-full min-w-0 border-t border-trace-divider pt-2">
					<ComparisonSectorStrip sectors={sectorDeltas} value={sector} onChange={onSector} trackCarLabel={trackCarLabel} />
				</div>
			</div>
		</>
	);
}

type ReferenceSuggestion = {
	session: RecordedSessionSummary;
	lap: RecordedSessionSummary["laps"][number];
	imported: boolean;
	gainSeconds: number;
};

function fasterReferenceSuggestions(
	sessions: RecordedSessionSummary[],
	targetSession: RecordedSessionSummary | undefined,
	targetLap: RecordedSessionSummary["laps"][number] | null,
	targetSessionId: string,
	targetLapIndex: number | null,
) {
	if (!targetSession || !targetLap) return [];
	const targetDuration = lapDuration(targetLap);
	return sessions
		.filter((session) => session.simulatorId === targetSession.simulatorId && session.track === targetSession.track && session.car === targetSession.car)
		.flatMap((session) =>
			validComparisonLaps(session)
				.filter((lap) => lapDuration(lap) < targetDuration && (session.id !== targetSessionId || lap.index !== targetLapIndex))
				.map((lap): ReferenceSuggestion => ({
					session,
					lap,
					imported: sessionSourceGroup(session) === "imported",
					gainSeconds: (targetDuration - lapDuration(lap)) / 1_000_000_000,
				})),
		)
		.sort((left, right) => Number(right.imported) - Number(left.imported) || lapDuration(left.lap) - lapDuration(right.lap))
		.slice(0, 10);
}

function SuggestedReferencesList({
	suggestions,
	currentSessionId,
	currentLapIndex,
	onSelect,
}: {
	suggestions: ReferenceSuggestion[];
	currentSessionId: string;
	currentLapIndex: number | null;
	onSelect: (suggestion: ReferenceSuggestion) => void;
}) {
	return (
		<>
			<p className="border-b border-trace-divider px-4 py-2 text-[11px] leading-4 text-trace-dim">
				Clean laps that are faster than the Analysed Lap. Imported references are listed first.
			</p>
			{suggestions.length === 0 ? (
				<p className="px-4 py-4 text-[12px] text-trace-muted">
					No faster lap with the same simulator, car, track, and layout has been recorded or imported.
				</p>
			) : (
				suggestions.map((suggestion) => {
					const identity = suggestion.session.driver ?? suggestion.session.title ?? formatSessionDate(suggestion.session.startedAt);
					const current = suggestion.session.id === currentSessionId && suggestion.lap.index === currentLapIndex;
					return (
						<button
							type="button"
							onClick={() => onSelect(suggestion)}
							className={`grid w-full grid-cols-[minmax(0,1fr)_100px_82px] items-center gap-4 border-b border-trace-divider px-4 py-3 text-left last:border-b-0 ${current ? "bg-trace-purple-wash" : "hover:bg-trace-deep"}`}
							aria-current={current ? "true" : undefined}
							key={`${suggestion.session.id}-${suggestion.lap.index}`}
						>
							<span className="min-w-0">
								<strong className="block truncate text-[12px] text-trace-text">{identity}</strong>
								<span className="mt-1 flex items-center gap-2 font-mono text-[9px] font-bold tracking-[.07em] text-trace-dim">
									{suggestion.imported && <span className="text-trace-purple">IMPORTED</span>}
									<span>{suggestion.session.sessionType}</span>
									<span>{formatSessionDate(suggestion.session.startedAt)}</span>
									{current && <span className="text-trace-accent">CURRENT</span>}
								</span>
							</span>
							<span className="font-mono text-right">
								<strong className="block text-[12px] text-trace-soft">{suggestion.lap.time}</strong>
								<span className="mt-1 block text-[9px] text-trace-dim">LAP {suggestion.lap.index}</span>
							</span>
							<strong className="font-mono text-right text-[12px] tabular-nums text-trace-purple">−{suggestion.gainSeconds.toFixed(3)}s</strong>
						</button>
					);
				})
			)}
		</>
	);
}

type SectorDelta = { index: number; seconds: number | null };

function comparisonSectorDeltas(
	referenceLaps: RecordedSessionSummary["laps"],
	referenceLap: number | null,
	comparisonLaps: RecordedSessionSummary["laps"],
	comparisonLap: number | null,
	samples: LapComparisonSample[],
): SectorDelta[] {
	const reference = referenceLaps.find((lap) => lap.index === referenceLap);
	const comparison = comparisonLaps.find((lap) => lap.index === comparisonLap);
	const indices = [
		...new Set([
			...(reference?.sectors.map((sector) => sector.index) ?? []),
			...(comparison?.sectors.map((sector) => sector.index) ?? []),
			...samples.flatMap((sample) => (sample.sectorIndex == null ? [] : [sample.sectorIndex])),
		]),
	].sort((left, right) => left - right);
	const cumulativeDeltas = new Map(
		indices.map((index) => [
			index,
			samples
				.slice()
				.reverse()
				.find((sample) => sample.sectorIndex === index && sample.deltaSeconds != null)?.deltaSeconds ?? null,
		]),
	);
	return indices.map((index, position) => {
		const referenceSector = reference?.sectors.find((sector) => sector.index === index);
		const comparisonSector = comparison?.sectors.find((sector) => sector.index === index);
		if (referenceSector && comparisonSector) {
			return { index, seconds: (comparisonSector.durationNs - referenceSector.durationNs) / 1_000_000_000 };
		}
		const cumulativeDelta = cumulativeDeltas.get(index) ?? null;
		const previousDelta = position === 0 ? 0 : (cumulativeDeltas.get(indices[position - 1]) ?? null);
		const seconds = cumulativeDelta == null || previousDelta == null ? null : cumulativeDelta - previousDelta;
		return { index, seconds };
	});
}

function ComparisonSectorStrip({
	sectors,
	value,
	onChange,
	trackCarLabel,
}: {
	sectors: SectorDelta[];
	value: number | null;
	onChange: (value: number | null) => void;
	trackCarLabel: string;
}) {
	return (
		<div className="flex h-6 min-w-0 items-stretch gap-3 font-mono" aria-label="Sector comparison and telemetry range">
			<div className="flex min-w-0 flex-1 items-stretch gap-1.5 overflow-x-auto">
				<span className="flex w-28 shrink-0 items-center text-[9px] font-black tracking-[.08em] text-trace-accent">
					{value == null ? "VIEWING FULL LAP" : `VIEWING SECTOR ${value}`}
				</span>
				<button
					type="button"
					onClick={() => onChange(null)}
					className={`shrink-0 border px-2 text-[9px] font-black leading-none tracking-[.08em] ${value == null ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}
				>
					LAP
				</button>
				{sectors.map((sector) => {
					const gaining = sector.seconds != null && sector.seconds < -0.0005;
					const losing = sector.seconds != null && sector.seconds > 0.0005;
					const tone = gaining
						? "border-trace-accent/45 bg-trace-accent/10 text-trace-accent"
						: losing
							? "border-trace-danger/60 bg-trace-danger/25 text-red-200"
							: "border-trace-divider bg-trace-deep text-trace-muted";
					const selected = value === sector.index ? "outline outline-2 -outline-offset-2 outline-trace-accent" : "hover:border-trace-soft";
					const explanation =
						sector.seconds == null
							? `Sector ${sector.index} timing is unavailable.`
							: Math.abs(sector.seconds) < 0.0005
								? `Sector ${sector.index} was even with the reference.`
								: `The Analysed Lap ${gaining ? "gained" : "lost"} ${Math.abs(sector.seconds).toFixed(3)} seconds against the Reference in sector ${sector.index}.`;
					return (
						<Tooltip content={explanation} key={sector.index}>
							<button
								type="button"
								onClick={() => onChange(sector.index)}
								className={`flex shrink-0 items-center gap-1.5 border px-2 text-[10px] font-bold leading-none tabular-nums ${tone} ${selected}`}
								aria-pressed={value === sector.index}
							>
								<span className="text-[9px] opacity-75">S{sector.index}</span>
								<strong>
									{sector.seconds == null
										? "—"
										: Math.abs(sector.seconds) < 0.0005
											? "0.000"
											: `${sector.seconds > 0 ? "+" : "−"}${Math.abs(sector.seconds).toFixed(3)}`}
								</strong>
							</button>
						</Tooltip>
					);
				})}
			</div>
			<Tooltip content={trackCarLabel} className="min-w-0 max-w-[38%] shrink-0 items-center justify-end">
				<span className="truncate text-right text-[10px] font-bold tracking-[.06em] text-trace-soft">{trackCarLabel}</span>
			</Tooltip>
		</div>
	);
}

function HudLapChoice({
	label,
	role,
	colour,
	sessions,
	sessionId,
	onSession,
	laps,
	lapIndex,
	onLap,
	disabledLap = null,
}: {
	label: string;
	role: string;
	colour: string;
	sessions: RecordedSessionSummary[];
	sessionId: string;
	onSession: (value: string) => void;
	laps: RecordedSessionSummary["laps"];
	lapIndex: number | null;
	onLap: (value: number) => void;
	disabledLap?: number | null;
}) {
	return (
		<div className="grid min-w-0 grid-cols-[112px_minmax(150px,1fr)_150px] items-center gap-2">
			<span className="min-w-0 font-mono">
				<strong className={`block truncate text-[10px] font-black leading-3 tracking-[.1em] ${colour}`}>{label}</strong>
				<span className="mt-0.5 block truncate text-[8px] font-bold leading-3 tracking-[.07em] text-trace-dim">{role}</span>
			</span>
			<select
				value={sessionId}
				onChange={(event) => onSession(event.target.value)}
				className="trace-select h-9 min-w-0 border border-trace-divider bg-trace-deep px-3 text-[11px] font-bold leading-none text-trace-text outline-none"
				aria-label={`${label} session`}
			>
				{sessions.map((session) => (
					<option value={session.id} key={session.id}>
						{comparisonSessionLabel(session)}
					</option>
				))}
			</select>
			<select
				value={lapIndex?.toString() ?? ""}
				onChange={(event) => onLap(Number(event.target.value))}
				className="trace-select h-9 border border-trace-divider bg-trace-deep px-3 font-mono text-[11px] font-bold leading-none text-trace-text outline-none"
				aria-label={`${label} lap`}
			>
				{lapIndex == null && (
					<option value="" disabled>
						No clean lap
					</option>
				)}
				{laps.map((lap) => (
					<option value={lap.index} disabled={lap.index === disabledLap} key={lap.index}>
						Lap {lap.index} · {lap.time}
					</option>
				))}
			</select>
		</div>
	);
}

function comparisonSessionLabel(session: RecordedSessionSummary) {
	const identity = session.driver ?? session.title ?? "Unnamed session";
	return `${session.car} @ ${session.track} · ${identity} · ${friendlySessionType(session)} · ${formatCompactSessionDate(session.startedAt)}`;
}

function formatComparisonGap(seconds?: number | null) {
	if (seconds == null) return "GAP —";
	if (Math.abs(seconds) < 0.0005) return "EVEN";
	return `${Math.abs(seconds).toFixed(3)}s ${seconds > 0 ? "BEHIND" : "AHEAD"}`;
}

function TelemetrySeek({ samples, cursorIndex, onSeek }: { samples: LapComparisonSample[]; cursorIndex: number | null; onSeek: (index: number) => void }) {
	const index = Math.min(Math.max(cursorIndex ?? 0, 0), Math.max(samples.length - 1, 0));
	const start = samples[0]?.distanceM ?? 0;
	const end = samples.at(-1)?.distanceM ?? 0;
	return (
		<label className="grid h-6 grid-cols-[72px_1fr_76px] items-center gap-3 font-mono text-[10px] tabular-nums text-trace-dim">
			<span>{Math.round(start)} M</span>
			<input
				className="trace-seek w-full"
				type="range"
				min="0"
				max={Math.max(samples.length - 1, 0)}
				step="1"
				value={index}
				disabled={samples.length < 2}
				onChange={(event) => onSeek(Number(event.target.value))}
				aria-label="Seek through lap distance"
			/>
			<span className="text-right">{Math.round(end)} M</span>
		</label>
	);
}

function HudValue({ label, value, unit, colour }: { label: string; value: string; unit?: string; colour?: string }) {
	return (
		<div className="h-10 min-w-0 overflow-hidden font-mono">
			<span className="block truncate whitespace-nowrap text-[10px] font-bold leading-3 tracking-[.1em] text-trace-dim">{label}</span>
			<strong className="mt-1 block truncate whitespace-nowrap text-[15px] leading-5 tabular-nums" style={{ color: colour }}>
				{value}
				{unit && <small className="ml-1 text-[9px] text-trace-dim">{unit}</small>}
			</strong>
		</div>
	);
}

function HudSteering({ value, colour }: { value?: number | null; colour: string }) {
	const percent = value == null || !Number.isFinite(value) ? 0 : Math.min(100, Math.max(-100, value));
	const rotation = percent * 4.5;
	return (
		<div className="flex h-10 min-w-0 items-center gap-2 font-mono">
			<svg
				className="size-9 shrink-0"
				viewBox="0 0 36 36"
				role="img"
				aria-label={value == null ? "Steering unavailable" : `Steering input ${Math.round(value)} percent`}
			>
				<circle cx="18" cy="18" r="15" fill="var(--color-trace-deep)" stroke={colour} strokeWidth="2" />
				<g transform={`rotate(${rotation} 18 18)`} stroke={colour} strokeWidth="2" strokeLinecap="round">
					<line x1="18" y1="18" x2="18" y2="31" />
					<line x1="7" y1="17" x2="29" y2="17" />
				</g>
				<circle cx="18" cy="18" r="2.5" fill={colour} />
			</svg>
			<span className="min-w-0">
				<span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">STEER</span>
				<strong className="block truncate text-[12px] leading-4 tabular-nums" style={{ color: colour }}>
					{value == null ? "—" : `${Math.round(value)}%`}
				</strong>
			</span>
		</div>
	);
}

function HudConditions({ air, track }: { air?: number | null; track?: number | null }) {
	return (
		<div className="h-10 min-w-0 overflow-hidden font-mono">
			<span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">CONDITIONS</span>
			<span className="mt-1 flex items-center gap-2 whitespace-nowrap text-[10px] leading-4 tabular-nums">
				{air != null && (
					<span className="text-trace-dim">
						AIR <strong className="text-trace-text">{formatHudTemperature(air)}</strong>
					</span>
				)}
				{track != null && (
					<span className="text-trace-dim">
						TRACK <strong className="text-trace-text">{formatHudTemperature(track)}</strong>
					</span>
				)}
			</span>
		</div>
	);
}

function hasHudConditions(air?: number | null, track?: number | null) {
	return (air != null && Number.isFinite(air)) || (track != null && Number.isFinite(track));
}

function HudProgress({ label, value, colour }: { label: string; value?: number | null; colour: string }) {
	const primary = Math.min(100, Math.max(0, value ?? 0));
	return (
		<div className="h-10 overflow-hidden font-mono">
			<span className="flex h-3 justify-between whitespace-nowrap text-[10px] font-bold leading-3 tracking-[.08em] tabular-nums text-trace-dim">
				<span className="truncate">{label}</span>
				<span>{Math.round(primary)}%</span>
			</span>
			<span className="mt-2 block h-2 overflow-hidden bg-trace-divider">
				<span className="block h-full transition-[width] duration-75" style={{ width: `${primary}%`, backgroundColor: colour }} />
			</span>
		</div>
	);
}

function HudPedals({ throttle, brake }: { throttle?: number | null; brake?: number | null }) {
	return (
		<div className="grid h-10 w-full min-w-0 max-w-80 grid-cols-2 gap-3 justify-self-center">
			<HudProgress label="THROTTLE" value={throttle} colour={channelColours.throttle} />
			<HudProgress label="BRAKE" value={brake} colour={channelColours.brake} />
		</div>
	);
}

function formatHudTemperature(value?: number | null) {
	return value == null ? "—" : `${Math.round(value)}°C`;
}

function numericCondition(value?: string | null) {
	if (!value) return null;
	const parsed = Number(value);
	return Number.isFinite(parsed) ? parsed : null;
}
