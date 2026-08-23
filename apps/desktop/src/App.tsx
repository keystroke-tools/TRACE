import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { open } from "@tauri-apps/plugin-dialog";
import desktopPackage from "../package.json";
import {
  telemetryDataSource,
  type CornerAnalysis,
  type GameInstallDirectory,
  type LapComparison,
  type LapComparisonSample,
  type LapTrace,
  type TrackMapAsset,
  type RecordedLapMetrics,
  type RecordedSessionSummary,
  type SavedComparison,
  type SessionExportFormat,
  type TelemetryStatus,
} from "./data-source";
import { TitleBar } from "./TitleBar";
import { Tooltip } from "./Tooltip";
import { useToast } from "./Toast";

const navigation = ["LIVE", "SESSIONS", "COMPARE", "SETUPS", "SETTINGS"] as const;
type Section = (typeof navigation)[number];
const DEFAULT_API_SERVICE_ENDPOINT = "https://api.simtrace.run";
const DEFAULT_LIVE_SERVICE_ENDPOINT = "https://live.simtrace.run";

export function App() {
  const [status, setStatus] = useState<TelemetryStatus | null>(null);
  const [sessions, setSessions] = useState<RecordedSessionSummary[]>([]);
  const [section, setSection] = useState<Section>("LIVE");
  const [openSessionId, setOpenSessionId] = useState<string | null>(null);
  const [openLapIndex, setOpenLapIndex] = useState<number | null>(null);
  const openSession = sessions.find((session) => session.id === openSessionId) ?? null;

  async function selectSimulator(simulatorId: string) {
    await telemetryDataSource.selectSimulator(simulatorId);
    setStatus(await telemetryDataSource.getStatus());
  }

  useEffect(() => {
    void Promise.all([
      telemetryDataSource.getStatus(),
      telemetryDataSource.getSessions(),
    ]).then(([nextStatus, nextSessions]) => {
      setStatus(nextStatus);
      setSessions(nextSessions);
    });
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      void telemetryDataSource.getStatus().then(setStatus);
      if (section === "SESSIONS") {
        void telemetryDataSource.getSessions().then(setSessions);
      }
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [section]);

  return (
    <main className="grid h-screen grid-cols-[176px_1fr] grid-rows-[48px_minmax(0,1fr)_38px] bg-trace-base text-trace-text max-[900px]:grid-cols-[140px_1fr]">
      <TitleBar status={status} backLabel={openLapIndex == null ? "SESSIONS" : "SESSION"} onBack={openSession ? () => { if (openLapIndex != null) setOpenLapIndex(null); else setOpenSessionId(null); } : undefined} />
      <Navigation active={section} onChange={(next) => { setSection(next); if (next !== "SESSIONS") { setOpenSessionId(null); setOpenLapIndex(null); } }} />
      <section className="trace-grid overflow-auto p-7">
        {section === "LIVE" && <Live status={status} onOpenSessions={() => setSection("SESSIONS")} onSelectSimulator={selectSimulator} />}
        {section === "SESSIONS" && (
          openSession ? (
            openLapIndex == null
              ? <SessionDetail session={openSession} onOpenLap={setOpenLapIndex} />
              : <LapVisualizer session={openSession} lapIndex={openLapIndex} />
          ) : (
            <Sessions
              sessions={sessions}
              onOpen={(sessionId) => { setOpenSessionId(sessionId); setOpenLapIndex(null); }}
              onDeleted={(sessionId) => setSessions((current) => current.filter((session) => session.id !== sessionId))}
              onUpdated={(updated) => setSessions((current) => current.map((session) => session.id === updated.id ? updated : session))}
              onImported={async () => setSessions(await telemetryDataSource.getSessions())}
            />
          )
        )}
        {section === "COMPARE" && <Compare sessions={sessions} />}
        {section === "SETUPS" && <Setups />}
        {section === "SETTINGS" && <Settings />}
      </section>
      <Footer status={status} />
    </main>
  );
}

function Navigation({ active, onChange }: { active: Section; onChange: (section: Section) => void }) {
  return (
    <aside className="border-r border-trace-divider bg-trace-surface pt-4" aria-label="Primary navigation">
      {navigation.map((item, index) => (
        <button
          key={item}
          type="button"
          onClick={() => onChange(item)}
          className={`flex h-[52px] w-full items-center gap-3 border-0 border-l-[3px] px-4 text-left text-xs font-bold tracking-[.1em] transition-colors ${
            item === active
              ? "border-trace-accent bg-trace-accent-wash text-white"
              : "border-transparent bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-text"
          }`}
        >
          <span className="font-mono text-[12px] text-trace-dim">0{index + 1}</span>
          {item}
        </button>
      ))}
    </aside>
  );
}

function Live({ status, onOpenSessions, onSelectSimulator }: { status: TelemetryStatus | null; onOpenSessions: () => void; onSelectSimulator: (simulatorId: string) => Promise<void> }) {
  const recording = status?.connection === "recording" || status?.connection === "replay";
  const simulatorName = status?.simulatorName ?? "YOUR SIMULATOR";
  const simulatorShortName = status?.simulatorShortName ?? "SIM";
  const availableChannels = status?.channels.filter((channel) => channel.available) ?? [];
  const unavailableChannels = status?.channels.filter((channel) => !channel.available) ?? [];
  const categories = Array.from(new Set(availableChannels.map((channel) => channel.category)));

  return (
    <>
      <PageIntro
        index="01"
        eyebrow="LIVE CAPTURE"
        title={recording ? `RECORDING ${simulatorName.toUpperCase()}` : `READY WHEN ${simulatorName.toUpperCase()} IS`}
        description={recording
          ? "TRACE is recording the current drive or replay automatically. Keep it running until the session ends."
          : `Start a drive or play a replay in ${simulatorName}. TRACE detects it and records locally—there is no record button to press.`}
      />
      <SimulatorPicker status={status} onSelect={onSelectSimulator} />
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
            TRACE saves portable analysis-ready channels plus the selected adapter's complete documented native data. {simulatorShortName}-native values remain in source units for future analysis; this is recording coverage, not a live sensor test.
          </p>
          <div className="grid grid-cols-2 gap-px p-4 max-[900px]:grid-cols-1">
            {categories.map((category) => (
              <div className="border border-trace-divider bg-trace-deep p-4" key={category}>
                <strong className={`font-mono text-[12px] tracking-[.1em] ${category.includes("NATIVE") ? "text-trace-purple" : "text-trace-accent"}`}>{category}</strong>
                <div className="mt-3 flex flex-wrap gap-2">
                  {availableChannels.filter((channel) => channel.category === category).map((channel) => (
                    <Tooltip className="border border-trace-divider bg-trace-surface px-2.5 py-1.5 text-[12px] text-trace-soft" content={channel.detail} key={channel.id}>
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
            <WorkflowStep number="1" title={`Open ${simulatorName}`}>Start driving or play a replay at normal speed.</WorkflowStep>
            <WorkflowStep number="2" title="TRACE records automatically">The status light pulses while samples are being saved.</WorkflowStep>
            <WorkflowStep number="3" title="Review the session">End normally, then open Sessions to inspect laps or export data.</WorkflowStep>
          </ol>
          <button type="button" onClick={onOpenSessions} className="mx-5 mb-5 border border-trace-accent-muted bg-trace-accent-wash px-4 py-3 text-[12px] font-black tracking-[.1em] text-trace-accent hover:border-trace-accent">
            OPEN SESSIONS
          </button>
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
                <p className="mt-1 leading-5 text-trace-dim">{channel.detail}. TRACE leaves uncertain data out instead of assigning it a misleading meaning.</p>
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
          {simulators.map((simulator) => <option value={simulator.id} disabled={!simulator.available} key={simulator.id}>{simulator.name}{simulator.available ? "" : " · unavailable"}</option>)}
        </select>
      </label>
      <span className="px-4 text-[12px] text-trace-dim">
        {selectable.length} capture adapter{selectable.length === 1 ? "" : "s"} installed
      </span>
    </div>
  );
}

function LapVisualizer({ session, lapIndex }: { session: RecordedSessionSummary; lapIndex: number }) {
  const [trace, setTrace] = useState<LapTrace | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cursorIndex, setCursorIndex] = useState<number | null>(null);
  const [sector, setSector] = useState<number | null>(null);
  const [telemetryWindow, setTelemetryWindow] = useState<TelemetryWindow | null>(null);
  const [mapZoomLinked, setMapZoomLinked] = useState(false);
  const mapPip = useTrackMapPip(trace != null);

  useEffect(() => {
    let active = true;
    setTrace(null);
    setError(null);
    setTelemetryWindow(null);
    void telemetryDataSource.visualizeSessionLap(session.id, lapIndex).then((value) => {
      if (active) setTrace(value);
    }).catch((reason) => {
      if (active) setError(reason instanceof Error ? reason.message : String(reason));
    });
    return () => { active = false; };
  }, [lapIndex, session.id]);

  const chartSamples = useMemo<LapComparisonSample[]>(() => trace?.samples.map((sample) => ({
    distanceM: sample.distanceM,
    sectorIndex: sample.sectorIndex,
    referenceSpeedKmh: sample.speedKmh,
    referenceThrottlePercent: sample.throttlePercent,
    referenceBrakePercent: sample.brakePercent,
    referenceSteeringPercent: sample.steeringPercent,
    referenceRpm: sample.rpm,
    referenceGear: sample.gear,
    referencePositionXM: sample.positionXM,
    referencePositionZM: sample.positionZM,
    referenceAirTemperatureC: sample.airTemperatureC,
    referenceTrackTemperatureC: sample.trackTemperatureC,
  })) ?? [], [trace]);
  const baseSamples = filterSamplesBySector(chartSamples, sector);
  const samples = filterSamplesByTelemetryWindow(baseSamples, telemetryWindow);
  const cursor = cursorIndex == null ? null : samples[cursorIndex] ?? null;
  const zoomTelemetry = (anchorM: number, direction: "in" | "out") => {
    setTelemetryWindow((current) => nextTelemetryWindow(baseSamples, current, anchorM, direction));
    setCursorIndex(null);
  };

  return (
    <>
      <PageIntro index="02" eyebrow="LAP VISUALIZER" title={`LAP ${lapIndex} · ${trace?.lapTime ?? session.laps.find((lap) => lap.index === lapIndex)?.time ?? "—"}`} description={`${session.track} · ${session.car}. Inspect the recorded driving inputs and line on one synchronized distance axis.`} />
      {!trace && !error && <div className="mt-7 border border-trace-divider bg-trace-surface p-8 font-mono text-[12px] text-trace-dim">PREPARING LAP TELEMETRY…</div>}
      {error && <div className="mt-7 border border-trace-warning/50 bg-trace-warning/10 p-5 text-[13px] text-trace-warning"><strong>Lap visualization unavailable.</strong> {error}</div>}
      {trace && (
        <div className="mt-7">
          <div className="flex items-center justify-between border border-trace-divider bg-trace-surface px-5 py-3">
            <SectorPicker samples={chartSamples} value={sector} onChange={(value) => { setSector(value); setTelemetryWindow(null); setCursorIndex(null); }} />
            <div className="flex gap-6 font-mono text-[12px] text-trace-muted">
              {telemetryWindow && <button type="button" onClick={() => { setTelemetryWindow(null); setCursorIndex(null); }} className="font-bold text-trace-accent hover:text-trace-text">{Math.round(telemetryWindow.startM)}–{Math.round(telemetryWindow.endM)} M · RESET ZOOM</button>}
              <span>{Math.round(cursor?.distanceM ?? trace.lapLengthM).toLocaleString()} M</span>
              <span>GEAR <strong className="text-trace-text">{formatGear(cursor?.referenceGear)}</strong></span>
            </div>
          </div>
          <div className="mt-3 pb-32">
            <div className="grid grid-cols-[minmax(560px,604px)_minmax(460px,1fr)] gap-3">
              <div ref={mapPip.anchor}><TrackMap samples={samples} cursorIndex={cursorIndex} trackMap={trace.trackMap} height={556} focusSelection={sector != null || telemetryWindow != null} rangeLabel={sector != null ? `SECTOR ${sector}` : telemetryWindow ? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M` : undefined} onRangeZoom={zoomTelemetry} rangeZoomLinked={mapZoomLinked} onRangeZoomLinked={setMapZoomLinked} /></div>
              <div className="grid gap-3">
                <ComparisonChart label="SPEED" unit="km/h" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} series={singleSeries("referenceSpeedKmh", channelColours.speed)} />
                <ComparisonChart label="GEAR" unit="" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={[-1, 8]} series={singleSeries("referenceGear", channelColours.gear)} />
              </div>
            </div>
            <div className="mt-3 grid gap-3">
              <ComparisonChart label="THROTTLE" unit="%" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={[0, 100]} series={singleSeries("referenceThrottlePercent", channelColours.throttle)} />
              <ComparisonChart label="BRAKE" unit="%" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={[0, 100]} series={singleSeries("referenceBrakePercent", channelColours.brake)} />
              <ComparisonChart label="STEERING INPUT" unit="%" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={steeringInputRange(samples)} zeroLine series={singleSeries("referenceSteeringPercent", channelColours.steering)} />
            </div>
          </div>
          {mapPip.visible && <FloatingTrackMap samples={samples} cursorIndex={cursorIndex} trackMap={trace.trackMap} focusSelection={sector != null || telemetryWindow != null} rangeLabel={sector != null ? `SECTOR ${sector}` : telemetryWindow ? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M` : undefined} onRangeZoom={zoomTelemetry} rangeZoomLinked={mapZoomLinked} onRangeZoomLinked={setMapZoomLinked} onDismiss={mapPip.dismiss} />}
          <TelemetryHud session={session} lapIndex={lapIndex} samples={samples} cursorIndex={cursorIndex} onSeek={setCursorIndex} />
        </div>
      )}
    </>
  );
}

function Compare({ sessions }: { sessions: RecordedSessionSummary[] }) {
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
  const compatibleSessions = useMemo(() => referenceSession == null ? eligibleSessions : eligibleSessions.filter((candidate) => candidate.simulatorId === referenceSession.simulatorId && candidate.track === referenceSession.track && candidate.car === referenceSession.car), [eligibleSessions, referenceSession]);
  const comparisonSession = compatibleSessions.find((candidate) => candidate.id === comparisonSessionId) ?? null;
  const referenceLaps = useMemo(() => referenceSession ? validComparisonLaps(referenceSession) : [], [referenceSession]);
  const comparisonLaps = useMemo(() => comparisonSession ? validComparisonLaps(comparisonSession) : [], [comparisonSession]);
  const referenceSuggestions = fasterReferenceSuggestions(eligibleSessions, comparisonSession ?? undefined, comparisonLaps.find((lap) => lap.index === comparisonLap) ?? null, comparisonSessionId, comparisonLap);

  useEffect(() => {
    void telemetryDataSource.getSavedComparisons().then(setSavedComparisons).catch((reason) => {
      showToast({ kind: "error", title: "Saved comparisons unavailable", message: reason instanceof Error ? reason.message : String(reason), timeoutMs: 8_000 });
    });
  }, [showToast]);

  useEffect(() => {
    if (!referenceSessionId && eligibleSessions[0]) {
      const analysedSession = eligibleSessions.find((session) => sessionSourceGroup(session) !== "imported") ?? eligibleSessions[0];
      const analysedLap = validComparisonLaps(analysedSession)[0];
      if (!analysedLap) return;
      const compatible = eligibleSessions.filter((candidate) => candidate.simulatorId === analysedSession.simulatorId && candidate.track === analysedSession.track && candidate.car === analysedSession.car);
      const suggested = fasterReferenceSuggestions(eligibleSessions, analysedSession, analysedLap, analysedSession.id, analysedLap.index)[0];
      const fallback = compatible
        .flatMap((session) => validComparisonLaps(session).filter((lap) => session.id !== analysedSession.id || lap.index !== analysedLap.index).map((lap) => ({ session, lap })))
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
    const nextComparisonSession = compatibleSessions.find((candidate) => candidate.id === referenceSession.id && validComparisonLaps(candidate).length >= 2)
      ?? compatibleSessions.find((candidate) => candidate.id !== referenceSession.id)
      ?? compatibleSessions[0];
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
    if (!referenceSessionId || !comparisonSessionId || referenceLap == null || comparisonLap == null || (referenceSessionId === comparisonSessionId && referenceLap === comparisonLap)) {
      setState("idle");
      return;
    }
    let active = true;
    setState("loading");
    setError(null);
    void telemetryDataSource.compareSessionLaps(referenceSessionId, referenceLap, comparisonSessionId, comparisonLap).then((value) => {
      if (!active) return;
      setComparison(value);
      setSector(null);
      setTelemetryWindow(null);
      setCornerIndex(null);
      setCursorIndex(null);
      setState("ready");
    }).catch((reason) => {
      if (!active) return;
      setComparison(null);
      setError(reason instanceof Error ? reason.message : String(reason));
      setState("error");
    });
    return () => { active = false; };
  }, [comparisonLap, comparisonRequestVersion, comparisonSessionId, referenceLap, referenceSessionId]);

  const corners = comparison?.cornerAnalysis.value?.corners ?? [];
  const finalDelta = comparison?.samples.slice().reverse().find((sample) => sample.deltaSeconds != null)?.deltaSeconds ?? null;
  const comparisonIsFaster = finalDelta != null && finalDelta < -0.0005;
  const defaultComparisonName = comparison
    ? `${referenceSession?.driver ?? "Reference"} ${comparison.referenceLapTime} vs ${comparisonSession?.driver ?? "Analysed"} ${comparison.comparisonLapTime}`.slice(0, 80)
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
      showToast({ kind: "error", title: "Could not save comparison", message: reason instanceof Error ? reason.message : String(reason), timeoutMs: 8_000 });
      return false;
    }
  }

  async function deleteSavedComparison(comparisonId: string) {
    try {
      setSavedComparisons(await telemetryDataSource.deleteSavedComparison(comparisonId));
      showToast({ kind: "success", title: "Saved comparison removed", message: "The shortcut was deleted; its sessions and telemetry were not changed.", timeoutMs: 3_500 });
    } catch (reason) {
      showToast({ kind: "error", title: "Could not remove comparison", message: reason instanceof Error ? reason.message : String(reason), timeoutMs: 8_000 });
    }
  }

  async function renameSavedComparison(comparisonId: string, name: string) {
    try {
      setSavedComparisons(await telemetryDataSource.renameSavedComparison(comparisonId, name));
      showToast({ kind: "success", title: "Favourite renamed", message: "The saved lap pair has a new name.", timeoutMs: 3_500 });
      return true;
    } catch (reason) {
      showToast({ kind: "error", title: "Could not rename favourite", message: reason instanceof Error ? reason.message : String(reason), timeoutMs: 8_000 });
      return false;
    }
  }

  function openSavedComparison(saved: SavedComparison) {
    const alreadySelected = saved.referenceSessionId === referenceSessionId
      && saved.referenceLapIndex === referenceLap
      && saved.analysedSessionId === comparisonSessionId
      && saved.analysedLapIndex === comparisonLap;
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
          <p className="mx-auto mt-2 max-w-lg text-[13px] leading-6 text-trace-muted">Record or import at least one complete valid lap. A second lap may come from the same session or another visit to the same track.</p>
        </div>
      ) : (
        <>
          <SavedComparisonsDock savedComparisons={savedComparisons} suggestions={referenceSuggestions} sessions={sessions} defaultName={defaultComparisonName} canSave={comparison != null && state === "ready"} currentReferenceSessionId={referenceSessionId} currentReferenceLap={referenceLap} currentAnalysedSessionId={comparisonSessionId} currentAnalysedLap={comparisonLap} onOpen={openSavedComparison} onSelectSuggestion={(suggestion) => selectSuggestedReference(suggestion.session.id, suggestion.lap.index)} onSave={saveCurrentComparison} onDelete={deleteSavedComparison} onRename={renameSavedComparison} />
          <CornerAnalysisPanel corners={corners} selectedCornerIndex={cornerIndex} comparisonIsFaster={comparisonIsFaster} collapsed={analysisCollapsed} onCollapsed={setAnalysisCollapsed} onSelect={(value) => { setCornerIndex(value === cornerIndex ? null : value); setSector(null); setTelemetryWindow(null); setCursorIndex(null); }} />
          <div className={analysisCollapsed ? "ml-14" : "ml-[300px]"}>
            {state === "loading" && <div className="border border-trace-divider bg-trace-surface p-8 font-mono text-[12px] text-trace-dim">ALIGNING RECORDED TELEMETRY…</div>}
            {state === "idle" && <div className="border border-trace-divider bg-trace-surface p-10 text-center"><strong className="text-base">Choose a Reference and an Analysed Lap below</strong><p className="mt-2 text-[13px] text-trace-muted">Use a faster clean lap as the Reference, then choose the compatible lap you want to improve—or reopen one from Saved Comparisons above the HUD.</p></div>}
            {state === "error" && <div className="border border-trace-warning/50 bg-trace-warning/10 p-5 text-[13px] text-trace-warning"><strong>Lap analysis unavailable.</strong> {error}</div>}
          </div>
          {comparison && state === "ready" && (
            <div>
              <div className="pb-56">
                <div className={`grid items-start gap-3 transition-[grid-template-columns] ${analysisCollapsed ? "grid-cols-[44px_minmax(0,1fr)]" : "grid-cols-[288px_minmax(0,1fr)]"}`}>
                  <div className="w-full">
                  </div>
                  <div className="min-w-0">
                    {(sector != null || selectedCorner != null || telemetryWindow != null) && <div className="mb-3 flex h-10 items-center justify-between border border-trace-accent/45 bg-trace-accent-wash px-4 font-mono">
                      <span className="text-[11px] font-black tracking-[.1em] text-trace-accent">VIEWING {selectedCorner ? selectedCorner.label : sector != null ? `SECTOR ${sector}` : "LAP RANGE"}{telemetryWindow ? ` · ${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M` : ""}</span>
                      <button type="button" onClick={() => { setSector(null); setTelemetryWindow(null); setCornerIndex(null); setCursorIndex(null); }} className="text-[10px] font-bold tracking-[.08em] text-trace-soft hover:text-trace-text">RETURN TO FULL LAP</button>
                    </div>}
                    <div className="grid grid-cols-[minmax(480px,560px)_minmax(360px,1fr)] gap-3">
                      <div ref={mapPip.anchor}><TrackMap samples={samples} cursorIndex={cursorIndex} comparison comparisonIsFaster={comparisonIsFaster} height={512} trackMap={comparison.trackMap} focusSelection={sector != null || selectedCorner != null || telemetryWindow != null} rangeLabel={selectedCorner?.label ?? (sector != null ? `SECTOR ${sector}` : telemetryWindow ? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M` : undefined)} corners={corners} selectedCornerIndex={cornerIndex} onRangeZoom={zoomTelemetry} rangeZoomLinked={mapZoomLinked} onRangeZoomLinked={setMapZoomLinked} /></div>
                      <div className="grid gap-3">
                        <ComparisonChart label="SPEED" unit="km/h" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} series={comparisonSeries("referenceSpeedKmh", "comparisonSpeedKmh", channelColours.speed, comparisonIsFaster)} />
                        <ComparisonChart label="GEAR" unit="" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={[-1, 8]} series={comparisonSeries("referenceGear", "comparisonGear", channelColours.gear, comparisonIsFaster)} />
                      </div>
                    </div>
                    <div className="mt-3 grid gap-3">
                      <ComparisonChart label="THROTTLE" unit="%" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={[0, 100]} series={comparisonSeries("referenceThrottlePercent", "comparisonThrottlePercent", channelColours.throttle, comparisonIsFaster)} />
                      <ComparisonChart label="BRAKE" unit="%" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={[0, 100]} series={comparisonSeries("referenceBrakePercent", "comparisonBrakePercent", channelColours.brake, comparisonIsFaster)} />
                      <ComparisonChart label="STEERING INPUT" unit="%" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={steeringInputRange(samples)} zeroLine series={comparisonSeries("referenceSteeringPercent", "comparisonSteeringPercent", channelColours.steering, comparisonIsFaster)} />
                    </div>
                    <div className="mt-3 grid grid-cols-2 gap-3">
                      <ComparisonChart label="ENGINE SPEED" unit="rpm" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} series={comparisonSeries("referenceRpm", "comparisonRpm", channelColours.rpm, comparisonIsFaster)} />
                      <ComparisonChart label="TIME DIFFERENCE" unit="s" samples={samples} cursorIndex={cursorIndex} onCursor={setCursorIndex} onZoom={zoomTelemetry} fixedRange={deltaRange(samples)} series={[{ label: "ANALYSED LAP VS REFERENCE", colour: channelColours.delta, value: (sample) => sample.deltaSeconds }]} zeroLine />
                    </div>
                  </div>
                </div>
              </div>
              {mapPip.visible && <FloatingTrackMap samples={samples} cursorIndex={cursorIndex} comparison comparisonIsFaster={comparisonIsFaster} trackMap={comparison.trackMap} focusSelection={sector != null || selectedCorner != null || telemetryWindow != null} rangeLabel={selectedCorner?.label ?? (sector != null ? `SECTOR ${sector}` : telemetryWindow ? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M` : undefined)} corners={corners} selectedCornerIndex={cornerIndex} onRangeZoom={zoomTelemetry} rangeZoomLinked={mapZoomLinked} onRangeZoomLinked={setMapZoomLinked} onDismiss={mapPip.dismiss} />}
            </div>
          )}
          <ComparisonHud comparison={comparison} sessions={eligibleSessions} compatibleSessions={compatibleSessions} referenceSessionId={referenceSessionId} onReferenceSession={setReferenceSessionId} referenceLaps={referenceLaps} referenceLap={referenceLap} onReferenceLap={(value) => { setReferenceLap(value); if (comparisonSessionId === referenceSessionId && comparisonLap === value) setComparisonLap(referenceLaps.find((lap) => lap.index !== value)?.index ?? null); }} comparisonSessionId={comparisonSessionId} onComparisonSession={setComparisonSessionId} comparisonLaps={comparisonLaps} comparisonLap={comparisonLap} onComparisonLap={setComparisonLap} onSwap={() => { if (referenceLap == null || comparisonLap == null) return; skipReferenceDefaults.current = true; skipComparisonDefaults.current = true; setReferenceSessionId(comparisonSessionId); setReferenceLap(comparisonLap); setComparisonSessionId(referenceSessionId); setComparisonLap(referenceLap); }} samples={samples} sector={sector} onSector={(value) => { setSector(value); setTelemetryWindow(null); setCornerIndex(null); setCursorIndex(null); }} cursorIndex={cursorIndex} onSeek={setCursorIndex} />
        </>
      )}
    </>
  );
}

function validComparisonLaps(session: RecordedSessionSummary) {
  return session.laps.filter((lap) => !lapIsInvalid(lap) && lap.time !== "—").slice().sort((left, right) => lapDuration(left) - lapDuration(right));
}

type ComparisonValueKey = keyof Pick<LapComparisonSample, "referenceSpeedKmh" | "comparisonSpeedKmh" | "referenceThrottlePercent" | "comparisonThrottlePercent" | "referenceBrakePercent" | "comparisonBrakePercent" | "referenceSteeringPercent" | "comparisonSteeringPercent" | "referenceRpm" | "comparisonRpm" | "referenceGear" | "comparisonGear">;
type ComparisonChartSeries = { label: string; colour: string; value: (sample: LapComparisonSample) => number | null | undefined };

const channelColours = {
  speed: "#45d6e8",
  throttle: "#42db76",
  brake: "#ff5263",
  gear: "#5394ff",
  steering: "#f2f3f5",
  rpm: "#ffb84d",
  faster: "var(--color-trace-purple)",
  delta: "#e8eaed",
  mapBrake: "#c13b4a",
};

function comparisonSeries(reference: ComparisonValueKey, comparison: ComparisonValueKey, colour: string, comparisonIsFaster: boolean): ComparisonChartSeries[] {
  return [
    { label: "REFERENCE", colour: comparisonIsFaster ? colour : channelColours.faster, value: (sample) => sample[reference] },
    { label: "ANALYSED LAP", colour: comparisonIsFaster ? channelColours.faster : colour, value: (sample) => sample[comparison] },
  ];
}

function singleSeries(key: ComparisonValueKey, colour: string): ComparisonChartSeries[] {
  return [{ label: "LAP", colour, value: (sample) => sample[key] }];
}

function filterSamplesBySector(samples: LapComparisonSample[], sector: number | null) {
  return sector == null ? samples : samples.filter((sample) => sample.sectorIndex === sector);
}

function filterSamplesByDistance(samples: LapComparisonSample[], startDistanceM: number, endDistanceM: number) {
  return samples.filter((sample) => sample.distanceM >= startDistanceM && sample.distanceM <= endDistanceM);
}

type TelemetryWindow = { startM: number; endM: number };

function filterSamplesByTelemetryWindow(samples: LapComparisonSample[], window: TelemetryWindow | null) {
  if (window == null) return samples;
  return samples.filter((sample) => sample.distanceM >= window.startM && sample.distanceM <= window.endM);
}

function nextTelemetryWindow(samples: LapComparisonSample[], current: TelemetryWindow | null, anchorM: number, direction: "in" | "out"): TelemetryWindow | null {
  const baseStart = samples[0]?.distanceM;
  const baseEnd = samples.at(-1)?.distanceM;
  if (baseStart == null || baseEnd == null || baseEnd <= baseStart) return current;
  const start = current?.startM ?? baseStart;
  const end = current?.endM ?? baseEnd;
  const span = end - start;
  const baseSpan = baseEnd - baseStart;
  const minimumSpan = Math.max(50, baseSpan / 64);
  const nextSpan = Math.min(baseSpan, Math.max(minimumSpan, span * (direction === "in" ? 0.7 : 1 / 0.7)));
  if (nextSpan >= baseSpan * 0.995) return null;
  const anchor = Math.min(end, Math.max(start, anchorM));
  const ratio = span <= 0 ? 0.5 : (anchor - start) / span;
  let nextStart = anchor - nextSpan * ratio;
  let nextEnd = nextStart + nextSpan;
  if (nextStart < baseStart) {
    nextStart = baseStart;
    nextEnd = baseStart + nextSpan;
  }
  if (nextEnd > baseEnd) {
    nextEnd = baseEnd;
    nextStart = baseEnd - nextSpan;
  }
  return { startM: nextStart, endM: nextEnd };
}

function SavedComparisonsDock({ savedComparisons, suggestions, sessions, defaultName, canSave, currentReferenceSessionId, currentReferenceLap, currentAnalysedSessionId, currentAnalysedLap, onOpen, onSelectSuggestion, onSave, onDelete, onRename }: { savedComparisons: SavedComparison[]; suggestions: ReferenceSuggestion[]; sessions: RecordedSessionSummary[]; defaultName: string; canSave: boolean; currentReferenceSessionId: string; currentReferenceLap: number | null; currentAnalysedSessionId: string; currentAnalysedLap: number | null; onOpen: (comparison: SavedComparison) => void; onSelectSuggestion: (suggestion: ReferenceSuggestion) => void; onSave: (name: string) => Promise<boolean>; onDelete: (id: string) => Promise<void>; onRename: (id: string, name: string) => Promise<boolean> }) {
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
  const currentSaved = savedComparisons.some((saved) => saved.referenceSessionId === currentReferenceSessionId && saved.referenceLapIndex === currentReferenceLap && saved.analysedSessionId === currentAnalysedSessionId && saved.analysedLapIndex === currentAnalysedLap);
  const titlebarActions = typeof document === "undefined" ? null : document.getElementById("trace-titlebar-actions");
  return (
    <>
      {titlebarActions && createPortal(<Tooltip content={canSave ? "Save the current lap comparison" : "Choose two laps before saving a comparison"} className="h-12">
        <button type="button" disabled={!canSave} onClick={() => setSaveOpen((value) => !value)} className={`grid size-12 place-items-center border-l border-trace-divider bg-trace-black ${saveOpen ? "text-trace-accent" : "text-trace-muted hover:bg-trace-raised hover:text-trace-text"} disabled:cursor-not-allowed disabled:text-trace-dim`} aria-label="Save current comparison" aria-expanded={saveOpen}>
          <svg className={`size-4 stroke-current ${currentSaved ? "fill-trace-accent" : "fill-none"}`} viewBox="0 0 16 16" aria-hidden="true"><path d="m8 1.5 1.9 3.85 4.25.62-3.08 3 .73 4.23L8 11.2l-3.8 2 .73-4.23-3.08-3 4.25-.62Z" /></svg>
        </button>
      </Tooltip>, titlebarActions)}
      {saveOpen && <form onSubmit={async (event) => { event.preventDefault(); if (!draftName.trim()) return; setSaving(true); const saved = await onSave(draftName); setSaving(false); if (saved) setSaveOpen(false); }} className="fixed right-[232px] top-12 z-[70] flex w-[380px] items-end gap-3 border border-trace-divider bg-trace-black p-3 shadow-[0_18px_45px_rgba(0,0,0,.6)]">
        <label className="min-w-0 flex-1 font-mono text-[9px] font-bold tracking-[.08em] text-trace-dim" htmlFor="saved-comparison-name">COMPARISON NAME<input id="saved-comparison-name" value={draftName} onChange={(event) => setDraftName(event.target.value)} maxLength={80} autoFocus className="mt-1.5 h-9 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-sans font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" /></label>
        <button type="button" onClick={() => setSaveOpen(false)} className="h-9 px-2 font-mono text-[10px] font-bold text-trace-dim hover:text-trace-text">CANCEL</button>
        <button type="submit" disabled={saving || !draftName.trim()} className="h-9 bg-trace-accent px-4 font-mono text-[10px] font-black text-trace-black disabled:opacity-40">{saving ? "SAVING…" : "SAVE"}</button>
      </form>}
      <aside className="fixed bottom-[239px] right-6 z-[31] w-[620px] max-w-[calc(100vw-248px)] border border-trace-divider bg-trace-black/95 shadow-[0_-12px_35px_rgba(0,0,0,.42)] backdrop-blur" aria-label="Comparison lap dock">
        <div className="flex h-10 items-stretch">
          <button type="button" onClick={() => { setActiveTab("saved"); setDockCollapsed(false); setMenuId(null); }} className={`flex min-w-0 flex-1 items-center gap-1.5 border-r border-trace-divider px-4 text-left font-mono hover:bg-trace-deep ${!dockCollapsed && activeTab === "saved" ? "bg-trace-deep text-trace-text" : "text-trace-soft"}`} aria-selected={!dockCollapsed && activeTab === "saved"} role="tab"><strong className="truncate text-[10px] tracking-[.1em]">SAVED COMPARISONS</strong><span className="shrink-0 text-[10px] font-black text-trace-accent">[{savedComparisons.length}]</span></button>
          <button type="button" onClick={() => { setActiveTab("suggested"); setDockCollapsed(false); setMenuId(null); }} className={`flex min-w-0 flex-1 items-center gap-1.5 px-4 text-left font-mono hover:bg-trace-deep ${!dockCollapsed && activeTab === "suggested" ? "bg-trace-deep text-trace-text" : "text-trace-soft"}`} aria-selected={!dockCollapsed && activeTab === "suggested"} role="tab"><strong className="truncate text-[10px] tracking-[.1em]">SUGGESTED REFERENCES</strong><span className={`shrink-0 text-[10px] font-black ${suggestions.length > 0 ? "text-trace-purple" : "text-trace-dim"}`}>[{suggestions.length}]</span></button>
          <button type="button" onClick={() => { setDockCollapsed(!dockCollapsed); setMenuId(null); }} className="grid w-10 shrink-0 place-items-center border-l border-trace-divider text-trace-dim hover:bg-trace-deep hover:text-trace-text" aria-label={dockCollapsed ? "Open comparison lap dock" : "Collapse comparison lap dock"} aria-expanded={!dockCollapsed}><svg className={`size-3 fill-none stroke-current transition-transform ${dockCollapsed ? "" : "rotate-180"}`} viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 4 3.5 3.5L9.5 4" /></svg></button>
        </div>
        {!dockCollapsed && <div className="max-h-72 overflow-x-hidden overflow-y-auto border-t border-trace-divider">
        {activeTab === "saved" ? <>
        {savedComparisons.length === 0 ? <div className="px-5 py-8 text-center"><strong className="block text-[13px] text-trace-soft">No favourite comparisons yet</strong><p className="mx-auto mt-2 max-w-md text-[11px] leading-5 text-trace-dim">Choose a useful Reference and Analysed Lap, then use the title-bar star to save it here.</p></div> : <div className="grid gap-2 p-2">
          {savedComparisons.map((saved) => {
            const referenceSession = sessions.find((session) => session.id === saved.referenceSessionId);
            const analysedSession = sessions.find((session) => session.id === saved.analysedSessionId);
            const referenceDriver = savedComparisonDriver(referenceSession);
            const analysedDriver = savedComparisonDriver(analysedSession);
            const current = saved.referenceSessionId === currentReferenceSessionId && saved.referenceLapIndex === currentReferenceLap && saved.analysedSessionId === currentAnalysedSessionId && saved.analysedLapIndex === currentAnalysedLap;
            return <article className={`relative w-full border bg-trace-surface ${current ? "border-trace-accent/70 outline outline-1 -outline-offset-1 outline-trace-accent/30" : "border-trace-divider hover:border-trace-soft"}`} key={saved.id}>
              <button type="button" onClick={() => { onOpen(saved); setDockCollapsed(true); }} className="block w-full text-left">
              <div className="flex items-center justify-between gap-3 border-b border-trace-divider px-3 py-1.5 pr-12"><span className="min-w-0"><strong className="block truncate text-[13px] leading-4 text-trace-text">{saved.name}</strong><span className="mt-0.5 block truncate font-mono text-[10px] font-bold leading-4 text-trace-dim">{saved.track} · {saved.car}</span></span>{current && <span className="inline-flex h-5 shrink-0 items-center justify-center border border-trace-accent/40 bg-trace-accent-wash px-1.5 font-mono text-[9px] font-black leading-none text-trace-accent">CURRENT</span>}</div>
              <div className="grid grid-cols-[1fr_auto_1fr] items-stretch gap-2 px-3 py-1.5">
                <SavedComparisonLap role="REFERENCE" driver={referenceDriver} lapIndex={saved.referenceLapIndex} durationNs={saved.referenceDurationNs} startedAt={saved.referenceStartedAt} accent="text-trace-purple" />
                <span className="self-center font-mono text-[9px] font-black text-trace-dim">VS</span>
                <SavedComparisonLap role="ANALYSED" driver={analysedDriver} lapIndex={saved.analysedLapIndex} durationNs={saved.analysedDurationNs} startedAt={saved.analysedStartedAt} accent="text-trace-accent" alignRight />
              </div>
              <div className="border-t border-trace-divider px-3 py-1 font-mono text-[9px] font-bold leading-4 text-trace-dim">SAVED {formatCompactSessionDate(saved.createdAt)}</div>
              </button>
              <div className="absolute right-1.5 top-1.5 z-10"><button type="button" onClick={() => setMenuId((value) => value === saved.id ? null : saved.id)} className="grid size-8 place-items-center border border-transparent text-trace-dim hover:border-trace-divider hover:bg-trace-deep hover:text-trace-text" aria-label={`Actions for ${saved.name}`} aria-expanded={menuId === saved.id}><svg className="h-1 w-3.5 fill-current" viewBox="0 0 14 2" aria-hidden="true"><circle cx="1" cy="1" r="1" /><circle cx="7" cy="1" r="1" /><circle cx="13" cy="1" r="1" /></svg></button>{menuId === saved.id && <div className="absolute right-0 top-9 w-28 border border-trace-divider bg-trace-black p-1 shadow-[0_10px_25px_rgba(0,0,0,.55)]"><button type="button" onClick={() => { setRenameId(saved.id); setRenameDraft(saved.name); setMenuId(null); }} className="block w-full px-2 py-2 text-left font-mono text-[9px] font-bold text-trace-muted hover:bg-trace-deep hover:text-trace-text">RENAME</button><button type="button" onClick={() => { setMenuId(null); void onDelete(saved.id); }} className="block w-full px-2 py-2 text-left font-mono text-[9px] font-bold text-[#ff5263] hover:bg-trace-danger/20">DELETE</button></div>}</div>
              {renameId === saved.id && <form onSubmit={async (event) => { event.preventDefault(); if (!renameDraft.trim()) return; setRenaming(true); const renamed = await onRename(saved.id, renameDraft); setRenaming(false); if (renamed) setRenameId(null); }} className="flex items-end gap-2 border-t border-trace-divider bg-trace-deep p-2" onClick={(event) => event.stopPropagation()}><label className="min-w-0 flex-1 font-mono text-[8px] font-bold text-trace-dim" htmlFor={`rename-${saved.id}`}>NEW NAME<input id={`rename-${saved.id}`} value={renameDraft} onChange={(event) => setRenameDraft(event.target.value)} maxLength={80} autoFocus className="mt-1 h-8 w-full border border-trace-divider bg-trace-black px-2 font-sans text-[11px] font-normal text-trace-text outline-none focus:border-trace-accent" /></label><button type="button" onClick={() => setRenameId(null)} className="h-8 px-2 font-mono text-[9px] font-bold text-trace-dim hover:text-trace-text">CANCEL</button><button type="submit" disabled={renaming || !renameDraft.trim()} className="h-8 bg-trace-accent px-3 font-mono text-[9px] font-black text-trace-black disabled:opacity-40">{renaming ? "…" : "SAVE"}</button></form>}
            </article>;
          })}
        </div>}
        </> : <SuggestedReferencesList suggestions={suggestions} currentSessionId={currentReferenceSessionId} currentLapIndex={currentReferenceLap} onSelect={(suggestion) => { onSelectSuggestion(suggestion); setDockCollapsed(true); }} />}
        </div>}
      </aside>
    </>
  );
}

function SavedComparisonLap({ role, driver, lapIndex, durationNs, startedAt, accent, alignRight = false }: { role: string; driver: string; lapIndex: number; durationNs: number; startedAt: string; accent: string; alignRight?: boolean }) {
  return <div className={`min-w-0 font-mono ${alignRight ? "text-right" : ""}`}><span className={`block text-[10px] font-black leading-3 tracking-[.08em] ${accent}`}>{role}</span><strong className="mt-0.5 block truncate font-sans text-[13px] leading-4 text-trace-text">{driver}</strong><strong className="mt-0.5 block text-[15px] leading-5 tabular-nums text-trace-soft">{formatLapDurationNs(durationNs)}</strong><span className="block truncate text-[10px] font-bold leading-4 text-trace-dim">LAP {lapIndex} · {formatCompactSessionDate(startedAt)}</span></div>;
}

function savedComparisonDriver(session?: RecordedSessionSummary) {
  return session?.driver?.trim() || session?.title?.trim() || "Unknown driver";
}

function CornerAnalysisPanel({ corners, selectedCornerIndex, comparisonIsFaster, collapsed, onCollapsed, onSelect }: { corners: CornerAnalysis[]; selectedCornerIndex: number | null; comparisonIsFaster: boolean; collapsed: boolean; onCollapsed: (collapsed: boolean) => void; onSelect: (index: number) => void }) {
  const opportunities = corners
    .filter((corner) => corner.totalLossSeconds != null && corner.totalLossSeconds > 0.005)
    .slice()
    .sort((left, right) => (right.totalLossSeconds ?? 0) - (left.totalLossSeconds ?? 0))
    .slice(0, 4);
  return (
    <section className={`fixed bottom-[252px] left-[204px] top-[76px] z-50 overflow-y-auto border border-trace-divider bg-trace-surface shadow-[0_18px_55px_rgba(0,0,0,.45)] transition-[width] ${collapsed ? "w-11" : "w-72"}`} aria-label="Rule-based lap analysis">
      <div className={`flex border-b border-trace-divider ${collapsed ? "h-11 items-center justify-center" : "min-h-16 items-center justify-between gap-3 p-3"}`}>
        {!collapsed && <div className="min-w-0"><strong className="block font-mono text-[11px] tracking-[.1em] text-trace-soft">ANALYSIS</strong><span className="mt-1 block text-[11px] leading-4 text-trace-dim">Rule-based comparison · No AI</span><span className="mt-1 block text-[10px] leading-4 text-trace-faint">{comparisonIsFaster ? "Analysed Lap is faster than Reference" : "Analysed Lap against faster Reference"}</span></div>}
        <button type="button" onClick={() => onCollapsed(!collapsed)} className="grid size-8 shrink-0 place-items-center text-trace-muted hover:bg-trace-deep hover:text-trace-text" aria-label={collapsed ? "Show analysis" : "Hide analysis"} aria-expanded={!collapsed}>
          <svg className={`size-4 fill-none stroke-current transition-transform ${collapsed ? "" : "rotate-180"}`} viewBox="0 0 16 16" aria-hidden="true"><path d="m6 3 5 5-5 5" /></svg>
        </button>
      </div>
      {collapsed ? (
        <button type="button" onClick={() => onCollapsed(false)} className="flex w-full items-center justify-center py-4 font-mono text-[10px] font-bold tracking-[.12em] text-trace-dim hover:text-trace-text" aria-label="Show analysis">
          <span className="[writing-mode:vertical-rl]">ANALYSIS</span>
        </button>
      ) : (
        <div>
          <p className="border-b border-trace-divider bg-trace-warning/5 px-3 py-2 text-[10px] leading-4 text-trace-dim">
            This analysis uses simple telemetry rules and may be incorrect. Verify its suggestions against the graphs and track map.
          </p>
          {opportunities.length === 0 ? <p className="px-4 py-4 text-[12px] leading-5 text-trace-dim">The rule-based comparison did not detect any meaningful corner losses.</p> : <div className="divide-y divide-trace-divider">
          {selectedCornerIndex != null && <button type="button" onClick={() => onSelect(selectedCornerIndex)} className="w-full px-4 py-3 text-left font-mono text-[10px] font-bold tracking-[.08em] text-trace-accent hover:bg-trace-deep hover:text-trace-text">SHOW FULL LAP</button>}
          {opportunities.map((corner) => {
            const selected = selectedCornerIndex === corner.index;
            const dominant = dominantCornerPhase(corner);
            return (
              <button type="button" onClick={() => onSelect(corner.index)} className={`block w-full min-w-0 px-4 py-3 text-left transition-colors ${selected ? "bg-trace-accent-wash outline outline-1 -outline-offset-1 outline-trace-accent" : "hover:bg-trace-deep"}`} aria-pressed={selected} key={corner.index}>
                <span className="flex items-baseline justify-between gap-3 font-mono"><strong className="text-[15px] text-trace-text">{corner.label}</strong><strong className="text-[15px] tabular-nums text-[#ff5263]">+{corner.totalLossSeconds?.toFixed(3)}s</strong></span>
                <span className="mt-2 block truncate text-[10px] font-black tracking-[.08em] text-trace-dim">MOST LOSS · {dominant}</span>
                <span className="mt-1 block truncate text-[11px] text-trace-muted">{cornerSummary(corner, dominant)}</span>
                <span className="mt-3 grid grid-cols-3 gap-1 border-t border-trace-divider pt-2" aria-label={`${corner.label} time difference by phase`}>
                  {corner.phases.map((phase) => (
                    <span className="min-w-0" key={phase.phase}>
                      <span className="block truncate font-mono text-[9px] font-bold tracking-[.06em] text-trace-dim">{cornerPhaseLabel(phase.phase)}</span>
                      <span className={`mt-0.5 block font-mono text-[11px] font-bold tabular-nums ${phase.lossSeconds == null ? "text-trace-dim" : phase.lossSeconds > 0 ? "text-[#ff5263]" : "text-[#42db76]"}`}>{formatPhaseDelta(phase.lossSeconds)}</span>
                    </span>
                  ))}
                </span>
              </button>
            );
          })}
          </div>}
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
  return corner.phases
    .filter((phase) => phase.lossSeconds != null)
    .reduce((dominant, phase) => (phase.lossSeconds ?? Number.NEGATIVE_INFINITY) > (dominant?.lossSeconds ?? Number.NEGATIVE_INFINITY) ? phase : dominant, null as CornerAnalysis["phases"][number] | null)
    ?.phase.toUpperCase() ?? "UNAVAILABLE";
}

function cornerSummary(corner: CornerAnalysis, dominantPhase: string) {
  const minimumSpeedDifference = corner.metrics.comparisonMinimumSpeedKmh != null && corner.metrics.referenceMinimumSpeedKmh != null
    ? corner.metrics.comparisonMinimumSpeedKmh - corner.metrics.referenceMinimumSpeedKmh
    : null;
  if (minimumSpeedDifference != null && minimumSpeedDifference < -1) return `${Math.round(Math.abs(minimumSpeedDifference))} km/h lower minimum speed`;
  const throttleDifference = corner.metrics.comparisonThrottlePointM != null && corner.metrics.referenceThrottlePointM != null
    ? corner.metrics.comparisonThrottlePointM - corner.metrics.referenceThrottlePointM
    : null;
  if (throttleDifference != null && throttleDifference > 3) return `Throttle applied ${Math.round(throttleDifference)} m later`;
  return `Loss develops through ${dominantPhase.toLowerCase()}`;
}

function SectorPicker({ samples, value, onChange }: { samples: LapComparisonSample[]; value: number | null; onChange: (value: number | null) => void }) {
  const sectors = [...new Set(samples.flatMap((sample) => sample.sectorIndex == null ? [] : [sample.sectorIndex]))].sort((left, right) => left - right);
  return (
    <div className="flex items-center gap-2" aria-label="Telemetry range">
      <span className="mr-2 font-mono text-[12px] font-bold tracking-[.1em] text-trace-dim">VIEW</span>
      <button type="button" onClick={() => onChange(null)} className={`border px-3 py-2 font-mono text-[12px] font-bold ${value == null ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}>FULL LAP</button>
      {sectors.map((item) => <button type="button" onClick={() => onChange(item)} className={`border px-3 py-2 font-mono text-[12px] font-bold ${value === item ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`} key={item}>SECTOR {item}</button>)}
    </div>
  );
}

function TelemetryHud({ session, lapIndex, samples, cursorIndex, onSeek }: { session: RecordedSessionSummary; lapIndex: number; samples: LapComparisonSample[]; cursorIndex: number | null; onSeek: (index: number) => void }) {
  const sample = samples[cursorIndex ?? 0] ?? null;
  const airTemperature = sample?.referenceAirTemperatureC ?? numericCondition(session.ambientTemperatureC);
  const trackTemperature = sample?.referenceTrackTemperatureC ?? numericCondition(session.roadTemperatureC);
  const showConditions = hasHudConditions(airTemperature, trackTemperature);
  return (
    <div className={`fixed bottom-12 left-[200px] right-6 z-30 grid h-[108px] ${showConditions ? "grid-cols-[minmax(190px,1fr)_95px_130px_90px_100px_130px_130px_150px]" : "grid-cols-[minmax(190px,1fr)_95px_130px_90px_100px_130px_130px]"} grid-rows-[28px_48px] items-center gap-x-4 gap-y-2 overflow-hidden border border-trace-divider bg-trace-black/95 px-5 py-3 shadow-[0_12px_40px_rgba(0,0,0,.55)] backdrop-blur`}>
      <div className="col-span-full min-w-0 border-b border-trace-divider pb-2">
        <TelemetrySeek samples={samples} cursorIndex={cursorIndex} onSeek={onSeek} />
      </div>
      <div className="min-w-0"><span className="block truncate text-[13px] font-black">{session.track} · {session.car}</span><span className="font-mono text-[11px] text-trace-dim">LAP {lapIndex} · {friendlySessionType(session)}</span></div>
      <HudValue label="DISTANCE" value={sample ? `${Math.round(sample.distanceM)} M` : "—"} />
      <HudValue label="SPEED / GEAR" value={sample?.referenceSpeedKmh == null ? "—" : `${Math.round(sample.referenceSpeedKmh)} · ${formatGear(sample.referenceGear)}`} colour={channelColours.speed} />
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

function ComparisonHud({ comparison, sessions, compatibleSessions, referenceSessionId, onReferenceSession, referenceLaps, referenceLap, onReferenceLap, comparisonSessionId, onComparisonSession, comparisonLaps, comparisonLap, onComparisonLap, onSwap, samples, sector, onSector, cursorIndex, onSeek }: ComparisonHudProps) {
  const sample = samples[cursorIndex ?? 0] ?? null;
  const finalDelta = comparison?.samples.slice().reverse().find((candidate) => candidate.deltaSeconds != null)?.deltaSeconds;
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
  const gapTone = sample?.deltaSeconds == null || Math.abs(sample.deltaSeconds) < 0.0005 ? "text-trace-text" : sample.deltaSeconds > 0 ? "text-[#ff5263]" : "text-[#42db76]";
  const referenceLabel = referenceSession?.driver?.trim() || "REFERENCE";
  const comparisonLabel = comparisonSession?.driver?.trim() || "ANALYSED LAP";
  const trackCarLabel = referenceSession ? `${referenceSession.track} · ${referenceSession.car}` : "TRACK · CAR";
  return (
    <>
      <div className={`fixed bottom-12 left-[200px] right-6 z-30 grid h-[192px] ${showConditions ? "grid-cols-[120px_72px_minmax(160px,260px)_112px_minmax(120px,1fr)_112px_minmax(160px,260px)_72px_120px]" : "grid-cols-[120px_72px_minmax(180px,320px)_minmax(120px,1fr)_minmax(180px,320px)_72px_120px]"} grid-rows-[28px_45px_44px_27px] items-center justify-center gap-x-3 gap-y-2 overflow-hidden border border-trace-divider bg-trace-black/95 px-5 py-3 shadow-[0_12px_40px_rgba(0,0,0,.55)] backdrop-blur`}>
      <div className="col-span-full min-w-0 border-b border-trace-divider pb-2">
        <TelemetrySeek samples={samples} cursorIndex={cursorIndex} onSeek={onSeek} />
      </div>
      <div className="col-span-full grid grid-cols-[1fr_52px_1fr] gap-3 border-b border-trace-divider pb-2">
        <HudLapChoice label={referenceLabel} role={comparisonIsFaster ? "SLOWER BASELINE" : "FASTER REFERENCE"} colour={comparisonIsFaster ? "text-trace-accent" : "text-trace-purple"} sessions={sessions} sessionId={referenceSessionId} onSession={onReferenceSession} laps={referenceLaps} lapIndex={referenceLap} onLap={onReferenceLap} />
        <div className="flex min-w-0 items-center justify-center">
          <button type="button" disabled={referenceLap == null || comparisonLap == null} onClick={onSwap} className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-deep text-trace-muted hover:border-trace-soft hover:text-trace-text disabled:text-trace-dim" aria-label="Swap Reference and Analysed Lap"><svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 5h9m0 0L9.5 2.5M12 5 9.5 7.5M13 11H4m0 0 2.5-2.5M4 11l2.5 2.5" /></svg></button>
        </div>
        <HudLapChoice label={comparisonLabel} role={comparisonIsFaster ? "FASTER SELECTED LAP" : "LAP TO IMPROVE"} colour={comparisonIsFaster ? "text-trace-purple" : "text-trace-accent"} sessions={compatibleSessions} sessionId={comparisonSessionId} onSession={onComparisonSession} laps={comparisonLaps} lapIndex={comparisonLap} onLap={onComparisonLap} disabledLap={comparisonSessionId === referenceSessionId ? referenceLap : null} />
      </div>
      <HudValue label="REFERENCE SPEED / GEAR" value={sample?.referenceSpeedKmh == null ? "—" : `${Math.round(sample.referenceSpeedKmh)} · ${formatGear(sample.referenceGear)}`} colour={comparisonIsFaster ? "var(--color-trace-accent)" : channelColours.faster} />
      <HudSteering value={sample?.referenceSteeringPercent} colour={comparisonIsFaster ? "var(--color-trace-accent)" : channelColours.faster} />
      <HudPedals throttle={sample?.referenceThrottlePercent} brake={sample?.referenceBrakePercent} />
      {showConditions && (referenceHasConditions ? <HudConditions air={referenceAirTemperature} track={referenceTrackTemperature} /> : <div aria-hidden="true" />)}
      <div className="h-10 min-w-0 overflow-hidden border-x border-trace-divider px-3 text-center font-mono">
        <span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">LIVE GAP</span>
        <strong className={`mt-1 block truncate text-[13px] leading-5 tabular-nums ${gapTone}`}>{sample == null ? "—" : `${Math.round(sample.distanceM)} M · ${formatComparisonGap(sample.deltaSeconds)}`}</strong>
      </div>
      {showConditions && (comparisonHasConditions ? <HudConditions air={comparisonAirTemperature} track={comparisonTrackTemperature} /> : <div aria-hidden="true" />)}
      <HudPedals throttle={sample?.comparisonThrottlePercent} brake={sample?.comparisonBrakePercent} />
      <HudSteering value={sample?.comparisonSteeringPercent} colour={comparisonIsFaster ? channelColours.faster : "var(--color-trace-accent)"} />
      <HudValue label="ANALYSED SPEED / GEAR" value={sample?.comparisonSpeedKmh == null ? "—" : `${Math.round(sample.comparisonSpeedKmh)} · ${formatGear(sample.comparisonGear)}`} colour={comparisonIsFaster ? channelColours.faster : "var(--color-trace-accent)"} />
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

function fasterReferenceSuggestions(sessions: RecordedSessionSummary[], targetSession: RecordedSessionSummary | undefined, targetLap: RecordedSessionSummary["laps"][number] | null, targetSessionId: string, targetLapIndex: number | null) {
  if (!targetSession || !targetLap) return [];
  const targetDuration = lapDuration(targetLap);
  return sessions
    .filter((session) => session.simulatorId === targetSession.simulatorId && session.track === targetSession.track && session.car === targetSession.car)
    .flatMap((session) => validComparisonLaps(session)
      .filter((lap) => lapDuration(lap) < targetDuration && (session.id !== targetSessionId || lap.index !== targetLapIndex))
      .map((lap): ReferenceSuggestion => ({ session, lap, imported: sessionSourceGroup(session) === "imported", gainSeconds: (targetDuration - lapDuration(lap)) / 1_000_000_000 })))
    .sort((left, right) => Number(right.imported) - Number(left.imported) || lapDuration(left.lap) - lapDuration(right.lap))
    .slice(0, 10);
}

function SuggestedReferencesList({ suggestions, currentSessionId, currentLapIndex, onSelect }: { suggestions: ReferenceSuggestion[]; currentSessionId: string; currentLapIndex: number | null; onSelect: (suggestion: ReferenceSuggestion) => void }) {
  return (
    <>
        <p className="border-b border-trace-divider px-4 py-2 text-[11px] leading-4 text-trace-dim">Clean laps that are faster than the Analysed Lap. Imported references are listed first.</p>
        {suggestions.length === 0 ? <p className="px-4 py-4 text-[12px] text-trace-muted">No faster lap with the same simulator, car, track, and layout has been recorded or imported.</p> : suggestions.map((suggestion) => {
          const identity = suggestion.session.driver ?? suggestion.session.title ?? formatSessionDate(suggestion.session.startedAt);
          const current = suggestion.session.id === currentSessionId && suggestion.lap.index === currentLapIndex;
          return <button type="button" onClick={() => onSelect(suggestion)} className={`grid w-full grid-cols-[minmax(0,1fr)_100px_82px] items-center gap-4 border-b border-trace-divider px-4 py-3 text-left last:border-b-0 ${current ? "bg-trace-purple-wash" : "hover:bg-trace-deep"}`} aria-current={current ? "true" : undefined} key={`${suggestion.session.id}-${suggestion.lap.index}`}>
            <span className="min-w-0"><strong className="block truncate text-[12px] text-trace-text">{identity}</strong><span className="mt-1 flex items-center gap-2 font-mono text-[9px] font-bold tracking-[.07em] text-trace-dim">{suggestion.imported && <span className="text-trace-purple">IMPORTED</span>}<span>{suggestion.session.sessionType}</span><span>{formatSessionDate(suggestion.session.startedAt)}</span>{current && <span className="text-trace-accent">CURRENT</span>}</span></span>
            <span className="font-mono text-right"><strong className="block text-[12px] text-trace-soft">{suggestion.lap.time}</strong><span className="mt-1 block text-[9px] text-trace-dim">LAP {suggestion.lap.index}</span></span>
            <strong className="font-mono text-right text-[12px] tabular-nums text-trace-purple">−{suggestion.gainSeconds.toFixed(3)}s</strong>
          </button>;
        })}
    </>
  );
}

type SectorDelta = { index: number; seconds: number | null };

function comparisonSectorDeltas(referenceLaps: RecordedSessionSummary["laps"], referenceLap: number | null, comparisonLaps: RecordedSessionSummary["laps"], comparisonLap: number | null, samples: LapComparisonSample[]): SectorDelta[] {
  const reference = referenceLaps.find((lap) => lap.index === referenceLap);
  const comparison = comparisonLaps.find((lap) => lap.index === comparisonLap);
  const indices = [...new Set([
    ...(reference?.sectors.map((sector) => sector.index) ?? []),
    ...(comparison?.sectors.map((sector) => sector.index) ?? []),
    ...samples.flatMap((sample) => sample.sectorIndex == null ? [] : [sample.sectorIndex]),
  ])].sort((left, right) => left - right);
  const cumulativeDeltas = new Map(indices.map((index) => [index, samples.slice().reverse().find((sample) => sample.sectorIndex === index && sample.deltaSeconds != null)?.deltaSeconds ?? null]));
  return indices.map((index, position) => {
    const referenceSector = reference?.sectors.find((sector) => sector.index === index);
    const comparisonSector = comparison?.sectors.find((sector) => sector.index === index);
    if (referenceSector && comparisonSector) {
      return { index, seconds: (comparisonSector.durationNs - referenceSector.durationNs) / 1_000_000_000 };
    }
    const cumulativeDelta = cumulativeDeltas.get(index) ?? null;
    const previousDelta = position === 0 ? 0 : cumulativeDeltas.get(indices[position - 1]) ?? null;
    const seconds = cumulativeDelta == null || previousDelta == null ? null : cumulativeDelta - previousDelta;
    return { index, seconds };
  });
}

function ComparisonSectorStrip({ sectors, value, onChange, trackCarLabel }: { sectors: SectorDelta[]; value: number | null; onChange: (value: number | null) => void; trackCarLabel: string }) {
  return (
    <div className="flex h-6 min-w-0 items-stretch gap-3 font-mono" aria-label="Sector comparison and telemetry range">
      <div className="flex min-w-0 flex-1 items-stretch gap-1.5 overflow-x-auto">
        <span className="flex w-28 shrink-0 items-center text-[9px] font-black tracking-[.08em] text-trace-accent">{value == null ? "VIEWING FULL LAP" : `VIEWING SECTOR ${value}`}</span>
        <button type="button" onClick={() => onChange(null)} className={`shrink-0 border px-2 text-[9px] font-black leading-none tracking-[.08em] ${value == null ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}>LAP</button>
        {sectors.map((sector) => {
        const gaining = sector.seconds != null && sector.seconds < -0.0005;
        const losing = sector.seconds != null && sector.seconds > 0.0005;
        const tone = gaining
          ? "border-trace-accent/45 bg-trace-accent/10 text-trace-accent"
          : losing
            ? "border-trace-danger/60 bg-trace-danger/25 text-red-200"
            : "border-trace-divider bg-trace-deep text-trace-muted";
        const selected = value === sector.index ? "outline outline-2 -outline-offset-2 outline-trace-accent" : "hover:border-trace-soft";
        const explanation = sector.seconds == null
          ? `Sector ${sector.index} timing is unavailable.`
          : Math.abs(sector.seconds) < 0.0005
            ? `Sector ${sector.index} was even with the reference.`
            : `The Analysed Lap ${gaining ? "gained" : "lost"} ${Math.abs(sector.seconds).toFixed(3)} seconds against the Reference in sector ${sector.index}.`;
        return (
          <Tooltip content={explanation} key={sector.index}>
            <button type="button" onClick={() => onChange(sector.index)} className={`flex shrink-0 items-center gap-1.5 border px-2 text-[10px] font-bold leading-none tabular-nums ${tone} ${selected}`} aria-pressed={value === sector.index}>
              <span className="text-[9px] opacity-75">S{sector.index}</span>
              <strong>{sector.seconds == null ? "—" : Math.abs(sector.seconds) < 0.0005 ? "0.000" : `${sector.seconds > 0 ? "+" : "−"}${Math.abs(sector.seconds).toFixed(3)}`}</strong>
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

function HudLapChoice({ label, role, colour, sessions, sessionId, onSession, laps, lapIndex, onLap, disabledLap = null }: { label: string; role: string; colour: string; sessions: RecordedSessionSummary[]; sessionId: string; onSession: (value: string) => void; laps: RecordedSessionSummary["laps"]; lapIndex: number | null; onLap: (value: number) => void; disabledLap?: number | null }) {
  return <div className="grid min-w-0 grid-cols-[112px_minmax(150px,1fr)_150px] items-center gap-2"><span className="min-w-0 font-mono"><strong className={`block truncate text-[10px] font-black leading-3 tracking-[.1em] ${colour}`}>{label}</strong><span className="mt-0.5 block truncate text-[8px] font-bold leading-3 tracking-[.07em] text-trace-dim">{role}</span></span><select value={sessionId} onChange={(event) => onSession(event.target.value)} className="trace-select h-9 min-w-0 border border-trace-divider bg-trace-deep px-3 text-[11px] font-bold leading-none text-trace-text outline-none" aria-label={`${label} session`}>{sessions.map((session) => <option value={session.id} key={session.id}>{comparisonSessionLabel(session)}</option>)}</select><select value={lapIndex?.toString() ?? ""} onChange={(event) => onLap(Number(event.target.value))} className="trace-select h-9 border border-trace-divider bg-trace-deep px-3 font-mono text-[11px] font-bold leading-none text-trace-text outline-none" aria-label={`${label} lap`}>{lapIndex == null && <option value="" disabled>No clean lap</option>}{laps.map((lap) => <option value={lap.index} disabled={lap.index === disabledLap} key={lap.index}>Lap {lap.index} · {lap.time}</option>)}</select></div>;
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
      <input className="trace-seek w-full" type="range" min="0" max={Math.max(samples.length - 1, 0)} step="1" value={index} disabled={samples.length < 2} onChange={(event) => onSeek(Number(event.target.value))} aria-label="Seek through lap distance" />
      <span className="text-right">{Math.round(end)} M</span>
    </label>
  );
}

function HudValue({ label, value, unit, colour }: { label: string; value: string; unit?: string; colour?: string }) {
  return <div className="h-10 min-w-0 overflow-hidden font-mono"><span className="block truncate whitespace-nowrap text-[10px] font-bold leading-3 tracking-[.1em] text-trace-dim">{label}</span><strong className="mt-1 block truncate whitespace-nowrap text-[15px] leading-5 tabular-nums" style={{ color: colour }}>{value}{unit && <small className="ml-1 text-[9px] text-trace-dim">{unit}</small>}</strong></div>;
}

function HudSteering({ value, colour }: { value?: number | null; colour: string }) {
  const percent = value == null || !Number.isFinite(value) ? 0 : Math.min(100, Math.max(-100, value));
  const rotation = percent * 4.5;
  return <div className="flex h-10 min-w-0 items-center gap-2 font-mono"><svg className="size-9 shrink-0" viewBox="0 0 36 36" role="img" aria-label={value == null ? "Steering unavailable" : `Steering input ${Math.round(value)} percent`}><circle cx="18" cy="18" r="15" fill="var(--color-trace-deep)" stroke={colour} strokeWidth="2" /><g transform={`rotate(${rotation} 18 18)`} stroke={colour} strokeWidth="2" strokeLinecap="round"><line x1="18" y1="18" x2="18" y2="31" /><line x1="7" y1="17" x2="29" y2="17" /></g><circle cx="18" cy="18" r="2.5" fill={colour} /></svg><span className="min-w-0"><span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">STEER</span><strong className="block truncate text-[12px] leading-4 tabular-nums" style={{ color: colour }}>{value == null ? "—" : `${Math.round(value)}%`}</strong></span></div>;
}

function HudConditions({ air, track }: { air?: number | null; track?: number | null }) {
  return <div className="h-10 min-w-0 overflow-hidden font-mono"><span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">CONDITIONS</span><span className="mt-1 flex items-center gap-2 whitespace-nowrap text-[10px] leading-4 tabular-nums">{air != null && <span className="text-trace-dim">AIR <strong className="text-trace-text">{formatHudTemperature(air)}</strong></span>}{track != null && <span className="text-trace-dim">TRACK <strong className="text-trace-text">{formatHudTemperature(track)}</strong></span>}</span></div>;
}

function hasHudConditions(air?: number | null, track?: number | null) {
  return (air != null && Number.isFinite(air)) || (track != null && Number.isFinite(track));
}

function HudProgress({ label, value, colour }: { label: string; value?: number | null; colour: string }) {
  const primary = Math.min(100, Math.max(0, value ?? 0));
  return <div className="h-10 overflow-hidden font-mono"><span className="flex h-3 justify-between whitespace-nowrap text-[10px] font-bold leading-3 tracking-[.08em] tabular-nums text-trace-dim"><span className="truncate">{label}</span><span>{Math.round(primary)}%</span></span><span className="mt-2 block h-2 overflow-hidden bg-trace-divider"><span className="block h-full transition-[width] duration-75" style={{ width: `${primary}%`, backgroundColor: colour }} /></span></div>;
}

function HudPedals({ throttle, brake }: { throttle?: number | null; brake?: number | null }) {
  return <div className="grid h-10 w-full min-w-0 max-w-80 grid-cols-2 gap-3 justify-self-center"><HudProgress label="THROTTLE" value={throttle} colour={channelColours.throttle} /><HudProgress label="BRAKE" value={brake} colour={channelColours.brake} /></div>;
}

function formatHudTemperature(value?: number | null) {
  return value == null ? "—" : `${Math.round(value)}°C`;
}

function numericCondition(value?: string | null) {
  if (!value) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function useTrackMapPip(active: boolean) {
  const anchor = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    const element = anchor.current;
    if (!active || !element) {
      setVisible(false);
      return;
    }
    const observer = new IntersectionObserver(([entry]) => setVisible(!entry.isIntersecting), { rootMargin: "-56px 0px 0px", threshold: 0.08 });
    observer.observe(element);
    return () => observer.disconnect();
  }, [active]);

  return { anchor, visible: visible && !dismissed, dismiss: () => setDismissed(true) };
}

function FloatingTrackMap({ samples, cursorIndex, comparison = false, comparisonIsFaster = false, trackMap, focusSelection = false, rangeLabel, corners = [], selectedCornerIndex = null, onRangeZoom, rangeZoomLinked = false, onRangeZoomLinked, onDismiss }: { samples: LapComparisonSample[]; cursorIndex: number | null; comparison?: boolean; comparisonIsFaster?: boolean; trackMap?: TrackMapAsset | null; focusSelection?: boolean; rangeLabel?: string; corners?: CornerAnalysis[]; selectedCornerIndex?: number | null; onRangeZoom?: (anchorM: number, direction: "in" | "out") => void; rangeZoomLinked?: boolean; onRangeZoomLinked?: (linked: boolean) => void; onDismiss: () => void }) {
  return <aside className="fixed right-6 top-16 z-40 w-[min(500px,calc(100vw-240px))] overflow-hidden border border-trace-accent/35 bg-trace-black shadow-[0_18px_55px_rgba(0,0,0,.65)]" aria-label="Floating track map"><TrackMap samples={samples} cursorIndex={cursorIndex} comparison={comparison} comparisonIsFaster={comparisonIsFaster} height={260} trackMap={trackMap} focusSelection={focusSelection} rangeLabel={rangeLabel} corners={corners} selectedCornerIndex={selectedCornerIndex} onRangeZoom={onRangeZoom} rangeZoomLinked={rangeZoomLinked} onRangeZoomLinked={onRangeZoomLinked} onDismiss={onDismiss} /></aside>;
}

function closestSampleAtElapsedTime(samples: LapComparisonSample[], key: "referenceElapsedSeconds" | "comparisonElapsedSeconds", targetSeconds: number) {
  return samples.reduce<LapComparisonSample | null>((closest, sample) => {
    const value = sample[key];
    if (value == null || !Number.isFinite(value)) return closest;
    const closestValue = closest?.[key];
    return closestValue == null || Math.abs(value - targetSeconds) < Math.abs(closestValue - targetSeconds) ? sample : closest;
  }, null);
}

function TrackMap({ samples, cursorIndex, comparison = false, comparisonIsFaster = false, height: requestedHeight, trackMap, focusSelection = false, rangeLabel, corners = [], selectedCornerIndex = null, onRangeZoom, rangeZoomLinked = false, onRangeZoomLinked, onDismiss }: { samples: LapComparisonSample[]; cursorIndex: number | null; comparison?: boolean; comparisonIsFaster?: boolean; height?: number; trackMap?: TrackMapAsset | null; focusSelection?: boolean; rangeLabel?: string; corners?: CornerAnalysis[]; selectedCornerIndex?: number | null; onRangeZoom?: (anchorM: number, direction: "in" | "out") => void; rangeZoomLinked?: boolean; onRangeZoomLinked?: (linked: boolean) => void; onDismiss?: () => void }) {
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; panX: number; panY: number } | null>(null);
  const mapViewport = useRef<HTMLDivElement>(null);
  const spatialZoomAt = useRef<(direction: "in" | "out", anchor: readonly [number, number]) => void>(() => undefined);
  const rangeDistanceAt = useRef<(anchor: readonly [number, number]) => number | null>(() => null);
  const displayHeight = requestedHeight ?? (comparison ? 720 : 600);
  const width = 1_000;
  const height = 700;
  useEffect(() => {
    const element = mapViewport.current;
    if (!element) return;
    const handleWheel = (event: WheelEvent) => {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      const svg = element.querySelector("svg");
      const bounds = svg?.getBoundingClientRect();
      const insideMap = bounds != null && event.clientX >= bounds.left && event.clientX <= bounds.right && event.clientY >= bounds.top && event.clientY <= bounds.bottom;
      const anchor = insideMap && bounds
        ? [(event.clientX - bounds.left) / Math.max(bounds.width, 1) * width, (event.clientY - bounds.top) / Math.max(bounds.height, 1) * height] as const
        : [width / 2, height / 2] as const;
      const direction = event.deltaY < 0 ? "in" : "out";
      if (onRangeZoom && rangeZoomLinked) {
        const cursorDistance = cursorIndex == null ? null : samples[cursorIndex]?.distanceM;
        const anchorDistance = insideMap ? rangeDistanceAt.current(anchor) : null;
        onRangeZoom(anchorDistance ?? cursorDistance ?? ((samples[0]?.distanceM ?? 0) + (samples.at(-1)?.distanceM ?? 0)) / 2, direction);
        return;
      }
      spatialZoomAt.current(direction, anchor);
    };
    element.addEventListener("wheel", handleWheel, { passive: false, capture: true });
    return () => element.removeEventListener("wheel", handleWheel, { capture: true });
  }, [cursorIndex, height, onRangeZoom, rangeZoomLinked, samples, width]);
  useEffect(() => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  }, [focusSelection]);
  useEffect(() => {
    if (zoom > 1) setPan((current) => current.x === 0 && current.y === 0 ? current : { x: 0, y: 0 });
    // A cursor move starts a fresh follow position. Retaining pan calculated
    // around the previous target can place the new target outside the view.
  }, [cursorIndex]);
  const padding = focusSelection ? 90 : 42;
  const drivenPoints = samples.flatMap((sample) => [
    sample.referencePositionXM != null && sample.referencePositionZM != null ? [sample.referencePositionXM, sample.referencePositionZM] as const : null,
    comparison && sample.comparisonPositionXM != null && sample.comparisonPositionZM != null ? [sample.comparisonPositionXM, sample.comparisonPositionZM] as const : null,
  ].filter((point): point is readonly [number, number] => point != null && point.every(Number.isFinite)));
  if (drivenPoints.length < 2) return <div className="grid min-h-[340px] place-items-center border border-trace-divider bg-trace-surface p-8 text-center text-[12px] leading-5 text-trace-dim">TRACK POSITION WAS NOT RECORDED<br />FOR THIS LAP</div>;
  const geometryPoints = trackMap ? [...trackMap.leftBoundary, ...trackMap.rightBoundary].map((point) => [point.xM, point.zM] as const) : [];
  const points = focusSelection && drivenPoints.length > 1 ? drivenPoints : geometryPoints.length > 3 ? geometryPoints : drivenPoints;
  const xs = points.map(([x]) => x);
  const zs = points.map(([, z]) => z);
  const minX = Math.min(...xs); const maxX = Math.max(...xs);
  const minZ = Math.min(...zs); const maxZ = Math.max(...zs);
  const scale = Math.min((width - padding * 2) / Math.max(maxX - minX, 1), (height - padding * 2) / Math.max(maxZ - minZ, 1));
  const offsetX = (width - (maxX - minX) * scale) / 2;
  const offsetZ = (height - (maxZ - minZ) * scale) / 2;
  // AC's map projection uses world Z directly as downward screen Y (see map.ini).
  const project = (x: number, z: number) => [offsetX + (x - minX) * scale, offsetZ + (z - minZ) * scale] as const;
  const path = (xKey: "referencePositionXM" | "comparisonPositionXM", zKey: "referencePositionZM" | "comparisonPositionZM") => samples.reduce((result, sample) => {
    const x = sample[xKey]; const z = sample[zKey];
    if (x == null || z == null || !Number.isFinite(x) || !Number.isFinite(z)) return result;
    const [px, py] = project(x, z);
    return `${result}${result ? "L" : "M"}${px.toFixed(2)},${py.toFixed(2)}`;
  }, "");
  const geometryPath = (geometry: TrackMapAsset["centreLine"], close = false) => {
    const result = geometry.reduce((value, point) => {
      const [px, py] = project(point.xM, point.zM);
      return `${value}${value ? "L" : "M"}${px.toFixed(2)},${py.toFixed(2)}`;
    }, "");
    return close && result ? `${result}Z` : result;
  };
  const brakeSegments = (prefix: string, xKey: "referencePositionXM" | "comparisonPositionXM", zKey: "referencePositionZM" | "comparisonPositionZM", brakeKey: "referenceBrakePercent" | "comparisonBrakePercent", strokeWidth: number) => samples.slice(1).flatMap((sample, offset) => {
    const previous = samples[offset];
    const brake = Math.max(previous[brakeKey] ?? 0, sample[brakeKey] ?? 0);
    const x1 = previous[xKey]; const z1 = previous[zKey]; const x2 = sample[xKey]; const z2 = sample[zKey];
    if (brake < 2 || x1 == null || z1 == null || x2 == null || z2 == null || ![x1, z1, x2, z2].every(Number.isFinite)) return [];
    const start = project(x1, z1); const end = project(x2, z2);
    return [<line x1={start[0]} y1={start[1]} x2={end[0]} y2={end[1]} stroke={channelColours.mapBrake} strokeWidth={strokeWidth} strokeLinecap="round" vectorEffect="non-scaling-stroke" key={`${prefix}-${offset}`} />];
  });
  const mapCentre = project((minX + maxX) / 2, (minZ + maxZ) / 2);
  const lineSegments = (line: readonly (readonly [number, number])[]) => line.slice(1).map((point, index) => [line[index], point] as const);
  const projectedGeometry = (line: TrackMapAsset["centreLine"]) => line.map((point) => project(point.xM, point.zM));
  const projectedSamples = (xKey: "referencePositionXM" | "comparisonPositionXM", zKey: "referencePositionZM" | "comparisonPositionZM") => samples.flatMap((sample) => {
    const x = sample[xKey]; const z = sample[zKey];
    return x == null || z == null || !Number.isFinite(x) || !Number.isFinite(z) ? [] : [project(x, z)];
  });
  const obstructionSegments = trackMap
    ? [...lineSegments(projectedGeometry(trackMap.leftBoundary)), ...lineSegments(projectedGeometry(trackMap.rightBoundary)), ...lineSegments(projectedGeometry(trackMap.centreLine))]
    : [...lineSegments(projectedSamples("referencePositionXM", "referencePositionZM")), ...(comparison ? lineSegments(projectedSamples("comparisonPositionXM", "comparisonPositionZM")) : [])];
  const distanceToSegment = (point: readonly [number, number], segment: readonly [readonly [number, number], readonly [number, number]]) => {
    const [start, end] = segment;
    const dx = end[0] - start[0]; const dy = end[1] - start[1];
    const lengthSquared = dx * dx + dy * dy;
    const position = lengthSquared === 0 ? 0 : Math.min(1, Math.max(0, ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / lengthSquared));
    return Math.hypot(point[0] - (start[0] + dx * position), point[1] - (start[1] + dy * position));
  };
  const labelClearance = (point: readonly [number, number]) => obstructionSegments.length === 0 ? Number.POSITIVE_INFINITY : Math.min(...obstructionSegments.map((segment) => distanceToSegment(point, segment)));
  const labelIsSafe = (point: readonly [number, number]) => point[0] >= 20 && point[0] <= width - 20 && point[1] >= 20 && point[1] <= height - 20 && labelClearance(point) >= 20;
  const cornerLabelPoint = (x: number, z: number) => {
    if (trackMap && trackMap.centreLine.length > 0) {
      const nearestIndex = trackMap.centreLine.reduce((nearest, point, index) => {
        const nearestPoint = trackMap.centreLine[nearest];
        const distance = (point.xM - x) ** 2 + (point.zM - z) ** 2;
        const nearestDistance = (nearestPoint.xM - x) ** 2 + (nearestPoint.zM - z) ** 2;
        return distance < nearestDistance ? index : nearest;
      }, 0);
      const centrePoint = trackMap.centreLine[nearestIndex];
      const nearestBoundary = (boundary: TrackMapAsset["centreLine"]) => boundary.reduce((nearest, point) => (point.xM - x) ** 2 + (point.zM - z) ** 2 < (nearest.xM - x) ** 2 + (nearest.zM - z) ** 2 ? point : nearest, boundary[0]);
      const boundaries = [trackMap.leftBoundary, trackMap.rightBoundary].filter((boundary) => boundary.length > 0).map(nearestBoundary);
      if (centrePoint && boundaries.length > 0) {
        const centre = project(centrePoint.xM, centrePoint.zM);
        const directions = boundaries.map((boundary) => {
          const edge = project(boundary.xM, boundary.zM);
          const dx = edge[0] - centre[0]; const dy = edge[1] - centre[1];
          const magnitude = Math.max(Math.hypot(dx, dy), 0.001);
          return { edge, x: dx / magnitude, y: dy / magnitude };
        });
        for (const offset of [24, 32, 42, 54, 70, 90]) {
          const safe = directions.map((direction) => [direction.edge[0] + direction.x * offset, direction.edge[1] + direction.y * offset] as const).filter(labelIsSafe).sort((left, right) => labelClearance(right) - labelClearance(left));
          if (safe[0]) return safe[0];
        }
        return null;
      }
    }
    const apex = project(x, z);
    const dx = apex[0] - mapCentre[0];
    const dy = apex[1] - mapCentre[1];
    const magnitude = Math.max(Math.hypot(dx, dy), 0.001);
    for (const offset of [28, 38, 50, 66, 84, 104]) {
      const candidate = [apex[0] + dx / magnitude * offset, apex[1] + dy / magnitude * offset] as const;
      if (labelIsSafe(candidate)) return candidate;
    }
    return null;
  };
  const visibleCornerLabels = corners.flatMap((corner) => {
    if (corner.apexDistanceM < (samples[0]?.distanceM ?? 0) || corner.apexDistanceM > (samples.at(-1)?.distanceM ?? 0)) return [];
    const sample = samples.reduce((closest, candidate) => Math.abs(candidate.distanceM - corner.apexDistanceM) < Math.abs(closest.distanceM - corner.apexDistanceM) ? candidate : closest, samples[0]);
    if (!sample || sample.referencePositionXM == null || sample.referencePositionZM == null) return [];
    const point = cornerLabelPoint(sample.referencePositionXM, sample.referencePositionZM);
    return point ? [{ corner, point }] : [];
  });
  const road = trackMap ? geometryPath([...trackMap.leftBoundary, ...[...trackMap.rightBoundary].reverse()], true) : "";
  const cursor = cursorIndex == null ? null : samples[cursorIndex] ?? null;
  const comparisonCursorSample = comparison && cursor?.referenceElapsedSeconds != null
    ? closestSampleAtElapsedTime(samples, "comparisonElapsedSeconds", cursor.referenceElapsedSeconds) ?? cursor
    : cursor;
  const referenceCursor = cursor?.referencePositionXM != null && cursor.referencePositionZM != null ? project(cursor.referencePositionXM, cursor.referencePositionZM) : null;
  const comparisonCursor = comparisonCursorSample?.comparisonPositionXM != null && comparisonCursorSample.comparisonPositionZM != null ? project(comparisonCursorSample.comparisonPositionXM, comparisonCursorSample.comparisonPositionZM) : null;
  const cursorTargets = [referenceCursor, comparisonCursor].filter((point): point is readonly [number, number] => point != null);
  const followedTarget = cursorTargets.length === 0 ? null : cursorTargets.reduce((total, point) => [total[0] + point[0], total[1] + point[1]] as const, [0, 0] as const).map((value) => value / cursorTargets.length);
  const followPan = zoom > 1 && followedTarget ? { x: zoom * (width / 2 - followedTarget[0]), y: zoom * (height / 2 - followedTarget[1]) } : { x: 0, y: 0 };
  const renderedPan = { x: pan.x + followPan.x, y: pan.y + followPan.y };
  const mapPointUnderViewportPoint = (anchor: readonly [number, number]) => [
    width / 2 + (anchor[0] - renderedPan.x - width / 2) / zoom,
    height / 2 + (anchor[1] - renderedPan.y - height / 2) / zoom,
  ] as const;
  rangeDistanceAt.current = (anchor) => {
    const mapPoint = mapPointUnderViewportPoint(anchor);
    return samples.reduce<{ distanceM: number; squaredDistance: number } | null>((nearest, sample) => {
      const positions = [
        sample.referencePositionXM != null && sample.referencePositionZM != null ? project(sample.referencePositionXM, sample.referencePositionZM) : null,
        comparison && sample.comparisonPositionXM != null && sample.comparisonPositionZM != null ? project(sample.comparisonPositionXM, sample.comparisonPositionZM) : null,
      ].filter((point): point is readonly [number, number] => point != null);
      const squaredDistance = Math.min(...positions.map((point) => (point[0] - mapPoint[0]) ** 2 + (point[1] - mapPoint[1]) ** 2));
      return positions.length > 0 && (nearest == null || squaredDistance < nearest.squaredDistance) ? { distanceM: sample.distanceM, squaredDistance } : nearest;
    }, null)?.distanceM ?? null;
  };
  spatialZoomAt.current = (direction, anchor) => {
    const nextZoom = Math.min(8, Math.max(1, zoom * (direction === "in" ? 1.15 : 0.87)));
    if (Math.abs(nextZoom - zoom) < 0.000_001) return;
    const ratio = nextZoom / zoom;
    const nextRenderedPan = {
      x: ratio * renderedPan.x + (1 - ratio) * (anchor[0] - width / 2),
      y: ratio * renderedPan.y + (1 - ratio) * (anchor[1] - height / 2),
    };
    const nextFollowPan = nextZoom > 1 && followedTarget ? { x: nextZoom * (width / 2 - followedTarget[0]), y: nextZoom * (height / 2 - followedTarget[1]) } : { x: 0, y: 0 };
    setZoom(nextZoom);
    setPan({ x: nextRenderedPan.x - nextFollowPan.x, y: nextRenderedPan.y - nextFollowPan.y });
  };
  const start = samples.find((sample) => sample.referencePositionXM != null && sample.referencePositionZM != null);
  const startPoint = start?.referencePositionXM != null && start.referencePositionZM != null ? project(start.referencePositionXM, start.referencePositionZM) : null;
  const resetView = () => { setZoom(1); setPan({ x: 0, y: 0 }); };
  const referenceColour = !comparison || comparisonIsFaster ? "var(--color-trace-accent)" : channelColours.faster;
  const comparisonColour = comparisonIsFaster ? channelColours.faster : "var(--color-trace-accent)";
  const mapRangeLabel = focusSelection ? rangeLabel ?? "SELECTED RANGE" : null;
  const adjustMapZoom = (direction: "in" | "out") => {
    if (onRangeZoom && rangeZoomLinked) {
      const anchor = cursor?.distanceM ?? rangeDistanceAt.current([width / 2, height / 2]) ?? ((samples[0]?.distanceM ?? 0) + (samples.at(-1)?.distanceM ?? 0)) / 2;
      onRangeZoom(anchor, direction);
    } else {
      spatialZoomAt.current(direction, [width / 2, height / 2]);
    }
  };
  const mapControls = <div className="flex items-center gap-1">{onRangeZoom && onRangeZoomLinked && <button type="button" onClick={() => onRangeZoomLinked(!rangeZoomLinked)} className={`inline-flex h-8 items-center justify-center border px-2 font-mono text-[9px] font-bold leading-none ${rangeZoomLinked ? "border-trace-accent/50 bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`} aria-label={rangeZoomLinked ? "Unlink map zoom from telemetry graphs" : "Link map zoom to telemetry graphs"}>{rangeZoomLinked ? "LINKED" : "MAP"}</button>}<button type="button" onClick={() => adjustMapZoom("in")} className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-base text-trace-muted hover:text-trace-text" aria-label={rangeZoomLinked ? "Zoom all telemetry in" : "Zoom map in"}>+</button><button type="button" onClick={() => adjustMapZoom("out")} className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-base text-trace-muted hover:text-trace-text" aria-label={rangeZoomLinked ? "Zoom all telemetry out" : "Zoom map out"}>−</button><button type="button" onClick={resetView} className="inline-flex h-8 items-center justify-center border border-trace-divider bg-trace-deep px-2 font-mono text-[10px] leading-none text-trace-muted hover:text-trace-text" aria-label="Reset map pan and spatial zoom">RESET</button>{onDismiss && <button type="button" onClick={onDismiss} className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-lg leading-none text-trace-muted hover:border-trace-accent/50 hover:text-trace-text" aria-label="Dismiss floating track map">×</button>}</div>;
  return (
    <div ref={mapViewport} className="overscroll-contain border border-trace-divider bg-trace-surface">
      {onDismiss
        ? <div className="flex h-10 items-center justify-end border-b border-trace-divider px-2">{mapControls}</div>
        : <div className="flex h-12 items-center justify-between border-b border-trace-divider px-4"><div>{mapRangeLabel && <span className="font-mono text-[10px] font-black text-trace-accent">{mapRangeLabel}</span>}</div><div className="ml-auto mr-4 flex items-center gap-4 font-mono text-[10px] font-bold text-trace-muted">{comparison && <><span className="flex items-center gap-2"><span className={`block w-6 border-t-2 ${comparisonIsFaster ? "" : "border-dashed"}`} style={{ borderColor: referenceColour }} />REFERENCE</span><span className="flex items-center gap-2"><span className={`block w-6 border-t-2 ${comparisonIsFaster ? "border-dashed" : ""}`} style={{ borderColor: comparisonColour }} />ANALYSED LAP</span></>}<span className="flex items-center gap-2"><span className="block h-1.5 w-6" style={{ backgroundColor: channelColours.mapBrake }} />BRAKE</span></div>{mapControls}</div>}
      <svg className="block w-full cursor-grab touch-none active:cursor-grabbing" style={{ height: displayHeight }} viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Recorded path around the track" onPointerDown={(event) => { event.currentTarget.setPointerCapture(event.pointerId); drag.current = { x: event.clientX, y: event.clientY, panX: pan.x, panY: pan.y }; }} onPointerMove={(event) => { if (!drag.current) return; const bounds = event.currentTarget.getBoundingClientRect(); setPan({ x: drag.current.panX + (event.clientX - drag.current.x) * width / bounds.width, y: drag.current.panY + (event.clientY - drag.current.y) * height / bounds.height }); }} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}>
        <g transform={`translate(${renderedPan.x} ${renderedPan.y}) translate(${width / 2} ${height / 2}) scale(${zoom}) translate(${-width / 2} ${-height / 2})`}>
          {trackMap && <path d={road} fill="var(--color-trace-deep)" stroke="none" />}
          {trackMap && <path d={geometryPath(trackMap.leftBoundary)} fill="none" stroke="var(--color-trace-soft)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />}
          {trackMap && <path d={geometryPath(trackMap.rightBoundary)} fill="none" stroke="var(--color-trace-soft)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />}
          {trackMap && <path d={geometryPath(trackMap.centreLine)} fill="none" stroke="var(--color-trace-divider)" strokeWidth="1" strokeDasharray="5 8" vectorEffect="non-scaling-stroke" />}
          <path d={path("referencePositionXM", "referencePositionZM")} fill="none" stroke={referenceColour} strokeWidth="3" strokeDasharray={comparison && !comparisonIsFaster ? "9 7" : undefined} strokeLinecap="round" vectorEffect="non-scaling-stroke" />
          {comparison && <path d={path("comparisonPositionXM", "comparisonPositionZM")} fill="none" stroke={comparisonColour} strokeWidth="3" strokeDasharray={comparisonIsFaster ? "9 7" : undefined} strokeLinecap="round" vectorEffect="non-scaling-stroke" />}
          {brakeSegments("reference-brake", "referencePositionXM", "referencePositionZM", "referenceBrakePercent", 6)}
          {comparison && brakeSegments("comparison-brake", "comparisonPositionXM", "comparisonPositionZM", "comparisonBrakePercent", 3.5)}
          {visibleCornerLabels.map(({ corner, point }) => <g transform={`translate(${point[0]} ${point[1]})`} pointerEvents="none" key={corner.index}><circle r="15" fill="var(--color-trace-black)" stroke={corner.index === selectedCornerIndex ? "var(--color-trace-text)" : "var(--color-trace-soft)"} strokeWidth={corner.index === selectedCornerIndex ? 2.25 : 1.75} vectorEffect="non-scaling-stroke" /><text y="4" textAnchor="middle" fill="var(--color-trace-text)" fontFamily="monospace" fontSize="11" fontWeight="900">{corner.label}</text></g>)}
          {startPoint && <g transform={`translate(${startPoint[0]} ${startPoint[1]})`}><line x1="-7" y1="-7" x2="7" y2="7" stroke="#fff" strokeWidth="2" vectorEffect="non-scaling-stroke" /><line x1="7" y1="-7" x2="-7" y2="7" stroke="#fff" strokeWidth="2" vectorEffect="non-scaling-stroke" /></g>}
          {referenceCursor && <circle cx={referenceCursor[0]} cy={referenceCursor[1]} r="6" fill={referenceColour} stroke="#101010" strokeWidth="2" vectorEffect="non-scaling-stroke" />}
          {comparisonCursor && <circle cx={comparisonCursor[0]} cy={comparisonCursor[1]} r="4.5" fill={comparisonColour} stroke="#101010" strokeWidth="2" vectorEffect="non-scaling-stroke" />}
        </g>
      </svg>
    </div>
  );
}

function formatGear(gear?: number | null) {
  if (gear == null) return "—";
  if (gear < 0) return "R";
  if (gear === 0) return "N";
  return String(gear);
}

function deltaRange(samples: LapComparisonSample[]): [number, number] {
  const maximum = Math.max(0.1, ...samples.flatMap((sample) => sample.deltaSeconds == null ? [] : [Math.abs(sample.deltaSeconds)]));
  return [-maximum, maximum];
}

function steeringInputRange(samples: LapComparisonSample[]): [number, number] {
  const maximum = Math.max(10, ...samples.flatMap((sample) => [sample.referenceSteeringPercent, sample.comparisonSteeringPercent].flatMap((value) => value == null || !Number.isFinite(value) ? [] : [Math.abs(value)])));
  const bound = Math.min(100, Math.ceil(maximum / 10) * 10);
  return [-bound, bound];
}

function ComparisonChart({ label, unit, samples, series, cursorIndex, onCursor, onZoom, fixedRange, zeroLine = false, compact = false }: { label: string; unit: string; samples: LapComparisonSample[]; series: ComparisonChartSeries[]; cursorIndex: number | null; onCursor: (index: number | null) => void; onZoom?: (anchorM: number, direction: "in" | "out") => void; fixedRange?: [number, number]; zeroLine?: boolean; compact?: boolean }) {
  const width = 1_000;
  const height = compact ? 82 : 220;
  const plot = { left: 58, right: 18, top: compact ? 10 : 24, bottom: compact ? 20 : 30 };
  const values = series.flatMap((item) => samples.flatMap((sample) => {
    const value = item.value(sample);
    return value == null || !Number.isFinite(value) ? [] : [value];
  }));
  const automaticMin = values.length ? Math.min(...values) : 0;
  const automaticMax = values.length ? Math.max(...values) : 1;
  const padding = Math.max((automaticMax - automaticMin) * 0.08, 0.01);
  const [minimum, maximum] = fixedRange ?? [automaticMin - padding, automaticMax + padding];
  const range = Math.max(maximum - minimum, 0.000_001);
  const firstDistance = samples[0]?.distanceM ?? 0;
  const lastDistance = samples.at(-1)?.distanceM ?? firstDistance + 1;
  const distanceRange = Math.max(lastDistance - firstDistance, 1);
  const x = (distance: number) => plot.left + (distance - firstDistance) / distanceRange * (width - plot.left - plot.right);
  const y = (value: number) => plot.top + (maximum - value) / range * (height - plot.top - plot.bottom);
  const cursorSample = cursorIndex == null ? null : samples[cursorIndex] ?? null;
  const zoomAnchorM = cursorSample?.distanceM ?? firstDistance + distanceRange / 2;
  const cursorX = cursorSample ? x(cursorSample.distanceM) : null;
  const tooltipTransform = cursorX == null
    ? undefined
    : cursorX < 100
      ? "translateX(5px)"
      : "translateX(calc(-100% - 5px))";
  const tooltipValues = cursorSample ? series.flatMap((item) => {
    const value = item.value(cursorSample);
    return value == null || !Number.isFinite(value) ? [] : [{ item, value, chartY: y(value) }];
  }) : [];
  const chartPixelHeight = compact ? 82 : 224;
  const headerPixelHeight = compact ? 36 : 48;
  const tooltipTops = tooltipValues.map(({ chartY }) => headerPixelHeight + chartY / height * chartPixelHeight);
  if (tooltipTops.length === 2 && Math.abs(tooltipTops[0] - tooltipTops[1]) < 32) {
    const midpoint = (tooltipTops[0] + tooltipTops[1]) / 2;
    const firstIsHigher = tooltipTops[0] <= tooltipTops[1];
    tooltipTops[0] = midpoint + (firstIsHigher ? -16 : 16);
    tooltipTops[1] = midpoint + (firstIsHigher ? 16 : -16);
  }
  return (
    <div className="relative overflow-hidden border border-trace-divider bg-trace-surface">
      <div className={`flex items-center justify-between gap-3 overflow-hidden border-b border-trace-divider px-4 ${compact ? "h-9" : "h-12"}`}>
        <span className="font-mono text-[12px] font-bold tracking-[.1em] text-trace-soft">{label}</span>
        <div className="flex min-w-0 items-center gap-4 overflow-hidden font-mono text-[11px] font-bold">{series.map((item) => <span className="flex shrink-0 items-center gap-1.5" key={item.label}><span className="size-1.5 rounded-full" style={{ backgroundColor: item.colour }} /><span style={{ color: item.colour }}>{item.label}</span></span>)}{onZoom && <span className="ml-auto flex shrink-0 items-center"><button type="button" onClick={() => onZoom(zoomAnchorM, "out")} className="grid size-7 place-items-center border border-trace-divider bg-trace-deep text-sm text-trace-muted hover:border-trace-soft hover:text-trace-text" aria-label={`Zoom all telemetry out from ${Math.round(zoomAnchorM)} metres`}>−</button><button type="button" onClick={() => onZoom(zoomAnchorM, "in")} className="grid size-7 place-items-center border border-l-0 border-trace-divider bg-trace-deep text-sm text-trace-muted hover:border-trace-soft hover:text-trace-text" aria-label={`Zoom all telemetry in around ${Math.round(zoomAnchorM)} metres`}>+</button></span>}</div>
      </div>
      <svg className={`block w-full touch-none ${compact ? "h-[82px]" : "h-56"}`} viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label={`${label} telemetry by lap distance`} onMouseMove={(event) => {
        const bounds = event.currentTarget.getBoundingClientRect();
        const pointerX = (event.clientX - bounds.left) / bounds.width * width;
        const ratio = Math.min(1, Math.max(0, (pointerX - plot.left) / (width - plot.left - plot.right)));
        onCursor(Math.round(ratio * (samples.length - 1)));
      }}>
        {[0, 0.5, 1].map((ratio) => <line x1={plot.left} x2={width - plot.right} y1={plot.top + ratio * (height - plot.top - plot.bottom)} y2={plot.top + ratio * (height - plot.top - plot.bottom)} className="stroke-trace-divider" strokeWidth="1" vectorEffect="non-scaling-stroke" key={ratio} />)}
        {zeroLine && minimum < 0 && maximum > 0 && <line x1={plot.left} x2={width - plot.right} y1={y(0)} y2={y(0)} className="stroke-trace-dim" strokeDasharray="4 4" vectorEffect="non-scaling-stroke" />}
        {series.map((item) => <path d={comparisonPath(samples, item.value, x, y)} fill="none" stroke={item.colour} strokeWidth="2" vectorEffect="non-scaling-stroke" key={item.label} />)}
        {cursorSample && <line x1={x(cursorSample.distanceM)} x2={x(cursorSample.distanceM)} y1={plot.top} y2={height - plot.bottom} className="stroke-trace-text" strokeWidth="1" vectorEffect="non-scaling-stroke" />}
      </svg>
      <span className="pointer-events-none absolute left-2 font-mono text-[12px] leading-none text-trace-dim" style={{ top: `${headerPixelHeight + plot.top / height * chartPixelHeight}px` }} aria-hidden="true">{formatChartValue(maximum, unit)}</span>
      <span className="pointer-events-none absolute left-2 -translate-y-full font-mono text-[12px] leading-none text-trace-dim" style={{ top: `${headerPixelHeight + (height - plot.bottom) / height * chartPixelHeight}px` }} aria-hidden="true">{formatChartValue(minimum, unit)}</span>
      <span className="pointer-events-none absolute -translate-y-full font-mono text-[12px] leading-none text-trace-dim" style={{ left: `${plot.left / width * 100}%`, top: `${headerPixelHeight + (height - 8) / height * chartPixelHeight}px` }} aria-hidden="true">{Math.round(firstDistance)} M</span>
      <span className="pointer-events-none absolute -translate-x-full -translate-y-full font-mono text-[12px] leading-none text-trace-dim" style={{ left: `${(width - plot.right) / width * 100}%`, top: `${headerPixelHeight + (height - 8) / height * chartPixelHeight}px` }} aria-hidden="true">{Math.round(lastDistance)} M</span>
      {cursorX != null && tooltipValues.map(({ item, value }, index) => (
        <span className="pointer-events-none absolute z-20 whitespace-nowrap rounded-sm px-2 py-1 font-mono text-[11px] font-black tabular-nums shadow-[0_5px_14px_rgba(0,0,0,.5)]" style={{ left: `${cursorX / width * 100}%`, top: `${tooltipTops[index]}px`, transform: `${tooltipTransform} translateY(-50%)`, backgroundColor: item.colour, color: chartTooltipTextColour(item.colour) }} role="status" aria-label={`${item.label}: ${formatChartValue(value, unit)}${unit ? ` ${unit}` : ""}`} key={item.label}>{formatChartValue(value, unit)}{unit ? ` ${unit}` : ""}</span>
      ))}
    </div>
  );
}

function comparisonPath(samples: LapComparisonSample[], value: (sample: LapComparisonSample) => number | null | undefined, x: (distance: number) => number, y: (value: number) => number) {
  let drawing = false;
  return samples.reduce((path, sample) => {
    const current = value(sample);
    if (current == null || !Number.isFinite(current)) {
      drawing = false;
      return path;
    }
    const command = drawing ? "L" : "M";
    drawing = true;
    return `${path}${command}${x(sample.distanceM).toFixed(2)},${y(current).toFixed(2)}`;
  }, "");
}

function formatChartValue(value: number, unit: string) {
  if (unit === "%" || unit === "" || unit === "rpm" || unit === "km/h" || unit === "°") return String(Math.round(value));
  if (unit === "s") return value.toFixed(3);
  const magnitude = Math.abs(value);
  return magnitude >= 100 ? value.toFixed(0) : magnitude >= 10 ? value.toFixed(1) : value.toFixed(3);
}

function chartTooltipTextColour(colour: string) {
  if (!colour.startsWith("#") || colour.length !== 7) return "#fff";
  const red = Number.parseInt(colour.slice(1, 3), 16);
  const green = Number.parseInt(colour.slice(3, 5), 16);
  const blue = Number.parseInt(colour.slice(5, 7), 16);
  return red * 0.299 + green * 0.587 + blue * 0.114 > 150 ? "#090b0d" : "#fff";
}

function formatDelta(value: number) {
  return `${value >= 0 ? "+" : "−"}${Math.abs(value).toFixed(3)} S`;
}

function Setups() {
  return (
    <>
      <PageIntro index="04" eyebrow="CAR SETUPS" title="CONNECT CHANGES TO LAP PERFORMANCE" description="Store setup snapshots beside sessions so a faster lap can be traced back to the car configuration that produced it." />
      <FeaturePreview label="PLANNED / PHASE 5" title="Setup workspace">
        <div className="grid gap-px bg-trace-divider md:grid-cols-3">
          <PreviewStep number="01" title="Save a snapshot" detail="Capture or import the setup used for a session." />
          <PreviewStep number="02" title="See what changed" detail="Compare values without hunting through setup screens." />
          <PreviewStep number="03" title="Link the result" detail="Associate a setup with laps, notes, and conditions." />
        </div>
      </FeaturePreview>
      <AvailabilityNote>Setup capture and imports are not implemented yet. This page describes the intended workflow without pretending the controls are active.</AvailabilityNote>
    </>
  );
}

function normalizeServiceEndpoint(value: string) {
  try {
    const endpoint = new URL(value.trim());
    if ((endpoint.protocol !== "https:" && endpoint.protocol !== "http:") || !endpoint.hostname) return null;
    return endpoint.toString().replace(/\/$/, "");
  } catch {
    return null;
  }
}

function Settings() {
  const showToast = useToast();
  const [directories, setDirectories] = useState<GameInstallDirectory[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<string | null>(null);
  const [profileName, setProfileName] = useState("");
  const [savedProfileName, setSavedProfileName] = useState("");
  const [savingProfile, setSavingProfile] = useState(false);
  const [apiEndpoint, setApiEndpoint] = useState(DEFAULT_API_SERVICE_ENDPOINT);
  const [liveEndpoint, setLiveEndpoint] = useState(DEFAULT_LIVE_SERVICE_ENDPOINT);
  const [savedApiEndpoint, setSavedApiEndpoint] = useState(DEFAULT_API_SERVICE_ENDPOINT);
  const [savedLiveEndpoint, setSavedLiveEndpoint] = useState(DEFAULT_LIVE_SERVICE_ENDPOINT);
  const [savingLiveSettings, setSavingLiveSettings] = useState(false);

  useEffect(() => {
    let active = true;
    void Promise.all([telemetryDataSource.getGameInstallDirectories(), telemetryDataSource.getDriverProfile(), telemetryDataSource.getLiveSettings()]).then(([values, profile, liveSettings]) => {
      if (!active) return;
      setDirectories(values);
      setDrafts(Object.fromEntries(values.map((value) => [value.simulatorId, value.path ?? ""])));
      setProfileName(profile.name ?? "");
      setSavedProfileName(profile.name ?? "");
      setApiEndpoint(liveSettings.apiEndpoint);
      setLiveEndpoint(liveSettings.liveEndpoint);
      setSavedApiEndpoint(liveSettings.apiEndpoint);
      setSavedLiveEndpoint(liveSettings.liveEndpoint);
      setLoading(false);
    }).catch((error) => {
      if (!active) return;
      setLoading(false);
      showToast({ kind: "error", title: "Settings unavailable", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
    });
    return () => { active = false; };
  }, [showToast]);

  async function saveProfile() {
    setSavingProfile(true);
    try {
      const profile = await telemetryDataSource.setDriverProfile(profileName.trim() || null);
      setProfileName(profile.name ?? "");
      setSavedProfileName(profile.name ?? "");
      showToast({ kind: "success", title: profile.name ? "Driver profile saved" : "Driver profile cleared", message: profile.name ? `${profile.name} will identify your new captures and shared TRACE sessions.` : "Future captures will not receive a driver name.", timeoutMs: 5_000 });
    } catch (error) {
      showToast({ kind: "error", title: "Could not save driver profile", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
    } finally {
      setSavingProfile(false);
    }
  }

  async function saveDirectory(simulatorId: string, customPath: string | null) {
    setSaving(simulatorId);
    try {
      const updated = await telemetryDataSource.setGameInstallDirectory(simulatorId, customPath);
      setDirectories((current) => current.map((value) => value.simulatorId === simulatorId ? updated : value));
      setDrafts((current) => ({ ...current, [simulatorId]: updated.path ?? "" }));
      showToast({ kind: "success", title: customPath ? "Game folder saved" : "Automatic detection restored", message: updated.path ?? `${updated.simulatorName} was not detected.`, timeoutMs: 4_500 });
    } catch (error) {
      showToast({ kind: "error", title: "Could not save game folder", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
    } finally {
      setSaving(null);
    }
  }

  async function saveLiveSettings() {
    const normalizedApiEndpoint = normalizeServiceEndpoint(apiEndpoint);
    const normalizedLiveEndpoint = normalizeServiceEndpoint(liveEndpoint);
    if (!normalizedApiEndpoint || !normalizedLiveEndpoint) {
      showToast({ kind: "error", title: "Invalid service endpoint", message: "Enter complete HTTP or HTTPS URLs for both the API and Live services.", timeoutMs: 7_000 });
      return;
    }
    setSavingLiveSettings(true);
    try {
      const settings = await telemetryDataSource.setLiveSettings(normalizedApiEndpoint, normalizedLiveEndpoint);
      setApiEndpoint(settings.apiEndpoint);
      setLiveEndpoint(settings.liveEndpoint);
      setSavedApiEndpoint(settings.apiEndpoint);
      setSavedLiveEndpoint(settings.liveEndpoint);
      showToast({ kind: "success", title: "Go Live services saved", message: "TRACE will use the configured API and Live services when remote spectating becomes available.", timeoutMs: 5_000 });
    } catch (error) {
      showToast({ kind: "error", title: "Could not save Go Live services", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
    } finally {
      setSavingLiveSettings(false);
    }
  }

  async function chooseDirectory(directory: GameInstallDirectory) {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: drafts[directory.simulatorId]?.trim() || directory.path || undefined,
        title: `Choose ${directory.simulatorName} folder`,
      });
      if (typeof selected === "string") {
        setDrafts((current) => ({ ...current, [directory.simulatorId]: selected }));
        await saveDirectory(directory.simulatorId, selected);
      }
    } catch (error) {
      showToast({ kind: "error", title: "Folder picker unavailable", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
    }
  }

  return (
    <>
      <PageIntro index="05" eyebrow="PREFERENCES" title="SETTINGS" description="Control how TRACE connects to your simulators and works with their data. Recording, storage, analysis, and appearance preferences will also live here as those features become configurable." />
      <form className="mt-7 border border-trace-divider bg-trace-surface" onSubmit={(event) => { event.preventDefault(); void saveProfile(); }}>
        <div className="border-b border-trace-divider px-5 py-4">
          <h2 className="text-[14px] font-black tracking-[.04em]">DRIVER PROFILE</h2>
          <p className="mt-1 max-w-3xl text-[12px] leading-5 text-trace-dim">Use a nickname or full name that other drivers will recognize. TRACE attaches it to new captures and includes it in shared <span className="font-mono text-trace-soft">.trace</span> packages; exports of older self-owned sessions use it when no session-specific driver is set.</p>
        </div>
        <label className="block p-5 text-[12px] font-bold tracking-[.08em] text-trace-dim">
          DISPLAY NAME
          <div className="mt-1.5 flex max-w-2xl">
            <input value={profileName} maxLength={80} onChange={(event) => setProfileName(event.target.value)} placeholder="Nickname or full name" className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 text-[13px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
            <button type="submit" disabled={savingProfile || profileName.trim() === savedProfileName} className="w-28 border border-l-0 border-trace-accent bg-trace-accent-wash text-[12px] font-bold text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim">{savingProfile ? "SAVING…" : "SAVE"}</button>
          </div>
        </label>
      </form>
      <form className="mt-7 border border-trace-divider bg-trace-surface" onSubmit={(event) => { event.preventDefault(); void saveLiveSettings(); }}>
        <div className="border-b border-trace-divider px-5 py-4">
          <h2 className="text-[14px] font-black tracking-[.04em]">GO LIVE</h2>
          <p className="mt-1 max-w-3xl text-[12px] leading-5 text-trace-dim">Choose the services TRACE will use to create and publish spectatable sessions. Keep the hosted defaults, or point both services at your own compatible deployment.</p>
        </div>
        <div className="grid grid-cols-2 divide-x divide-trace-divider">
          <label className="block p-5 text-[12px] font-bold tracking-[.08em] text-trace-dim">
            API ENDPOINT
            <span className="mt-1 block min-h-10 max-w-xl font-normal leading-5 normal-case tracking-normal text-trace-dim">Creates sessions and handles control-plane requests.</span>
            <div className="mt-2 flex">
              <input type="url" value={apiEndpoint} onChange={(event) => setApiEndpoint(event.target.value)} placeholder={DEFAULT_API_SERVICE_ENDPOINT} spellCheck={false} className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
              <button type="button" disabled={savingLiveSettings || apiEndpoint === DEFAULT_API_SERVICE_ENDPOINT} onClick={() => setApiEndpoint(DEFAULT_API_SERVICE_ENDPOINT)} className="w-24 shrink-0 border border-l-0 border-trace-divider bg-trace-surface text-[10px] font-bold leading-none text-trace-soft hover:bg-trace-raised hover:text-trace-text disabled:bg-trace-deep disabled:text-trace-dim">DEFAULT</button>
            </div>
            <span className="mt-2 block truncate font-mono text-[10px] font-normal normal-case tracking-normal text-trace-soft">{DEFAULT_API_SERVICE_ENDPOINT}</span>
          </label>
          <label className="block p-5 text-[12px] font-bold tracking-[.08em] text-trace-dim">
            LIVE ENDPOINT
            <span className="mt-1 block min-h-10 max-w-xl font-normal leading-5 normal-case tracking-normal text-trace-dim">Carries realtime telemetry to spectators; secure WebSockets will be derived from this URL.</span>
            <div className="mt-2 flex">
              <input type="url" value={liveEndpoint} onChange={(event) => setLiveEndpoint(event.target.value)} placeholder={DEFAULT_LIVE_SERVICE_ENDPOINT} spellCheck={false} className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
              <button type="button" disabled={savingLiveSettings || liveEndpoint === DEFAULT_LIVE_SERVICE_ENDPOINT} onClick={() => setLiveEndpoint(DEFAULT_LIVE_SERVICE_ENDPOINT)} className="w-24 shrink-0 border border-l-0 border-trace-divider bg-trace-surface text-[10px] font-bold leading-none text-trace-soft hover:bg-trace-raised hover:text-trace-text disabled:bg-trace-deep disabled:text-trace-dim">DEFAULT</button>
            </div>
            <span className="mt-2 block truncate font-mono text-[10px] font-normal normal-case tracking-normal text-trace-soft">{DEFAULT_LIVE_SERVICE_ENDPOINT}</span>
          </label>
        </div>
        <div className="flex min-h-14 items-center justify-between gap-5 border-t border-trace-divider px-5 py-2">
          <span className="text-[11px] leading-5 text-trace-dim">Only complete HTTP and HTTPS base URLs are accepted.</span>
          <button type="submit" disabled={savingLiveSettings || !apiEndpoint.trim() || !liveEndpoint.trim() || (apiEndpoint.trim() === savedApiEndpoint && liveEndpoint.trim() === savedLiveEndpoint)} className="h-10 w-28 shrink-0 border border-trace-accent bg-trace-accent-wash text-[12px] font-bold leading-none text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim">{savingLiveSettings ? "SAVING…" : "SAVE"}</button>
        </div>
      </form>
      <div className="mt-7 border border-trace-divider bg-trace-surface">
        <div className="border-b border-trace-divider px-5 py-4">
          <h2 className="text-[14px] font-black tracking-[.04em]">GAME FOLDERS</h2>
          <p className="mt-1 max-w-3xl text-[12px] leading-5 text-trace-dim">Game roots give each simulator adapter access to the files and metadata needed for content identification, replay and setup workflows, and future integrations. Choose the main game folder—not one of its subfolders.</p>
        </div>
        {loading ? (
          <div className="p-6 font-mono text-[12px] text-trace-dim">CHECKING INSTALLED GAMES…</div>
        ) : directories.length === 0 ? (
          <div className="p-6 text-[12px] text-trace-dim">No configurable game adapters are installed.</div>
        ) : directories.map((directory) => {
          const draft = drafts[directory.simulatorId] ?? "";
          const unchanged = draft.trim() === (directory.path ?? "");
          return (
            <form className="p-5" key={directory.simulatorId} onSubmit={(event) => { event.preventDefault(); void saveDirectory(directory.simulatorId, draft.trim() || null); }}>
              <div className="flex items-center justify-between gap-4">
                <div>
                  <strong className="text-[14px] text-trace-text">{directory.simulatorName}</strong>
                  <span className={`ml-3 inline-flex border px-2 py-1 font-mono text-[12px] font-bold tracking-[.08em] ${directory.source === "missing" ? "border-trace-warning/50 text-trace-warning" : directory.source === "manual" ? "border-trace-soft/50 text-trace-soft" : "border-trace-accent-muted text-trace-accent"}`}>
                    {directory.source === "manual" ? "CUSTOM" : directory.source === "detected" ? "AUTO-DETECTED" : "NOT FOUND"}
                  </span>
                </div>
                {directory.source === "manual" && <button type="button" disabled={saving === directory.simulatorId} onClick={() => void saveDirectory(directory.simulatorId, null)} className="border-0 bg-transparent text-[12px] font-bold text-trace-muted hover:text-trace-text disabled:text-trace-dim">USE AUTO-DETECTION</button>}
              </div>
              <label className="mt-4 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
                INSTALL DIRECTORY
                <div className="mt-1.5 flex">
                  <input value={draft} onChange={(event) => setDrafts((current) => ({ ...current, [directory.simulatorId]: event.target.value }))} placeholder="C:\\Program Files (x86)\\Steam\\steamapps\\common\\assettocorsa" className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
                  <button type="button" disabled={saving === directory.simulatorId} onClick={() => void chooseDirectory(directory)} className="flex h-11 w-28 items-center justify-center gap-2 border border-l-0 border-trace-divider bg-trace-surface text-[12px] font-bold text-trace-soft hover:bg-trace-raised hover:text-trace-text disabled:text-trace-dim">
                    <svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true"><path d="M1.5 4.5h5l1.2 1.5h6.8v7.5h-13zM1.5 4.5V2.8h4.2l1.2 1.7" /></svg>
                    BROWSE
                  </button>
                  <button type="submit" disabled={saving === directory.simulatorId || unchanged || !draft.trim()} className="w-24 border border-l-0 border-trace-accent bg-trace-accent-wash text-[12px] font-bold text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim">
                    {saving === directory.simulatorId ? "SAVING…" : "SAVE"}
                  </button>
                </div>
              </label>
              <p className="mt-2 text-[12px] leading-5 text-trace-dim">{directory.path ? `Currently using ${directory.source === "manual" ? "your custom path" : "the detected Steam installation"}.` : "TRACE could not locate this game automatically. Paste its installation folder above."}</p>
            </form>
          );
        })}
      </div>
    </>
  );
}

function PageIntro({ index, eyebrow, title, description }: { index: string; eyebrow: string; title: string; description: string }) {
  return (
    <div className="max-w-3xl">
      <SectionHeading index={index}>{eyebrow}</SectionHeading>
      <h1 className="mt-3 text-2xl font-black tracking-[-.02em]">{title}</h1>
      <p className="mt-2 max-w-2xl text-[14px] leading-6 text-trace-muted">{description}</p>
    </div>
  );
}

function WorkflowStep({ number, title, children }: { number: string; title: string; children: ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="grid size-7 shrink-0 place-items-center border border-trace-accent-muted font-mono text-[12px] text-trace-accent">{number}</span>
      <div><strong className="block text-[13px] text-trace-text">{title}</strong><span className="mt-1 block text-[12px] leading-5 text-trace-faint">{children}</span></div>
    </li>
  );
}

function FeaturePreview({ label, title, children }: { label: string; title: string; children: ReactNode }) {
  return (
    <div className="mt-7 border border-trace-divider bg-trace-surface">
      <div className="flex items-center justify-between border-b border-trace-divider px-5 py-4">
        <h2 className="text-[14px] font-black tracking-[.04em]">{title}</h2>
        <span className="font-mono text-[12px] font-bold tracking-[.08em] text-trace-accent">{label}</span>
      </div>
      {children}
    </div>
  );
}

function PreviewStep({ number, title, detail }: { number: string; title: string; detail: string }) {
  return <div className="min-h-44 bg-trace-surface p-6"><span className="font-mono text-[12px] text-trace-accent">{number}</span><h3 className="mt-7 text-base font-black">{title}</h3><p className="mt-2 text-[13px] leading-5 text-trace-faint">{detail}</p></div>;
}

function AvailabilityNote({ children }: { children: ReactNode }) {
  return <p className="border border-t-0 border-trace-divider bg-trace-deep px-5 py-4 text-[12px] leading-5 text-trace-muted"><strong className="mr-2 text-trace-warning">NOT AVAILABLE YET</strong>{children}</p>;
}

function Sessions({ sessions, onOpen, onDeleted, onUpdated, onImported }: { sessions: RecordedSessionSummary[]; onOpen: (sessionId: string) => void; onDeleted: (sessionId: string) => void; onUpdated: (session: RecordedSessionSummary) => void; onImported: () => Promise<void> }) {
  const showToast = useToast();
  const [query, setQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [simulatorFilter, setSimulatorFilter] = useState("all");
  const [sortOrder, setSortOrder] = useState("newest");
  const [importing, setImporting] = useState(false);
  const visibleSessions = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return sessions
      .filter((session) => {
        const source = sessionSourceGroup(session);
        const matchesSource = sourceFilter === "all" || source === sourceFilter;
        const matchesSimulator = simulatorFilter === "all" || session.simulatorId === simulatorFilter;
        const ownershipLabel = session.ownership === "other" ? "other driver" : session.ownership === "mine" ? "my drive" : "not specified";
        const searchable = [session.title, session.driver, ownershipLabel, session.simulatorName, session.track, session.car, session.sessionType, session.source, ...session.tags]
          .join(" ")
          .toLocaleLowerCase();
        return matchesSource && matchesSimulator && (!normalizedQuery || searchable.includes(normalizedQuery));
      })
      .slice()
      .sort((left, right) => {
        const difference = new Date(right.startedAt).getTime() - new Date(left.startedAt).getTime();
        return sortOrder === "newest" ? difference : -difference;
      });
  }, [query, sessions, simulatorFilter, sortOrder, sourceFilter]);
  const simulators = useMemo(() => Array.from(new Map(sessions.map((session) => [session.simulatorId, session.simulatorName])).entries()), [sessions]);

  async function deleteRecordedSession(session: RecordedSessionSummary) {
    try {
      const result = await telemetryDataSource.deleteSession(session.id);
      onDeleted(result.sessionId);
      showToast(result.cleanupWarning
        ? { kind: "error", title: "Deleted with cleanup warning", message: result.cleanupWarning, timeoutMs: 9_000 }
        : { kind: "success", title: "Session deleted", message: `${session.track} was removed from your session library.`, timeoutMs: 4_500 });
      return true;
    } catch (error) {
      showToast({ kind: "error", title: "Could not delete session", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
      return false;
    }
  }

  async function updateRecordedSession(session: RecordedSessionSummary, title: string | null, driver: string | null, ownership: RecordedSessionSummary["ownership"], tags: string[]) {
    try {
      await telemetryDataSource.updateSessionDetails(session.id, title, driver, ownership, tags);
      onUpdated({ ...session, title, driver, ownership, tags });
      showToast({ kind: "success", title: "Session updated", message: "Name, driver attribution, ownership, and tags were saved.", timeoutMs: 3_500 });
      return true;
    } catch (error) {
      showToast({ kind: "error", title: "Could not update session", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
      return false;
    }
  }

  async function importTraceSession() {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "Import a TRACE session",
      filters: [{ name: "TRACE session", extensions: ["trace"] }],
    });
    if (typeof selected !== "string") return;
    setImporting(true);
    try {
      const result = await telemetryDataSource.importSession(selected);
      await onImported();
      showToast({ kind: "success", title: "Session imported", message: `${result.lapCount} laps and ${result.sampleCount.toLocaleString()} telemetry samples are ready to review.`, timeoutMs: 6_000 });
    } catch (error) {
      showToast({ kind: "error", title: "Import failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
    } finally {
      setImporting(false);
    }
  }

  return (
    <>
      <div className="flex items-end justify-between gap-6">
        <div>
          <SectionHeading index="02">SESSION LIBRARY</SectionHeading>
          <h1 className="mt-3 text-2xl font-black tracking-[-.02em]">SESSIONS</h1>
          <p className="mt-2 text-[13px] text-trace-muted">Browse drives and replays, then open one to review every lap and its telemetry.</p>
        </div>
        <div className="flex items-center gap-4">
          <span className="font-mono text-[12px] text-trace-faint">{visibleSessions.length} shown · {sessions.length} total</span>
          <button type="button" disabled={importing} onClick={() => void importTraceSession()} className="h-10 border border-trace-accent/45 bg-trace-accent-wash px-4 font-mono text-[11px] font-bold tracking-[.08em] text-trace-accent hover:border-trace-accent hover:text-white disabled:border-trace-divider disabled:text-trace-dim">{importing ? "IMPORTING…" : "IMPORT .TRACE"}</button>
        </div>
      </div>

      <div className="mt-6 flex flex-wrap border border-trace-divider bg-trace-surface">
        <label className="flex min-w-[280px] flex-1 items-center gap-3 border-r border-trace-divider px-4 focus-within:bg-trace-raised">
          <svg className="size-3.5 shrink-0 stroke-trace-dim" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" />
            <path d="m10.5 10.5 3 3" />
          </svg>
          <span className="sr-only">Search sessions</span>
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search track, car, or session…"
            className="h-12 min-w-0 flex-1 border-0 bg-transparent text-[12px] text-trace-text outline-none placeholder:text-trace-dim"
          />
        </label>
        <select
          aria-label="Filter sessions by source"
          value={sourceFilter}
          onChange={(event) => setSourceFilter(event.target.value)}
          className="trace-select h-12 border-0 border-r border-trace-divider bg-trace-surface pl-4 font-mono text-[12px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
        >
          <option value="all">All sources</option>
          <option value="replay">Replays</option>
          <option value="native">Drives</option>
          <option value="imported">Imports</option>
        </select>
        {simulators.length > 1 && (
          <select
            aria-label="Filter sessions by simulator"
            value={simulatorFilter}
            onChange={(event) => setSimulatorFilter(event.target.value)}
            className="trace-select h-12 border-0 border-r border-trace-divider bg-trace-surface pl-4 font-mono text-[12px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
          >
            <option value="all">All simulators</option>
            {simulators.map(([id, name]) => <option value={id} key={id}>{name}</option>)}
          </select>
        )}
        <select
          aria-label="Sort sessions"
          value={sortOrder}
          onChange={(event) => setSortOrder(event.target.value)}
          className="trace-select h-12 border-0 bg-trace-surface pl-4 font-mono text-[12px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
        >
          <option value="newest">Newest first</option>
          <option value="oldest">Oldest first</option>
        </select>
      </div>

      <div className="mt-3 border border-trace-divider bg-trace-surface">
        {sessions.length === 0 ? (
          <EmptySessions title="No sessions yet">Select an installed simulator, then start a drive or play a replay. TRACE will save it here automatically.</EmptySessions>
        ) : visibleSessions.length === 0 ? (
          <EmptySessions title="Nothing matches">Try a different search or change the source filter.</EmptySessions>
        ) : visibleSessions.map((session) => (
          <SessionRow key={session.id} session={session} onOpen={() => onOpen(session.id)} onDelete={deleteRecordedSession} onUpdate={updateRecordedSession} />
        ))}
      </div>
    </>
  );
}

function SessionRow({ session, onOpen, onDelete, onUpdate }: { session: RecordedSessionSummary; onOpen: () => void; onDelete: (session: RecordedSessionSummary) => Promise<boolean>; onUpdate: (session: RecordedSessionSummary, title: string | null, driver: string | null, ownership: RecordedSessionSummary["ownership"], tags: string[]) => Promise<boolean> }) {
  const showToast = useToast();
  const [actionsOpen, setActionsOpen] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [editingDetails, setEditingDetails] = useState(false);
  const [exportMenuOpen, setExportMenuOpen] = useState(false);
  const [draftTitle, setDraftTitle] = useState(session.title ?? "");
  const [draftDriver, setDraftDriver] = useState(session.driver ?? "");
  const [draftOwnership, setDraftOwnership] = useState<RecordedSessionSummary["ownership"]>(session.ownership);
  const [draftTags, setDraftTags] = useState(session.tags.join(", "));
  const [exporting, setExporting] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [savingDetails, setSavingDetails] = useState(false);
  const actionsMenu = useRef<HTMLDivElement>(null);
  const timedLaps = session.laps.filter((lap) => lap.time !== "—" && lap.validity !== "invalid");
  const bestLap = timedLaps.slice().sort((left, right) => lapDuration(left) - lapDuration(right))[0];

  useEffect(() => {
    if (!actionsOpen) return;
    function dismissOnPointerDown(event: PointerEvent) {
      if (!actionsMenu.current?.contains(event.target as Node)) {
        setActionsOpen(false);
        setConfirmingDelete(false);
        setEditingDetails(false);
        setExportMenuOpen(false);
      }
    }
    function dismissOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setActionsOpen(false);
        setConfirmingDelete(false);
        setEditingDetails(false);
        setExportMenuOpen(false);
      }
    }
    document.addEventListener("pointerdown", dismissOnPointerDown);
    document.addEventListener("keydown", dismissOnEscape);
    return () => {
      document.removeEventListener("pointerdown", dismissOnPointerDown);
      document.removeEventListener("keydown", dismissOnEscape);
    };
  }, [actionsOpen]);

  async function exportTelemetry(exportFormat: SessionExportFormat) {
    setExporting(true);
    try {
      const result = await telemetryDataSource.exportSession(session.id, exportFormat);
      showToast({ kind: "success", title: `${result.format} exported`, message: `${result.sampleCount.toLocaleString()} samples saved to ${result.path}`, timeoutMs: 7_000 });
      setExportMenuOpen(false);
      setActionsOpen(false);
    } catch (error) {
      showToast({ kind: "error", title: "Export failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
    } finally {
      setExporting(false);
    }
  }

  async function deleteRecording() {
    setDeleting(true);
    const deleted = await onDelete(session);
    setDeleting(false);
    if (deleted) setActionsOpen(false);
  }

  async function saveDetails() {
    setSavingDetails(true);
    const title = draftTitle.trim() || null;
    const seen = new Set<string>();
    const tags = draftTags.split(",").map((tag) => tag.trim()).filter((tag) => {
      const key = tag.toLocaleLowerCase();
      if (!tag || seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    const driver = draftDriver.trim() || null;
    const saved = await onUpdate(session, title, driver, draftOwnership, tags);
    setSavingDetails(false);
    if (saved) {
      setEditingDetails(false);
      setActionsOpen(false);
    }
  }

  return (
    <article className="relative border-b border-trace-divider last:border-b-0">
      <div className="flex items-stretch">
        <button
          type="button"
          aria-label={`View ${session.track} session`}
          onClick={onOpen}
          className="group grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_88px_124px_20px] items-center gap-6 border-0 bg-transparent px-5 py-5 text-left hover:bg-trace-raised"
        >
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-3">
              <span className="shrink-0 font-mono text-[12px] font-extrabold tracking-[.1em] text-trace-accent">{friendlySessionType(session)}</span>
              <h2 className="min-w-0 truncate text-base font-black tracking-[.02em]">{session.title ?? session.track}</h2>
              {session.ownership !== "unknown" && <OwnershipBadge ownership={session.ownership} />}
            </div>
            <span className="mt-2 flex min-w-0 items-center gap-2 text-[12px] text-trace-dim">
              <span className="shrink-0 text-trace-soft">{session.car}</span>
              {session.title && <><span aria-hidden="true">·</span><span className="min-w-0 truncate">{session.track}</span></>}
              <span aria-hidden="true">·</span>
              <Tooltip className="shrink-0" content={session.startedAt}>{formatSessionDate(session.startedAt)}</Tooltip>
              {session.driver && <><span aria-hidden="true">·</span><span className="min-w-0 truncate text-trace-muted">{session.driver}</span></>}
            </span>
          </div>
          <div className="font-mono text-right">
            <strong className="block text-sm text-trace-soft">{session.laps.length}</strong>
            <span className="mt-1 block text-[12px] tracking-[.08em] text-trace-dim">LAPS</span>
          </div>
          <div className="font-mono text-right">
            <strong className="block text-sm text-trace-soft">{bestLap?.time ?? "—"}</strong>
            <span className="mt-1 block text-[12px] tracking-[.08em] text-trace-dim">BEST LAP</span>
          </div>
          <svg className="size-4 fill-none stroke-current text-trace-muted transition-transform group-hover:translate-x-0.5 group-hover:text-trace-text" viewBox="0 0 16 16" aria-hidden="true">
            <path d="m6 4 4 4-4 4" />
          </svg>
        </button>
        <div className="flex shrink-0 items-stretch border-l border-trace-divider">
          <div className="relative flex" ref={actionsMenu}>
            <Tooltip className="h-full" content="Session actions">
              <button
                type="button"
                aria-label={`Actions for ${session.track} session`}
                aria-expanded={actionsOpen}
                onClick={() => { setActionsOpen((value) => !value); setConfirmingDelete(false); setEditingDetails(false); setExportMenuOpen(false); }}
                className="grid h-full w-12 place-items-center border-0 bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-text"
              >
                <svg className="size-4 fill-current" viewBox="0 0 16 16" aria-hidden="true">
                  <circle cx="3" cy="8" r="1.2" /><circle cx="8" cy="8" r="1.2" /><circle cx="13" cy="8" r="1.2" />
                </svg>
              </button>
            </Tooltip>
            {actionsOpen && (
              <div className="absolute right-0 top-[calc(100%-10px)] z-20 max-h-[calc(100vh-80px)] w-72 overflow-y-auto border border-trace-divider bg-trace-black p-2 shadow-[0_12px_30px_#000]">
                {confirmingDelete ? (
                  <DeleteConfirmation session={session} deleting={deleting} onCancel={() => setConfirmingDelete(false)} onConfirm={() => void deleteRecording()} />
                ) : editingDetails ? (
                  <SessionDetailsEditor title={draftTitle} driver={draftDriver} ownership={draftOwnership} tags={draftTags} saving={savingDetails} onTitleChange={setDraftTitle} onDriverChange={setDraftDriver} onOwnershipChange={setDraftOwnership} onTagsChange={setDraftTags} onCancel={() => setEditingDetails(false)} onSave={() => void saveDetails()} />
                ) : (
                  <>
                    <span className="block px-2 pb-2 pt-1 text-[12px] font-bold text-trace-soft">Session actions</span>
                    <button type="button" onClick={() => { setDraftTitle(session.title ?? ""); setDraftDriver(session.driver ?? ""); setDraftOwnership(session.ownership); setDraftTags(session.tags.join(", ")); setExportMenuOpen(false); setEditingDetails(true); }} className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-text hover:bg-trace-raised">Name, driver & tags…</button>
                    <button
                      type="button"
                      aria-expanded={exportMenuOpen}
                      disabled={exporting || !session.exportable}
                      onClick={() => setExportMenuOpen((value) => !value)}
                      className="flex w-full items-center justify-between border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-text hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent"
                    >
                      <span>{exporting ? "Exporting…" : "Export…"}</span>
                      <svg className={`size-3 fill-none stroke-current transition-transform ${exportMenuOpen ? "rotate-90" : ""}`} viewBox="0 0 12 12" aria-hidden="true">
                        <path d="m4.5 2.5 3 3.5-3 3.5" />
                      </svg>
                    </button>
                    {exportMenuOpen && session.exportable && (
                      <div className="ml-2 border-l border-trace-divider bg-trace-deep pb-1 pl-1 pt-2">
                        <span className="block px-2 pb-1 font-mono text-[9px] font-bold tracking-[.12em] text-trace-dim">SHARE</span>
                        <ExportOption label="Shareable session" detail=".trace · compact telemetry, laps & details" disabled={exporting} onClick={() => void exportTelemetry("trace")} />
                        <span className="mt-1 block border-t border-trace-divider px-2 pb-1 pt-2 font-mono text-[9px] font-bold tracking-[.12em] text-trace-dim">DATA EXPORTS</span>
                        <ExportOption label="Raw telemetry" detail="Arrow IPC · all captured channels" disabled={exporting} onClick={() => void exportTelemetry("arrow")} />
                        <ExportOption label="Spreadsheet" detail="CSV · core channels" disabled={exporting} onClick={() => void exportTelemetry("csv")} />
                      </div>
                    )}
                    {!session.exportable && <p className="px-2 py-2 text-[12px] leading-4 text-trace-dim">This session has no finalized telemetry to export.</p>}
                    <div className="my-1 border-t border-trace-divider" />
                    <button type="button" disabled={!session.deletable} onClick={() => { setExportMenuOpen(false); setConfirmingDelete(true); }} className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-warning hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent">{session.deletable ? "Delete session…" : "Session in progress"}</button>
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </article>
  );
}

function SessionDetail({ session, onOpenLap }: { session: RecordedSessionSummary; onOpenLap: (lapIndex: number) => void }) {
  const [metrics, setMetrics] = useState<RecordedLapMetrics[]>([]);
  const [metricsState, setMetricsState] = useState<"loading" | "ready" | "error">("loading");
  const metricsByLap = useMemo(() => new Map(metrics.map((value) => [value.lapIndex, value])), [metrics]);
  const hasSectorTiming = session.laps.some((lap) => lap.sectors.length > 0);
  const sectorIndices = [...new Set(session.laps.flatMap((lap) => lap.sectors.map((sector) => sector.index)))].sort((left, right) => left - right);
  const timedLaps = session.laps.filter((lap) => lap.time !== "—" && !lapIsInvalid(lap));
  const bestLap = timedLaps.slice().sort((left, right) => lapDuration(left) - lapDuration(right))[0];
  const fastestDuration = bestLap ? lapDuration(bestLap) : Number.POSITIVE_INFINITY;
  const theoreticalBest = theoreticalBestLap(session.laps, sectorIndices);

  useEffect(() => {
    let active = true;
    setMetricsState("loading");
    void telemetryDataSource.getSessionLapMetrics(session.id).then((values) => {
      if (!active) return;
      setMetrics(values);
      setMetricsState("ready");
    }).catch(() => {
      if (active) setMetricsState("error");
    });
    return () => { active = false; };
  }, [session.id]);

  return (
    <>
      <div className="flex items-end justify-between gap-6">
        <div className="min-w-0">
          <SectionHeading index="02">SESSION OVERVIEW</SectionHeading>
          <h1 className="mt-3 truncate text-2xl font-black tracking-[-.02em]">{session.title ?? session.track}</h1>
          <p className="mt-2 text-[13px] text-trace-muted">{session.car} · {friendlySessionType(session)} · {formatSessionDate(session.startedAt)}</p>
        </div>
        <div className="flex shrink-0 items-center gap-3">
          {session.ownership !== "unknown" && <OwnershipBadge ownership={session.ownership} />}
          {session.driver && <span className="text-[12px] text-trace-soft">{session.driver}</span>}
        </div>
      </div>

      <div className="mt-6 grid grid-cols-5 border border-trace-divider bg-trace-surface">
        <Metric label="LAPS" value={String(session.laps.length)} accent />
        <Metric label="FASTEST LAP" value={bestLap?.time ?? "—"} />
        <Metric label="THEORETICAL BEST" value={theoreticalBest ?? "—"} detail="The quickest valid time recorded in each sector, added together." purple />
        <Metric label="SOURCE" value={sessionSourceLabel(session).toUpperCase()} />
        <Metric label="SIMULATOR" value={session.simulatorName.toUpperCase()} />
      </div>

      {(session.ambientTemperatureC || session.roadTemperatureC || session.weatherName || session.trackGripPercent != null) && (
        <div className="mt-3 flex flex-wrap items-center gap-x-7 gap-y-2 border border-trace-divider bg-trace-surface px-5 py-3 font-mono text-[11px] text-trace-dim">
          <strong className="tracking-[.1em] text-trace-soft">SESSION CONDITIONS</strong>
          {session.ambientTemperatureC && <span>AIR <strong className="text-trace-text">{session.ambientTemperatureC}°C</strong></span>}
          {session.roadTemperatureC && <span>TRACK <strong className="text-trace-text">{session.roadTemperatureC}°C</strong></span>}
          {session.trackGripPercent != null && <span>STARTING GRIP <strong className="text-trace-text">{session.trackGripPercent}%</strong></span>}
          {session.weatherName && <span>WEATHER <strong className="text-trace-text">{friendlyConditionName(session.weatherName)}</strong></span>}
        </div>
      )}

      {!hasSectorTiming && (
        <div className="mt-4 border border-trace-divider bg-trace-surface px-4 py-3 text-[12px] leading-5 text-trace-muted">
          <strong className="text-trace-soft">No sector timing was emitted for this session.</strong> Lap telemetry and derived metrics remain available.
        </div>
      )}

      {metricsState === "error" && (
        <div className="mt-4 border border-trace-warning/40 bg-trace-warning/10 px-4 py-3 text-[12px] text-trace-warning">Lap times are available, but the additional fuel, speed, and tyre summaries could not be loaded.</div>
      )}

      <div className="mt-4 border border-trace-divider bg-trace-surface">
        <div className="flex items-center justify-between border-b border-trace-divider px-5 py-4">
          <h2 className="text-[13px] font-black tracking-[.04em]">LAPS</h2>
          <span className="font-mono text-[12px] text-trace-faint">{session.laps.length} TOTAL</span>
        </div>
        <div className="grid grid-cols-[60px_100px_minmax(280px,1fr)_168px_126px_136px] items-center gap-6 border-b border-trace-divider bg-trace-deep px-6 py-3 font-mono text-[12px] font-bold tracking-[.08em] text-trace-dim">
          <span>LAP</span><span>TIME</span><span>SECTORS</span><span className="border-l border-trace-divider pl-6">FUEL</span><span className="border-l border-trace-divider pl-6">TOP SPEED</span><span className="border-l border-trace-divider pl-6">TYRES</span>
        </div>
        {session.laps.length === 0 ? (
          <div className="p-8 text-center text-[12px] text-trace-dim">No complete laps are available.</div>
        ) : session.laps.map((lap) => {
          const lapMetrics = metricsByLap.get(lap.index);
          const invalid = lapIsInvalid(lap);
          const fastest = !invalid && lap.time !== "—" && lapDuration(lap) === fastestDuration;
          return (
            <div
              className={`grid min-h-[108px] grid-cols-[60px_100px_minmax(280px,1fr)_168px_126px_136px] items-center gap-6 border-b border-l-2 border-b-trace-divider px-6 py-4 font-mono text-[12px] last:border-b-0 ${invalid ? "border-l-trace-danger bg-trace-danger/15" : fastest ? "border-l-trace-purple bg-trace-purple/10 shadow-[inset_0_0_28px_rgba(184,124,255,0.04)]" : "border-l-transparent"}`}
              key={lap.index}
              role="button"
              tabIndex={0}
              onClick={() => onOpenLap(lap.index)}
              onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); onOpenLap(lap.index); } }}
              aria-label={`Visualize lap ${lap.index}`}
            >
              <Tooltip content={invalid ? lapInvalidityDetail(lap) : null}>
                <span className={invalid ? "text-red-300" : fastest ? "text-trace-purple" : "text-trace-faint"}>{String(lap.index).padStart(2, "0")}</span>
              </Tooltip>
              <strong className={invalid ? "text-red-200" : fastest ? "text-trace-purple" : "text-trace-text"}>{lap.time}</strong>
              {hasSectorTiming ? <SectorBars lap={lap} laps={session.laps} sectorIndices={sectorIndices} /> : <span className="text-[12px] text-trace-dim">UNAVAILABLE</span>}
              <div className="flex min-h-16 items-center border-l border-trace-divider pl-6"><FuelUsage state={metricsState} metrics={lapMetrics} /></div>
              <div className="flex min-h-16 items-center border-l border-trace-divider pl-6"><LapMetricValue state={metricsState} value={lapMetrics?.maxSpeedKmh != null ? `${lapMetrics.maxSpeedKmh.toFixed(1)} km/h` : null} /></div>
              <div className="flex min-h-16 items-center justify-between gap-3 border-l border-trace-divider pl-6"><TyreWearGrid state={metricsState} metrics={lapMetrics} /><svg className="size-4 shrink-0 fill-none stroke-current text-trace-dim" viewBox="0 0 16 16" aria-hidden="true"><path d="m6 3 5 5-5 5" /></svg></div>
            </div>
          );
        })}
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 font-mono text-[12px] text-trace-dim">
        <SectorLegend colour="bg-trace-purple" label="Session best" />
        <SectorLegend colour="bg-trace-accent" label="Improved" />
        <SectorLegend colour="bg-trace-sector-yellow" label="Slower" />
      </div>
    </>
  );
}

function LapMetricValue({ state, value, detail }: { state: "loading" | "ready" | "error"; value: string | null; detail?: string | null }) {
  const label = state === "loading" ? "LOADING…" : value ?? "—";
  const className = `truncate text-[12px] ${value ? "text-trace-soft" : "text-trace-dim"}`;
  return detail ? <Tooltip className={className} content={detail}>{label}</Tooltip> : <span className={className}>{label}</span>;
}

function formatFuelUsed(metrics?: RecordedLapMetrics) {
  return metrics?.fuelUsedLitres != null ? `${metrics.fuelUsedLitres.toFixed(2)} L` : null;
}

function fuelDetail(metrics?: RecordedLapMetrics) {
  return metrics?.fuelStartLitres != null && metrics.fuelEndLitres != null
    ? `${metrics.fuelStartLitres.toFixed(2)} L → ${metrics.fuelEndLitres.toFixed(2)} L`
    : null;
}

function FuelUsage({ state, metrics }: { state: "loading" | "ready" | "error"; metrics?: RecordedLapMetrics }) {
  const used = formatFuelUsed(metrics);
  const capacity = metrics?.fuelCapacityLitres;
  const remaining = metrics?.fuelEndLitres;
  if (state === "loading") return <span className="text-[12px] text-trace-dim">LOADING…</span>;
  if (capacity == null || remaining == null || !Number.isFinite(capacity) || !Number.isFinite(remaining) || capacity <= 0) {
    return <LapMetricValue state={state} value={used} detail={fuelDetail(metrics)} />;
  }
  const percentage = Math.min(100, Math.max(0, remaining / capacity * 100));
  const fill = percentage <= 10 ? "bg-trace-danger" : percentage <= 25 ? "bg-trace-warning" : "bg-trace-accent";
  const detail = `${remaining.toFixed(2)} L of ${capacity.toFixed(2)} L remaining${used ? ` · ${used} consumed this lap` : ""}`;
  return (
    <Tooltip className="flex w-full min-w-0 flex-col" content={detail}>
      <span className="flex w-full items-center justify-between gap-2 text-[12px]">
        <span className="truncate text-trace-soft">{used ? `${used} USED` : "—"}</span>
        <span className="shrink-0 text-trace-faint">{Math.round(percentage)}%</span>
      </span>
      <span className="mt-2 block h-1.5 w-full bg-trace-divider" aria-hidden="true">
        <span className={`block h-full ${fill}`} style={{ width: `${percentage}%` }} />
      </span>
    </Tooltip>
  );
}

function TyreWearGrid({ state, metrics }: { state: "loading" | "ready" | "error"; metrics?: RecordedLapMetrics }) {
  if (state === "loading") return <span className="text-[12px] text-trace-dim">LOADING…</span>;
  const tyres = [
    { short: "FL", name: "Front left", index: 0 },
    { short: "FR", name: "Front right", index: 1 },
    { short: "RL", name: "Rear left", index: 2 },
    { short: "RR", name: "Rear right", index: 3 },
  ];
  return (
    <div className="grid w-fit grid-cols-2 gap-1" aria-label="Tyre condition remaining at the end of this lap">
      {tyres.map((tyre) => {
        const start = metrics?.tyreWearStart[tyre.index];
        const end = metrics?.tyreWearEnd[tyre.index];
        const remaining = end != null && Number.isFinite(end)
          ? Math.min(100, Math.max(0, end))
          : null;
        const colour = remaining == null ? null : tyreConditionColour(remaining);
        const value = remaining == null ? "—" : `${Math.round(remaining)}%`;
        const used = start != null && end != null && Number.isFinite(start) && Number.isFinite(end)
          ? Math.max(0, start - end)
          : null;
        const detail = remaining == null
          ? `${tyre.name}: wear telemetry unavailable`
          : `${tyre.name}: ${value} condition remaining${start != null ? ` · ${start.toFixed(2)}% to ${remaining.toFixed(2)}%${used != null ? ` · ${used.toFixed(2)}% used this lap` : ""}` : ""}`;
        return (
          <Tooltip key={tyre.short} content={detail}>
            <span
              className="flex size-9 items-center justify-center border font-mono text-[12px] font-bold tabular-nums"
              style={{
                borderRadius: "9999px",
                borderColor: colour?.border ?? "var(--color-trace-divider)",
                backgroundColor: colour?.background ?? "var(--color-trace-deep)",
                color: colour?.text ?? "var(--color-trace-dim)",
              }}
              aria-label={`${tyre.name}: ${remaining == null ? "unavailable" : `${value} condition remaining`}`}
            >
              {value}
            </span>
          </Tooltip>
        );
      })}
    </div>
  );
}

function tyreConditionColour(remainingPercent: number) {
  const condition = Math.min(1, Math.max(0, remainingPercent / 100));
  const hue = 110 * condition;
  return {
    border: `hsl(${hue} 82% 48%)`,
    background: `hsl(${hue} 82% 48% / 0.16)`,
    text: `hsl(${hue} 88% 68%)`,
  };
}

function lapIsInvalid(lap: RecordedSessionSummary["laps"][number]) {
  return lap.validity === "invalid" || (lap.maxTyresOut != null && lap.maxTyresOut >= 3);
}

function SectorLegend({ colour, label }: { colour: string; label: string }) {
  return <span className="inline-flex items-center gap-1.5"><span className={`h-1.5 w-3 ${colour}`} aria-hidden="true" />{label}</span>;
}

function OwnershipBadge({ ownership }: { ownership: Exclude<RecordedSessionSummary["ownership"], "unknown"> }) {
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

function SectorBars({ lap, laps, sectorIndices }: { lap: RecordedSessionSummary["laps"][number]; laps: RecordedSessionSummary["laps"]; sectorIndices: number[] }) {
  return (
    <div className="flex min-w-0 gap-1.5 overflow-x-auto" aria-label={`Sector times for lap ${lap.index}`}>
      {sectorIndices.map((index) => {
        const sector = lap.sectors.find((candidate) => candidate.index === index);
        const performance = sectorPerformance(laps, lap, index);
        const colour = performance === "purple"
          ? "bg-trace-purple"
          : performance === "green"
            ? "bg-trace-accent"
            : performance === "yellow"
              ? "bg-trace-sector-yellow"
              : "bg-trace-dim";
        return (
          <Tooltip className="min-w-[72px] flex-1 flex-col" content={`Sector ${index}: ${sector?.time ?? "unavailable"}`} key={index}>
            <div className={`h-1.5 ${colour}`} aria-hidden="true" />
            <span className="mt-1 block truncate text-[12px] text-trace-faint">S{index} {sector?.time ?? "—"}</span>
          </Tooltip>
        );
      })}
    </div>
  );
}

function sectorPerformance(laps: RecordedSessionSummary["laps"], lap: RecordedSessionSummary["laps"][number], sectorIndex: number) {
  const sector = lap.sectors.find((candidate) => candidate.index === sectorIndex);
  if (!sector || lapIsInvalid(lap)) return "grey";
  const comparable = laps.filter((candidate) => !lapIsInvalid(candidate)).flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex));
  const sessionBest = Math.min(...comparable.map((candidate) => candidate.durationNs));
  if (sector.durationNs === sessionBest) return "purple";
  const priorBest = Math.min(
    ...laps
      .filter((candidate) => candidate.index < lap.index && !lapIsInvalid(candidate))
      .flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex))
      .map((candidate) => candidate.durationNs),
  );
  return sector.durationNs < priorBest ? "green" : "yellow";
}

function ExportOption({ label, detail, disabled, onClick }: { label: string; detail: string; disabled: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="block w-full border-0 bg-transparent px-2 py-2 text-left hover:bg-trace-raised disabled:text-trace-dim"
    >
      <strong className="block font-mono text-[12px] tracking-[.08em] text-trace-text">{label}</strong>
      <span className="mt-1 block text-[12px] text-trace-dim">{detail}</span>
    </button>
  );
}

function DeleteConfirmation({ session, deleting, onCancel, onConfirm }: { session: RecordedSessionSummary; deleting: boolean; onCancel: () => void; onConfirm: () => void }) {
  return (
    <div className="p-2">
      <strong className="block text-[13px] text-trace-text">Delete this session?</strong>
      <p className="mt-2 text-[12px] leading-5 text-trace-faint">
        {session.track} and its saved telemetry will be permanently removed. This cannot be undone.
      </p>
      <div className="mt-4 grid grid-cols-2 gap-2">
        <button type="button" disabled={deleting} onClick={onCancel} className="border border-trace-divider bg-transparent px-3 py-2.5 text-[12px] font-bold text-trace-soft hover:bg-trace-raised disabled:text-trace-dim">Cancel</button>
        <button type="button" disabled={deleting} onClick={onConfirm} className="border border-trace-warning bg-transparent px-3 py-2.5 text-[12px] font-bold text-trace-warning hover:bg-trace-warning hover:text-trace-black disabled:border-trace-divider disabled:text-trace-dim">
          {deleting ? "Deleting…" : "Delete"}
        </button>
      </div>
    </div>
  );
}

function SessionDetailsEditor({ title, driver, ownership, tags, saving, onTitleChange, onDriverChange, onOwnershipChange, onTagsChange, onCancel, onSave }: { title: string; driver: string; ownership: RecordedSessionSummary["ownership"]; tags: string; saving: boolean; onTitleChange: (value: string) => void; onDriverChange: (value: string) => void; onOwnershipChange: (value: RecordedSessionSummary["ownership"]) => void; onTagsChange: (value: string) => void; onCancel: () => void; onSave: () => void }) {
  return (
    <form className="p-2" onSubmit={(event) => { event.preventDefault(); onSave(); }}>
      <strong className="block text-[13px] text-trace-text">Session identity</strong>
      <label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
        DISPLAY NAME
        <input autoFocus maxLength={80} value={title} onChange={(event) => onTitleChange(event.target.value)} placeholder="Optional custom name" className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
      </label>
      <label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
        DRIVER / AUTHOR
        <input maxLength={80} value={driver} onChange={(event) => onDriverChange(event.target.value)} placeholder="Who drove this session?" className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
      </label>
      <label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
        OWNERSHIP
        <select value={ownership} onChange={(event) => onOwnershipChange(event.target.value as RecordedSessionSummary["ownership"])} className="trace-select mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep pl-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent">
          <option value="unknown">Not specified</option>
          <option value="mine">My driving</option>
          <option value="other">Another driver</option>
        </select>
      </label>
      <label className="mt-3 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
        TAGS
        <input value={tags} onChange={(event) => onTagsChange(event.target.value)} placeholder="league, wet, reference" className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent" />
      </label>
      <p className="mt-2 text-[12px] leading-4 text-trace-dim">Separate up to 12 tags with commas. Name, driver, ownership, and tags are included in search.</p>
      <div className="mt-4 grid grid-cols-2 gap-2">
        <button type="button" disabled={saving} onClick={onCancel} className="border border-trace-divider bg-transparent px-3 py-2.5 text-[12px] font-bold text-trace-soft hover:bg-trace-raised disabled:text-trace-dim">Cancel</button>
        <button type="submit" disabled={saving} className="border border-trace-accent bg-trace-accent-wash px-3 py-2.5 text-[12px] font-bold text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:text-trace-dim">{saving ? "Saving…" : "Save"}</button>
      </div>
    </form>
  );
}

function EmptySessions({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="p-12 text-center">
      <span className="trace-crosshair mx-auto block" aria-hidden="true" />
      <strong className="mt-5 block text-base">{title}</strong>
      <p className="mx-auto mt-2 max-w-md text-[12px] leading-5 text-trace-faint">{children}</p>
    </div>
  );
}

function lapInvalidityDetail(lap: RecordedSessionSummary["laps"][number]) {
  const partial = lap.validityReason?.includes("partial") ?? false;
  return partial
    ? "TRACE joined after this lap began, so it is incomplete and excluded from comparisons."
    : lap.maxTyresOut != null && lap.maxTyresOut >= 3
      ? "Three or more tyres were observed outside the track; this lap is excluded from comparisons."
      : lap.validityReason ?? "The simulator marked this lap invalid.";
}

function sessionSourceGroup(session: RecordedSessionSummary) {
  const source = session.source.toLocaleLowerCase();
  if (source.includes("replay")) return "replay";
  if (source.includes("import")) return "imported";
  return "native";
}

function sessionSourceLabel(session: RecordedSessionSummary) {
  const source = sessionSourceGroup(session);
  if (source === "replay") return "Replay capture";
  if (source === "imported") return "Imported telemetry";
  return "Drive";
}

function friendlySessionType(session: RecordedSessionSummary) {
  const type = session.sessionType.toLocaleLowerCase();
  if (type === "session") return "DRIVE";
  if (type === "qualify") return "QUALIFYING";
  if (type === "time_attack") return "TIME ATTACK";
  if (type.includes("replay")) return "REPLAY";
  return session.sessionType;
}

function lapDuration(lap: RecordedSessionSummary["laps"][number]) {
  if (lap.durationNs != null) return lap.durationNs;
  return lapTimeMs(lap.time) * 1_000_000;
}

function theoreticalBestLap(laps: RecordedSessionSummary["laps"], sectorIndices: number[]) {
  if (sectorIndices.length === 0) return null;
  const validLaps = laps.filter((lap) => !lapIsInvalid(lap));
  let totalDurationNs = 0;
  for (const index of sectorIndices) {
    const durations = validLaps
      .flatMap((lap) => lap.sectors)
      .filter((sector) => sector.index === index && Number.isFinite(sector.durationNs) && sector.durationNs > 0)
      .map((sector) => sector.durationNs);
    if (durations.length === 0) return null;
    totalDurationNs += Math.min(...durations);
  }
  const totalMilliseconds = Math.floor(totalDurationNs / 1_000_000);
  const minutes = Math.floor(totalMilliseconds / 60_000);
  const seconds = Math.floor((totalMilliseconds % 60_000) / 1_000);
  const milliseconds = totalMilliseconds % 1_000;
  return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds).padStart(3, "0")}`;
}

function lapTimeMs(value: string) {
  const match = /^(\d+):(\d{2})\.(\d{3})$/.exec(value);
  if (!match) return Number.POSITIVE_INFINITY;
  return Number(match[1]) * 60_000 + Number(match[2]) * 1_000 + Number(match[3]);
}

function formatLapDurationNs(durationNs: number) {
  const totalMilliseconds = Math.max(0, Math.round(durationNs / 1_000_000));
  const minutes = Math.floor(totalMilliseconds / 60_000);
  const seconds = Math.floor((totalMilliseconds % 60_000) / 1_000);
  const milliseconds = totalMilliseconds % 1_000;
  return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds).padStart(3, "0")}`;
}

function formatSessionDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatCompactSessionDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "short",
    year: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function friendlyConditionName(value: string) {
  return value.replace(/^\d+_/, "").replaceAll("_", " ").toUpperCase();
}

function Footer({ status }: { status: TelemetryStatus | null }) {
  return (
    <footer className="col-span-full flex items-center gap-6 border-t border-trace-divider bg-trace-black px-[14px] font-mono text-[12px] tracking-[.06em] text-trace-dim">
      <span>TRACE ENGINE <b className="ml-1 text-trace-accent">READY</b></span>
      <span>{status?.simulatorShortName ?? "SIM"} MODULE <b className="ml-1 text-trace-accent">LIFECYCLE</b></span>
      <span>STORAGE <b className="ml-1 text-trace-accent">LOCAL</b></span>
      <span className="ml-auto">V{desktopPackage.version}</span>
    </footer>
  );
}

function SectionHeading({ index, children }: { index: string; children: ReactNode }) {
  return <div className="text-[12px] font-extrabold tracking-[.14em] text-trace-soft"><span className="mr-2.5 text-trace-accent">{index}</span>{children}</div>;
}

function PanelTitle({ children }: { children: ReactNode }) {
  return <div className="border-b border-trace-divider px-4 py-[14px] text-[12px] font-extrabold tracking-[.14em] text-trace-soft">{children}</div>;
}

function Metric({ label, value, detail, accent = false, purple = false }: { label: string; value: string; detail?: string; accent?: boolean; purple?: boolean }) {
  return (
    <div className="min-h-[92px] border-r border-trace-divider bg-trace-surface p-[18px] last:border-r-0 max-[900px]:[&:nth-child(-n+2)]:border-b max-[900px]:[&:nth-child(even)]:border-r-0">
      <span className="block text-[12px] font-extrabold tracking-[.12em] text-trace-muted">{detail ? <Tooltip content={detail}>{label}</Tooltip> : label}</span>
      <strong className={`mt-[15px] block font-mono text-base font-bold ${purple ? "text-trace-purple" : accent ? "text-trace-accent" : ""}`}>{value}</strong>
    </div>
  );
}
