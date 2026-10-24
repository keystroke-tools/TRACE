import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
	telemetryDataSource,
	type CornerAnalysis,
	type DiscordReviewActivity,
	type DrivingObservation,
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
	formatSessionType,
	lapDuration,
	lapIsInvalid,
	sessionSourceGroup,
} from "../sessions/session-components";
import { channelColours, comparisonSeries, ComparisonChart, deltaRange, formatGear, singleSeries, steeringAngleRange } from "../telemetry/ComparisonChart";
import { FloatingTrackMap, TrackMap, useTrackMapPip } from "../telemetry/TrackMap";
import { ComparisonSessionPicker } from "./ComparisonSessionPicker";
import { fasterReferenceSuggestions, type ComparisonSelection, type ReferenceSuggestion, validComparisonLaps } from "./comparison-selection";
import { FEATURE_FLAGS_CHANGED_EVENT, richComparisonPickerEnabled } from "../settings/feature-flags";
import {
	ComparisonTelemetryHud,
	comparisonSectorDeltas as sharedComparisonSectorDeltas,
	nearestTelemetryPlaybackIndex,
	SectorPicker,
	telemetryPlaybackSpeeds,
	telemetryPlaybackTimeline,
} from "@trace/ui";
import {
	filterSamplesByDistance,
	filterSamplesBySector,
	filterSamplesByTelemetryWindow,
	nextTelemetryWindow,
	type TelemetryWindow,
} from "../telemetry/telemetry-window";

export function ComparePage({
	sessions,
	onReviewActivity,
	initialSelection,
}: {
	sessions: RecordedSessionSummary[];
	onReviewActivity: (activity: DiscordReviewActivity | null) => void;
	initialSelection?: ComparisonSelection | null;
}) {
	const showToast = useToast();
	const eligibleSessions = useMemo(() => sessions.filter((session) => validComparisonLaps(session).length > 0), [sessions]);
	const [referenceSessionId, setReferenceSessionId] = useState(initialSelection?.referenceSessionId ?? "");
	const [comparisonSessionId, setComparisonSessionId] = useState(initialSelection?.analysedSessionId ?? "");
	const [referenceLap, setReferenceLap] = useState<number | null>(initialSelection?.referenceLapIndex ?? null);
	const [comparisonLap, setComparisonLap] = useState<number | null>(initialSelection?.analysedLapIndex ?? null);
	const [comparison, setComparison] = useState<LapComparison | null>(null);
	const [comparisonRequestVersion, setComparisonRequestVersion] = useState(0);
	const [state, setState] = useState<"idle" | "loading" | "ready" | "error">("idle");
	const [error, setError] = useState<string | null>(null);
	const [cursorIndex, setCursorIndex] = useState<number | null>(null);
	const [comparisonPlaying, setComparisonPlaying] = useState(false);
	const [playbackSpeed, setPlaybackSpeed] = useState(1);
	const [sector, setSector] = useState<number | null>(null);
	const [telemetryWindow, setTelemetryWindow] = useState<TelemetryWindow | null>(null);
	const [mapZoomLinked, setMapZoomLinked] = useState(false);
	const [cornerIndex, setCornerIndex] = useState<number | null>(null);
	const [analysisCollapsed, setAnalysisCollapsed] = useState(false);
	const [savedComparisons, setSavedComparisons] = useState<SavedComparison[]>([]);
	const mapPip = useTrackMapPip(comparison != null && state === "ready");
	const skipReferenceDefaults = useRef(initialSelection != null);
	const skipComparisonDefaults = useRef(initialSelection != null);
	const playbackAnimationFrame = useRef(0);
	const playbackStartIndex = useRef(0);
	const playbackSpeedRef = useRef(1);
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
		const context = referenceSession ?? comparisonSession;
		onReviewActivity({
			kind: "comparison",
			simulator: context?.simulatorId,
			track: context?.track,
			car: context?.car,
			sessionType: context?.sessionType,
		});
	}, [
		comparisonSession?.car,
		comparisonSession?.id,
		comparisonSession?.sessionType,
		comparisonSession?.simulatorId,
		comparisonSession?.track,
		onReviewActivity,
		referenceSession?.car,
		referenceSession?.id,
		referenceSession?.sessionType,
		referenceSession?.simulatorId,
		referenceSession?.track,
	]);

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
		if (!initialSelection && !referenceSessionId && eligibleSessions[0]) {
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
	}, [eligibleSessions, initialSelection, referenceSessionId]);

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
	const drivingObservations = comparison?.drivingAnalysis?.value?.observations ?? [];
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
	const baseSamples = useMemo(
		() =>
			comparison
				? selectedCorner
					? filterSamplesByDistance(comparison.samples, selectedCorner.startDistanceM, selectedCorner.endDistanceM)
					: filterSamplesBySector(comparison.samples, sector)
				: [],
		[comparison, sector, selectedCorner],
	);
	const samples = useMemo(() => filterSamplesByTelemetryWindow(baseSamples, telemetryWindow), [baseSamples, telemetryWindow]);

	useEffect(() => {
		if (!comparisonPlaying) return;
		if (samples.length < 2) {
			setComparisonPlaying(false);
			return;
		}
		const timeline = telemetryPlaybackTimeline(samples);
		let index = Math.min(playbackStartIndex.current, samples.length - 1);
		let playbackTime = timeline[index] ?? 0;
		let previous = performance.now();
		const tick = (now: number) => {
			playbackTime += ((now - previous) / 1_000) * playbackSpeedRef.current;
			previous = now;
			while (index < samples.length - 1 && timeline[index + 1] <= playbackTime) index += 1;
			setCursorIndex(index);
			if (index >= samples.length - 1) {
				setComparisonPlaying(false);
				return;
			}
			playbackAnimationFrame.current = requestAnimationFrame(tick);
		};
		playbackAnimationFrame.current = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(playbackAnimationFrame.current);
	}, [comparisonPlaying, samples]);

	const toggleComparisonPlayback = () => {
		if (comparisonPlaying) {
			setComparisonPlaying(false);
			return;
		}
		const current = Math.min(Math.max(cursorIndex ?? 0, 0), Math.max(samples.length - 1, 0));
		const start = current >= samples.length - 1 ? 0 : current;
		playbackStartIndex.current = start;
		if (start !== current || cursorIndex == null) setCursorIndex(start);
		setComparisonPlaying(samples.length > 1);
	};
	const changePlaybackSpeed = (speed: number) => {
		playbackSpeedRef.current = speed;
		setPlaybackSpeed(speed);
	};

	useEffect(() => {
		if (state !== "ready" || samples.length < 2) return;
		const handlePlaybackKey = (event: KeyboardEvent) => {
			const target = event.target;
			if (target instanceof HTMLElement && (target.isContentEditable || target.matches("input, select, textarea, button, [role='button']"))) return;

			if (event.code === "Space") {
				if (event.repeat) return;
				event.preventDefault();
				toggleComparisonPlayback();
				return;
			}

			if (event.code === "ArrowLeft" || event.code === "ArrowRight" || event.code === "Home" || event.code === "End") {
				event.preventDefault();
				setComparisonPlaying(false);
				if (event.code === "Home" || event.code === "End") {
					setCursorIndex(event.code === "Home" ? 0 : samples.length - 1);
					return;
				}
				const timeline = telemetryPlaybackTimeline(samples);
				const current = Math.min(Math.max(cursorIndex ?? 0, 0), samples.length - 1);
				const direction = event.code === "ArrowLeft" ? -1 : 1;
				const seconds = event.shiftKey ? 5 : 1;
				setCursorIndex(nearestTelemetryPlaybackIndex(timeline, timeline[current] + direction * seconds));
				return;
			}

			if (event.code === "BracketLeft" || event.code === "BracketRight") {
				event.preventDefault();
				const current = telemetryPlaybackSpeeds.indexOf(playbackSpeed as (typeof telemetryPlaybackSpeeds)[number]);
				const direction = event.code === "BracketLeft" ? -1 : 1;
				const next = Math.min(telemetryPlaybackSpeeds.length - 1, Math.max(0, (current < 0 ? 3 : current) + direction));
				changePlaybackSpeed(telemetryPlaybackSpeeds[next]);
			}
		};
		window.addEventListener("keydown", handlePlaybackKey);
		return () => window.removeEventListener("keydown", handlePlaybackKey);
	}, [comparisonPlaying, cursorIndex, playbackSpeed, samples, state]);
	const zoomTelemetry = (anchorM: number, direction: "in" | "out") => {
		setComparisonPlaying(false);
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
						drivingObservations={drivingObservations}
						selectedCornerIndex={cornerIndex}
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
										<div className="grid grid-cols-[minmax(0,3fr)_minmax(380px,1fr)] gap-3">
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
											<div ref={mapPip.anchor} className="min-w-0">
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
												label="STEERING ANGLE"
												unit="°"
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												fixedRange={steeringAngleRange(samples)}
												zeroLine
												series={comparisonSeries(
													"referenceSteeringDegrees",
													"comparisonSteeringDegrees",
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
												label="GEAR"
												unit=""
												samples={samples}
												cursorIndex={cursorIndex}
												onCursor={setCursorIndex}
												onZoom={zoomTelemetry}
												fixedRange={[-1, 8]}
												formatValue={(value) => formatGear(value)}
												series={comparisonSeries("referenceGear", "comparisonGear", channelColours.gear, comparisonIsFaster)}
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
							setComparisonPlaying(false);
							setSector(value);
							setTelemetryWindow(null);
							setCornerIndex(null);
							setCursorIndex(null);
						}}
						cursorIndex={cursorIndex}
						onSeek={(index) => {
							setComparisonPlaying(false);
							setCursorIndex(index);
						}}
						playing={comparisonPlaying}
						onTogglePlayback={toggleComparisonPlayback}
						playbackSpeed={playbackSpeed}
						onPlaybackSpeed={changePlaybackSpeed}
					/>
				</>
			)}
		</>
	);
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
				className="fixed bottom-[251px] right-6 z-[31] w-[620px] max-w-[calc(100vw-var(--trace-sidebar)-72px)] border border-trace-divider bg-trace-black/95 shadow-[0_-12px_35px_rgba(0,0,0,.42)] backdrop-blur"
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
	drivingObservations,
	selectedCornerIndex,
	collapsed,
	onCollapsed,
	onSelect,
}: {
	corners: CornerAnalysis[];
	drivingObservations: DrivingObservation[];
	selectedCornerIndex: number | null;
	collapsed: boolean;
	onCollapsed: (collapsed: boolean) => void;
	onSelect: (index: number) => void;
}) {
	const [analysisTab, setAnalysisTab] = useState<"insights" | "corners">("insights");
	const opportunities = corners
		.filter((corner) => corner.totalLossSeconds != null && corner.totalLossSeconds > 0.005)
		.slice()
		.sort((left, right) => (right.totalLossSeconds ?? 0) - (left.totalLossSeconds ?? 0))
		.slice(0, 4);
	const highConfidenceObservations = drivingObservations.filter((observation) => observation.tier === "high");
	const lowConfidenceObservations = drivingObservations.filter((observation) => observation.tier === "low");
	return (
		<section
			className={`fixed bottom-[252px] left-[calc(var(--trace-sidebar)+28px)] top-[76px] z-50 overflow-y-auto border border-trace-divider bg-trace-surface shadow-[0_18px_55px_rgba(0,0,0,.45)] transition-[width] ${collapsed ? "w-11" : "w-72"}`}
			aria-label="Rule-based lap analysis"
		>
			<div
				className={`flex border-b border-trace-divider ${collapsed ? "h-11 items-center justify-center" : "min-h-16 items-center justify-between gap-3 p-3"}`}
			>
				{!collapsed && (
					<div className="min-w-0">
						<strong className="block font-mono text-[11px] tracking-[.1em] text-trace-soft">ANALYSIS</strong>
						<span className="mt-1 block text-[11px] leading-4 text-trace-dim">Telemetry rules · Not AI-generated</span>
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
						Estimates may be wrong. Verify them against the graphs and track map.
					</p>
					<div
						className="sticky top-0 z-10 grid grid-cols-2 border-b border-trace-divider bg-trace-surface"
						role="tablist"
						aria-label="Analysis view"
					>
						{(
							[
								{ id: "insights", label: "INSIGHTS" },
								{ id: "corners", label: "BREAKDOWN" },
							] as const
						).map((tab) => (
							<button
								type="button"
								role="tab"
								aria-selected={analysisTab === tab.id}
								onClick={() => {
									setAnalysisTab(tab.id);
									if (tab.id === "insights" && selectedCornerIndex != null) onSelect(selectedCornerIndex);
								}}
								className={`h-9 border-b-2 font-mono text-[10px] font-bold tracking-[.08em] transition-colors ${analysisTab === tab.id ? "border-trace-accent bg-trace-deep text-trace-text" : "border-transparent text-trace-dim hover:bg-trace-deep hover:text-trace-text"}`}
								key={tab.id}
							>
								{tab.label}
								{(tab.id === "insights" ? drivingObservations.length : opportunities.length) > 0
									? ` · ${tab.id === "insights" ? drivingObservations.length : opportunities.length}`
									: ""}
							</button>
						))}
					</div>
					{analysisTab === "insights" ? (
						<div role="tabpanel">
							{highConfidenceObservations.length > 0 && (
								<div className="border-b border-trace-divider px-3 py-3">
									<div className="divide-y divide-trace-divider">
										{highConfidenceObservations.map((observation) => (
											<DrivingObservationRow observation={observation} key={observation.kind} />
										))}
									</div>
								</div>
							)}
							{lowConfidenceObservations.length > 0 && (
								<details className="border-b border-trace-divider px-3 py-2">
									<summary className="cursor-pointer select-none font-mono text-[10px] font-bold tracking-[.06em] text-trace-dim hover:text-trace-text">
										LOW CONFIDENCE · {lowConfidenceObservations.length}
									</summary>
									<div className="mt-2 divide-y divide-trace-divider">
										{lowConfidenceObservations.map((observation) => (
											<DrivingObservationRow observation={observation} key={observation.kind} />
										))}
									</div>
								</details>
							)}
							{drivingObservations.length === 0 && (
								<p className="px-4 py-4 text-[12px] leading-5 text-trace-dim">No repeated driving insights were detected for this lap pair.</p>
							)}
						</div>
					) : opportunities.length === 0 ? (
						<p className="px-4 py-4 text-[12px] leading-5 text-trace-dim" role="tabpanel">
							The rule-based comparison did not detect any meaningful corner losses.
						</p>
					) : (
						<div>
							{selectedCornerIndex != null && (
								<button
									type="button"
									onClick={() => onSelect(selectedCornerIndex)}
									className="w-full border-b border-trace-divider px-4 py-3 text-left font-mono text-[10px] font-bold tracking-[.08em] text-trace-accent hover:bg-trace-deep hover:text-trace-text"
								>
									SHOW FULL LAP
								</button>
							)}
							{opportunities.map((corner) => (
								<CornerBreakdownRow
									corner={corner}
									selected={selectedCornerIndex === corner.index}
									onSelect={() => onSelect(corner.index)}
									key={corner.index}
								/>
							))}
						</div>
					)}
				</div>
			)}
		</section>
	);
}

function CornerBreakdownRow({ corner, selected, onSelect }: { corner: CornerAnalysis; selected: boolean; onSelect: () => void }) {
	const [expanded, setExpanded] = useState(false);
	const dominant = dominantCornerPhase(corner);
	return (
		<article className={`border-b border-trace-divider ${selected ? "bg-trace-accent-wash outline outline-1 -outline-offset-1 outline-trace-accent" : ""}`}>
			<div className="flex min-w-0 items-stretch">
				<button
					type="button"
					onClick={onSelect}
					className="grid min-w-0 flex-1 grid-cols-[40px_minmax(0,1fr)] items-start gap-2.5 px-3 py-3 text-left hover:bg-trace-deep"
					aria-pressed={selected}
					aria-label={`${selected ? "Clear" : "View"} ${corner.label} telemetry`}
				>
					<span className="font-mono text-[18px] font-black leading-none text-trace-text">{corner.label}</span>
					<span className="min-w-0">
						<span className="flex items-start justify-between gap-2">
							<strong className="min-w-0 text-[11px] font-bold leading-4 text-trace-soft">{cornerSummary(corner, dominant)}</strong>
							<strong className="shrink-0 font-mono text-[13px] leading-4 tabular-nums text-[#ff5263]">
								+{corner.totalLossSeconds?.toFixed(3)}s
							</strong>
						</span>
						<span className="mt-1 block font-mono text-[9px] font-bold leading-3 tracking-[.06em] text-trace-dim">
							{dominant} LOSS · {Math.round(corner.startDistanceM)}–{Math.round(corner.endDistanceM)} M
						</span>
					</span>
				</button>
				<button
					type="button"
					onClick={() => setExpanded((value) => !value)}
					className="grid w-9 shrink-0 place-items-center border-l border-trace-divider text-trace-dim hover:bg-trace-deep hover:text-trace-text"
					aria-label={`${expanded ? "Hide" : "Show"} ${corner.label} details`}
					aria-expanded={expanded}
				>
					<svg
						className={`size-3.5 fill-none stroke-current transition-transform ${expanded ? "rotate-180" : ""}`}
						viewBox="0 0 16 16"
						aria-hidden="true"
					>
						<path d="m3 6 5 5 5-5" />
					</svg>
				</button>
			</div>
			{expanded && (
				<div className="border-t border-trace-divider px-4 pb-3">
					<CornerBrakingMetrics corner={corner} />
					<div className="mt-3 grid grid-cols-3 gap-3 border-t border-trace-divider pt-2" aria-label={`${corner.label} time difference by phase`}>
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
					</div>
				</div>
			)}
		</article>
	);
}

function DrivingObservationRow({ observation }: { observation: DrivingObservation }) {
	const [title, value] = drivingObservationCopy(observation);
	return (
		<div className="py-2.5">
			<span className="block text-[12px] font-semibold leading-4 text-trace-text">{title}</span>
			<span className="mt-0.5 block text-[10px] leading-4 text-trace-muted">
				{value} · T{observation.cornerIndices.join(", T")}
			</span>
		</div>
	);
}

function drivingObservationCopy(observation: DrivingObservation): [string, string] {
	const corners = `${observation.cornerIndices.length} of ${observation.eligibleCornerCount} measured corners`;
	const distance = `${Math.abs(Math.round(observation.meanDifference))} m`;
	const speed = `${Math.abs(Math.round(observation.meanDifference))} km/h`;
	const seconds = `${Math.abs(observation.meanDifference).toFixed(3)} s total`;
	const count = `${Math.abs(observation.meanDifference).toFixed(1)} extra reversals on average`;
	switch (observation.kind) {
		case "braking_earlier":
			return ["Braking starts earlier", `${distance} earlier across ${corners}`];
		case "braking_later":
			return ["Braking starts later", `${distance} later across ${corners}`];
		case "brake_release_earlier":
			return ["Brake release is earlier", `${distance} earlier across ${corners}`];
		case "brake_release_later":
			return ["Brake release is later", `${distance} later across ${corners}`];
		case "lower_minimum_speed":
			return ["Lower minimum corner speed", `${speed} lower across ${corners}`];
		case "later_throttle":
			return ["Throttle application is later", `${distance} later across ${corners}`];
		case "entry_loss":
			return ["Most repeated loss is on entry", `${seconds} across ${corners}`];
		case "mid_corner_loss":
			return ["Most repeated loss is mid-corner", `${seconds} across ${corners}`];
		case "exit_loss":
			return ["Most repeated loss is on exit", `${seconds} across ${corners}`];
		case "more_steering_corrections":
			return ["More steering corrections detected", `${count} across ${corners}`];
	}
}

function CornerBrakingMetrics({ corner }: { corner: CornerAnalysis }) {
	const reference = brakingFacts(corner, "reference");
	const analysed = brakingFacts(corner, "comparison");
	if (!reference && !analysed) return null;
	return (
		<span className="grid grid-cols-2 gap-2 pt-3 font-mono">
			{[
				{ label: "REFERENCE", facts: reference, colour: "text-trace-purple" },
				{ label: "ANALYSED", facts: analysed, colour: "text-trace-accent" },
			].map(({ label, facts, colour }) => (
				<span className="min-w-0" key={label}>
					<span className={`block text-[9px] font-bold tracking-[.06em] ${colour}`}>{label}</span>
					<span className="mt-1 block text-[11px] font-bold tabular-nums text-trace-text">{facts?.beforeApex ?? "—"}</span>
					<span className="mt-0.5 block text-[9px] text-trace-dim">BEFORE APEX</span>
					{facts && (
						<span className="mt-1 block text-[9px] tabular-nums text-trace-muted">
							{facts.brakeSpan === 0 ? "BRAKE TAP" : `${facts.brakeSpan} M ZONE`} · {facts.peak} PEAK
						</span>
					)}
				</span>
			))}
		</span>
	);
}

function brakingFacts(corner: CornerAnalysis, driver: "reference" | "comparison") {
	const start = driver === "reference" ? corner.metrics.referenceBrakingPointM : corner.metrics.comparisonBrakingPointM;
	if (start == null) return null;
	const release = driver === "reference" ? corner.metrics.referenceBrakeReleasePointM : corner.metrics.comparisonBrakeReleasePointM;
	const peak = driver === "reference" ? corner.metrics.referencePeakBrakePercent : corner.metrics.comparisonPeakBrakePercent;
	return {
		beforeApex: `${Math.max(0, Math.round(corner.apexDistanceM - start))} M`,
		brakeSpan: Math.max(0, Math.round((release ?? start) - start)),
		peak: peak == null ? "—" : `${Math.round(peak)}%`,
	};
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
	const brakingDifference =
		corner.metrics.comparisonBrakingPointM != null && corner.metrics.referenceBrakingPointM != null
			? corner.metrics.comparisonBrakingPointM - corner.metrics.referenceBrakingPointM
			: null;
	if (dominantPhase === "ENTRY" && brakingDifference != null && Math.abs(brakingDifference) >= 3)
		return `Brakes ${Math.round(Math.abs(brakingDifference))} m ${brakingDifference > 0 ? "later" : "earlier"} than Reference`;
	const minimumSpeedDifference =
		corner.metrics.comparisonMinimumSpeedKmh != null && corner.metrics.referenceMinimumSpeedKmh != null
			? corner.metrics.comparisonMinimumSpeedKmh - corner.metrics.referenceMinimumSpeedKmh
			: null;
	if (dominantPhase === "MIDDLE" && minimumSpeedDifference != null && minimumSpeedDifference < -1)
		return `${Math.round(Math.abs(minimumSpeedDifference))} km/h lower minimum speed`;
	const throttleDifference =
		corner.metrics.comparisonThrottlePointM != null && corner.metrics.referenceThrottlePointM != null
			? corner.metrics.comparisonThrottlePointM - corner.metrics.referenceThrottlePointM
			: null;
	if (dominantPhase === "EXIT" && throttleDifference != null && throttleDifference > 3) return `Throttle applied ${Math.round(throttleDifference)} m later`;
	return `Loss develops through ${dominantPhase.toLowerCase()}`;
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
	playing: boolean;
	onTogglePlayback: () => void;
	playbackSpeed: number;
	onPlaybackSpeed: (speed: number) => void;
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
	playing,
	onTogglePlayback,
	playbackSpeed,
	onPlaybackSpeed,
}: ComparisonHudProps) {
	const [richSessionPicker, setRichSessionPicker] = useState(richComparisonPickerEnabled);
	useEffect(() => {
		const update = () => setRichSessionPicker(richComparisonPickerEnabled());
		window.addEventListener(FEATURE_FLAGS_CHANGED_EVENT, update);
		return () => window.removeEventListener(FEATURE_FLAGS_CHANGED_EVENT, update);
	}, []);
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
	const sectorDeltas = sharedComparisonSectorDeltas(
		referenceLaps.find((lap) => lap.index === referenceLap)?.sectors.map((item) => ({ index: item.index, durationNs: item.durationNs })) ?? [],
		comparisonLaps.find((lap) => lap.index === comparisonLap)?.sectors.map((item) => ({ index: item.index, durationNs: item.durationNs })) ?? [],
		comparison?.samples ?? [],
	);
	const referenceLabel = referenceSession?.driver?.trim() || "REFERENCE";
	const comparisonLabel = comparisonSession?.driver?.trim() || "ANALYSED LAP";
	const trackCarLabel = referenceSession ? `${referenceSession.track} · ${referenceSession.car}` : "TRACK · CAR";
	return (
		<ComparisonTelemetryHud
			samples={samples}
			cursorIndex={cursorIndex}
			onSeek={onSeek}
			referenceLabel={referenceLabel}
			comparisonLabel={comparisonLabel}
			trackCarLabel={trackCarLabel}
			sectorDeltas={sectorDeltas}
			sector={sector}
			onSector={onSector}
			comparisonIsFaster={comparisonIsFaster}
			onSwap={referenceLap == null || comparisonLap == null ? undefined : onSwap}
			playing={playing}
			onTogglePlayback={onTogglePlayback}
			playbackSpeed={playbackSpeed}
			onPlaybackSpeed={onPlaybackSpeed}
			referenceAirTemperatureC={referenceAirTemperature}
			referenceTrackTemperatureC={referenceTrackTemperature}
			comparisonAirTemperatureC={comparisonAirTemperature}
			comparisonTrackTemperatureC={comparisonTrackTemperature}
			referenceChoice={
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
					richSessionPicker={richSessionPicker}
				/>
			}
			comparisonChoice={
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
					richSessionPicker={richSessionPicker}
				/>
			}
		/>
	);
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
									<span>{formatSessionType(suggestion.session.sessionType)}</span>
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
	richSessionPicker,
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
	richSessionPicker: boolean;
}) {
	return (
		<div className="grid min-w-0 grid-cols-[112px_minmax(190px,1fr)_140px] items-center gap-2">
			<span className="min-w-0 font-mono">
				<strong className={`block truncate text-[10px] font-black leading-3 tracking-[.1em] ${colour}`}>{label}</strong>
				<span className="mt-0.5 block truncate text-[8px] font-bold leading-3 tracking-[.07em] text-trace-dim">{role}</span>
			</span>
			<ComparisonSessionPicker sessions={sessions} value={sessionId} onChange={onSession} label={role} rich={richSessionPicker} />
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

function numericCondition(value?: string | null) {
	if (!value) return null;
	const parsed = Number(value);
	return Number.isFinite(parsed) ? parsed : null;
}
