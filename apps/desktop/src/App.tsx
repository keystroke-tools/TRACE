import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  telemetryDataSource,
  type RecordedSessionSummary,
  type SessionExportFormat,
  type TelemetryStatus,
} from "./data-source";
import { TitleBar } from "./TitleBar";
import { Tooltip } from "./Tooltip";
import { useToast } from "./Toast";

const navigation = ["LIVE", "SESSIONS", "COMPARE", "SETUPS"] as const;
type Section = (typeof navigation)[number];

export function App() {
  const [status, setStatus] = useState<TelemetryStatus | null>(null);
  const [sessions, setSessions] = useState<RecordedSessionSummary[]>([]);
  const [section, setSection] = useState<Section>("LIVE");

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
      <TitleBar status={status} />
      <Navigation active={section} onChange={setSection} />
      <section className="trace-grid overflow-auto p-7">
        {section === "LIVE" && <Live status={status} onOpenSessions={() => setSection("SESSIONS")} onSelectSimulator={selectSimulator} />}
        {section === "SESSIONS" && (
          <Sessions
            sessions={sessions}
            onDeleted={(sessionId) => setSessions((current) => current.filter((session) => session.id !== sessionId))}
            onUpdated={(updated) => setSessions((current) => current.map((session) => session.id === updated.id ? updated : session))}
          />
        )}
        {section === "COMPARE" && <Compare />}
        {section === "SETUPS" && <Setups />}
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
          <span className="font-mono text-[11px] text-trace-dim">0{index + 1}</span>
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
                <strong className={`font-mono text-[10px] tracking-[.1em] ${category.includes("NATIVE") ? "text-trace-purple" : "text-trace-accent"}`}>{category}</strong>
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
          <button type="button" onClick={onOpenSessions} className="mx-5 mb-5 border border-trace-accent-muted bg-trace-accent-wash px-4 py-3 text-[11px] font-black tracking-[.1em] text-trace-accent hover:border-trace-accent">
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
        <span className="font-mono text-[10px] font-bold tracking-[.1em] text-trace-dim">SIMULATOR</span>
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
      <span className="px-4 text-[11px] text-trace-dim">
        {selectable.length} capture adapter{selectable.length === 1 ? "" : "s"} installed
      </span>
    </div>
  );
}

function Compare() {
  return (
    <>
      <PageIntro index="03" eyebrow="LAP COMPARISON" title="UNDERSTAND WHERE TIME IS WON" description="Compare two recorded laps by distance to see the delta, speed, throttle, and brake traces together." />
      <FeaturePreview label="PLANNED / PHASE 3" title="Comparison workspace">
        <div className="grid gap-px bg-trace-divider md:grid-cols-3">
          <PreviewStep number="01" title="Choose a reference" detail="Pick a clean lap from your session archive." />
          <PreviewStep number="02" title="Add a comparison" detail="Select another lap from the same track and layout." />
          <PreviewStep number="03" title="Inspect the difference" detail="TRACE aligns both laps by distance and highlights gains and losses." />
        </div>
      </FeaturePreview>
      <AvailabilityNote>Lap selection and synchronized charts are the next analysis milestone. Your current recordings remain usable when it arrives.</AvailabilityNote>
    </>
  );
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
      <span className="grid size-7 shrink-0 place-items-center border border-trace-accent-muted font-mono text-[10px] text-trace-accent">{number}</span>
      <div><strong className="block text-[13px] text-trace-text">{title}</strong><span className="mt-1 block text-[12px] leading-5 text-trace-faint">{children}</span></div>
    </li>
  );
}

function FeaturePreview({ label, title, children }: { label: string; title: string; children: ReactNode }) {
  return (
    <div className="mt-7 border border-trace-divider bg-trace-surface">
      <div className="flex items-center justify-between border-b border-trace-divider px-5 py-4">
        <h2 className="text-[14px] font-black tracking-[.04em]">{title}</h2>
        <span className="font-mono text-[10px] font-bold tracking-[.08em] text-trace-accent">{label}</span>
      </div>
      {children}
    </div>
  );
}

function PreviewStep({ number, title, detail }: { number: string; title: string; detail: string }) {
  return <div className="min-h-44 bg-trace-surface p-6"><span className="font-mono text-[10px] text-trace-accent">{number}</span><h3 className="mt-7 text-base font-black">{title}</h3><p className="mt-2 text-[13px] leading-5 text-trace-faint">{detail}</p></div>;
}

function AvailabilityNote({ children }: { children: ReactNode }) {
  return <p className="border border-t-0 border-trace-divider bg-trace-deep px-5 py-4 text-[12px] leading-5 text-trace-muted"><strong className="mr-2 text-trace-warning">NOT AVAILABLE YET</strong>{children}</p>;
}

function Sessions({ sessions, onDeleted, onUpdated }: { sessions: RecordedSessionSummary[]; onDeleted: (sessionId: string) => void; onUpdated: (session: RecordedSessionSummary) => void }) {
  const showToast = useToast();
  const [query, setQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [simulatorFilter, setSimulatorFilter] = useState("all");
  const [sortOrder, setSortOrder] = useState("newest");
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
        : { kind: "success", title: "Recording deleted", message: `${session.track} was deleted from your session library.`, timeoutMs: 4_500 });
      return true;
    } catch (error) {
      showToast({ kind: "error", title: "Could not delete recording", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
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

  return (
    <>
      <div className="flex items-end justify-between gap-6">
        <div>
          <SectionHeading index="02">SESSION LIBRARY</SectionHeading>
          <h1 className="mt-3 text-2xl font-black tracking-[-.02em]">YOUR RECORDED SESSIONS</h1>
          <p className="mt-2 text-[13px] text-trace-muted">Find a drive or replay, review its laps, export the telemetry, or remove recordings you no longer need.</p>
        </div>
        <span className="font-mono text-[11px] text-trace-faint">
          {visibleSessions.length} shown · {sessions.length} total
        </span>
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
          className="trace-select h-12 border-0 border-r border-trace-divider bg-trace-surface pl-4 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
        >
          <option value="all">All sources</option>
          <option value="replay">Replays</option>
          <option value="native">Recorded drives</option>
          <option value="imported">Imports</option>
        </select>
        {simulators.length > 1 && (
          <select
            aria-label="Filter sessions by simulator"
            value={simulatorFilter}
            onChange={(event) => setSimulatorFilter(event.target.value)}
            className="trace-select h-12 border-0 border-r border-trace-divider bg-trace-surface pl-4 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
          >
            <option value="all">All simulators</option>
            {simulators.map(([id, name]) => <option value={id} key={id}>{name}</option>)}
          </select>
        )}
        <select
          aria-label="Sort sessions"
          value={sortOrder}
          onChange={(event) => setSortOrder(event.target.value)}
          className="trace-select h-12 border-0 bg-trace-surface pl-4 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
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
          <SessionRow key={session.id} session={session} onDelete={deleteRecordedSession} onUpdate={updateRecordedSession} />
        ))}
      </div>
    </>
  );
}

function SessionRow({ session, onDelete, onUpdate }: { session: RecordedSessionSummary; onDelete: (session: RecordedSessionSummary) => Promise<boolean>; onUpdate: (session: RecordedSessionSummary, title: string | null, driver: string | null, ownership: RecordedSessionSummary["ownership"], tags: string[]) => Promise<boolean> }) {
  const showToast = useToast();
  const [expanded, setExpanded] = useState(false);
  const [showAllLaps, setShowAllLaps] = useState(false);
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
  const fastestDuration = bestLap ? lapDuration(bestLap) : Number.POSITIVE_INFINITY;
  const sectorCount = Math.max(3, ...session.laps.flatMap((lap) => lap.sectors.map((sector) => sector.index)));
  const visibleLaps = showAllLaps ? session.laps : session.laps.slice(0, 3);
  const hiddenLapCount = session.laps.length - visibleLaps.length;

  function toggleExpanded() {
    if (expanded) setShowAllLaps(false);
    setExpanded((value) => !value);
  }

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
      <div className="flex min-h-[82px] items-stretch">
        <button
          type="button"
          aria-expanded={expanded}
          onClick={toggleExpanded}
          className="grid min-w-0 flex-1 grid-cols-[minmax(170px,1.3fr)_minmax(145px,1fr)_100px_120px] items-center gap-5 border-0 bg-transparent px-5 text-left hover:bg-trace-raised max-[1050px]:grid-cols-[minmax(170px,1.3fr)_minmax(130px,1fr)_90px]"
        >
          <div className="min-w-0">
            <span className="block truncate text-[10px] font-extrabold tracking-[.1em] text-trace-accent">{friendlySessionType(session)}</span>
            <h2 className="mt-1.5 truncate text-base font-black tracking-[.03em]">{session.title ?? session.track}</h2>
            <Tooltip className="mt-1 flex min-w-0 font-mono text-[9px] text-trace-dim" content={session.startedAt}>
              <span className="truncate">{session.title ? `${session.track} · ` : ""}{formatSessionDate(session.startedAt)}</span>
            </Tooltip>
          </div>
          <div className="min-w-0">
            <span className="block truncate text-[12px] text-trace-soft">{session.car}</span>
            <span className="mt-1 block truncate text-[11px] text-trace-dim">{session.simulatorName} · {sessionSourceLabel(session)}</span>
            {(session.driver || session.ownership !== "unknown") && (
              <span className={`mt-1 block truncate font-mono text-[9px] font-bold tracking-[.05em] ${session.ownership === "other" ? "text-trace-purple" : "text-trace-accent"}`}>
                {session.ownership === "other" ? "OTHER DRIVER" : session.ownership === "mine" ? "MY DRIVE" : "DRIVER"}{session.driver ? ` · ${session.driver}` : ""}
              </span>
            )}
            {session.tags.length > 0 && <span className="mt-1 block truncate font-mono text-[9px] text-trace-purple">{session.tags.slice(0, 3).map((tag) => `#${tag}`).join("  ")}</span>}
          </div>
          <div className="font-mono">
            <span className="block text-[9px] tracking-[.08em] text-trace-dim">LAPS</span>
            <strong className="mt-1 block text-[12px] text-trace-soft">{session.laps.length}</strong>
          </div>
          <div className="font-mono max-[1050px]:hidden">
            <span className="block text-[9px] tracking-[.08em] text-trace-dim">BEST RECORDED</span>
            <strong className="mt-1 block text-[12px] text-trace-soft">{bestLap?.time ?? "—"}</strong>
          </div>
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
                    <span className="block px-2 pb-2 pt-1 text-[11px] font-bold text-trace-soft">Session actions</span>
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
                      <div className="ml-2 border-l border-trace-divider bg-trace-deep pl-1">
                        <ExportOption label="Full recording" detail="Arrow IPC · all captured channels" disabled={exporting} onClick={() => void exportTelemetry("arrow")} />
                        <ExportOption label="Spreadsheet" detail="CSV · core channels" disabled={exporting} onClick={() => void exportTelemetry("csv")} />
                      </div>
                    )}
                    {!session.exportable && <p className="px-2 py-2 text-[10px] leading-4 text-trace-dim">This session has no finalized telemetry to export.</p>}
                    <div className="my-1 border-t border-trace-divider" />
                    <button type="button" disabled={!session.deletable} onClick={() => { setExportMenuOpen(false); setConfirmingDelete(true); }} className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-warning hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent">{session.deletable ? "Delete recording…" : "Recording in progress"}</button>
                  </>
                )}
              </div>
            )}
          </div>
          <Tooltip className="h-full" content={expanded ? "Hide laps" : "Show laps"}>
            <button
              type="button"
              aria-label={expanded ? `Collapse ${session.track} laps` : `Show ${session.track} laps`}
              onClick={toggleExpanded}
              className="grid h-full w-12 place-items-center border-0 border-l border-trace-divider bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-text"
            >
              <svg className={`size-4 fill-none stroke-current transition-transform ${expanded ? "rotate-180" : ""}`} viewBox="0 0 16 16" aria-hidden="true">
                <path d="m4 6 4 4 4-4" />
              </svg>
            </button>
          </Tooltip>
        </div>
      </div>
      {expanded && (
        <div className="border-t border-trace-divider bg-trace-deep px-5 py-4">
          <div className="mb-3 flex items-center justify-between text-[11px] text-trace-dim">
            <span>{visibleLaps.length} of {session.laps.length} laps shown</span>
            <Tooltip content={session.startedAt}>{formatSessionDate(session.startedAt)}</Tooltip>
          </div>
          <div className="mb-2 flex flex-wrap items-center gap-x-4 gap-y-1 font-mono text-[9px] text-trace-dim" aria-label="Sector colour legend">
            <SectorLegend colour="bg-trace-purple" label="Session best" />
            <SectorLegend colour="bg-trace-accent" label="Improved" />
            <SectorLegend colour="bg-trace-sector-yellow" label="Slower" />
            <SectorLegend colour="bg-trace-dim" label="Unavailable" />
          </div>
          <div className="max-h-64 overflow-y-auto border border-trace-divider bg-trace-surface">
            {session.laps.length === 0 ? (
              <div className="p-5 text-[12px] text-trace-dim">No completed lap boundaries were recorded.</div>
            ) : visibleLaps.map((lap) => {
              const fastest = lap.time !== "—" && lapDuration(lap) === fastestDuration;
              return (
              <div className={`grid min-h-16 grid-cols-[72px_92px_minmax(190px,1fr)_92px] items-center gap-3 border-b px-4 font-mono text-[10px] last:border-b-0 ${fastest ? "border-l-2 border-l-trace-purple border-b-trace-divider bg-trace-purple-wash" : "border-b-trace-divider"}`} key={lap.index}>
                <span className={fastest ? "text-trace-purple" : "text-trace-faint"}>LAP {String(lap.index).padStart(2, "0")}</span>
                <strong className={fastest ? "text-trace-purple" : lap.validity === "valid" ? "text-trace-text" : "text-trace-soft"}>{lap.time}</strong>
                <SectorBars lap={lap} laps={session.laps} sectorCount={sectorCount} />
                <LapValidity lap={lap} />
              </div>
              );
            })}
          </div>
          {session.laps.length > 3 && (
            <button type="button" onClick={() => setShowAllLaps((value) => !value)} className="mt-3 border border-trace-divider bg-trace-surface px-3 py-2 text-[11px] font-bold text-trace-soft hover:bg-trace-raised hover:text-trace-text">
              {showAllLaps ? "Show first 3 laps" : `Show ${hiddenLapCount} more lap${hiddenLapCount === 1 ? "" : "s"}`}
            </button>
          )}
          <p className="mt-3 text-[11px] leading-5 text-trace-dim">
            “Recorded” means TRACE captured the complete lap. Tyres-out evidence is shown when available; {session.simulatorName} does not provide TRACE with a definitive final lap-valid verdict.
          </p>
        </div>
      )}
    </article>
  );
}

function SectorLegend({ colour, label }: { colour: string; label: string }) {
  return <span className="inline-flex items-center gap-1.5"><span className={`h-1.5 w-3 ${colour}`} aria-hidden="true" />{label}</span>;
}

function SectorBars({ lap, laps, sectorCount }: { lap: RecordedSessionSummary["laps"][number]; laps: RecordedSessionSummary["laps"]; sectorCount: number }) {
  return (
    <div className="flex min-w-0 gap-1.5" aria-label={`Sector times for lap ${lap.index}`}>
      {Array.from({ length: sectorCount }, (_, offset) => offset + 1).map((index) => {
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
          <Tooltip className="min-w-0 flex-1 flex-col" content={`Sector ${index}: ${sector?.time ?? "unavailable"}`} key={index}>
            <div className={`h-1.5 ${colour}`} aria-hidden="true" />
            <span className="mt-1 block truncate text-[9px] text-trace-faint">S{index} {sector?.time ?? "—"}</span>
          </Tooltip>
        );
      })}
    </div>
  );
}

function sectorPerformance(laps: RecordedSessionSummary["laps"], lap: RecordedSessionSummary["laps"][number], sectorIndex: number) {
  const sector = lap.sectors.find((candidate) => candidate.index === sectorIndex);
  if (!sector || lap.validity === "invalid" || lap.validityReason?.includes("partial")) return "grey";
  const comparable = laps.flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex));
  const sessionBest = Math.min(...comparable.map((candidate) => candidate.durationNs));
  if (sector.durationNs === sessionBest) return "purple";
  const priorBest = Math.min(
    ...laps
      .filter((candidate) => candidate.index < lap.index)
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
      <strong className="block font-mono text-[10px] tracking-[.08em] text-trace-text">{label}</strong>
      <span className="mt-1 block text-[10px] text-trace-dim">{detail}</span>
    </button>
  );
}

function DeleteConfirmation({ session, deleting, onCancel, onConfirm }: { session: RecordedSessionSummary; deleting: boolean; onCancel: () => void; onConfirm: () => void }) {
  return (
    <div className="p-2">
      <strong className="block text-[13px] text-trace-text">Delete this recording?</strong>
      <p className="mt-2 text-[11px] leading-5 text-trace-faint">
        {session.track} and its saved telemetry will be permanently removed. This cannot be undone.
      </p>
      <div className="mt-4 grid grid-cols-2 gap-2">
        <button type="button" disabled={deleting} onClick={onCancel} className="border border-trace-divider bg-transparent px-3 py-2.5 text-[11px] font-bold text-trace-soft hover:bg-trace-raised disabled:text-trace-dim">Cancel</button>
        <button type="button" disabled={deleting} onClick={onConfirm} className="border border-trace-warning bg-transparent px-3 py-2.5 text-[11px] font-bold text-trace-warning hover:bg-trace-warning hover:text-trace-black disabled:border-trace-divider disabled:text-trace-dim">
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
      <label className="mt-3 block text-[10px] font-bold tracking-[.08em] text-trace-dim">
        DISPLAY NAME
        <input autoFocus maxLength={80} value={title} onChange={(event) => onTitleChange(event.target.value)} placeholder="Optional custom name" className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-purple" />
      </label>
      <label className="mt-3 block text-[10px] font-bold tracking-[.08em] text-trace-dim">
        DRIVER / AUTHOR
        <input maxLength={80} value={driver} onChange={(event) => onDriverChange(event.target.value)} placeholder="Who drove this recording?" className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-purple" />
      </label>
      <label className="mt-3 block text-[10px] font-bold tracking-[.08em] text-trace-dim">
        OWNERSHIP
        <select value={ownership} onChange={(event) => onOwnershipChange(event.target.value as RecordedSessionSummary["ownership"])} className="trace-select mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep pl-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-purple">
          <option value="unknown">Not specified</option>
          <option value="mine">My driving</option>
          <option value="other">Another driver</option>
        </select>
      </label>
      <label className="mt-3 block text-[10px] font-bold tracking-[.08em] text-trace-dim">
        TAGS
        <input value={tags} onChange={(event) => onTagsChange(event.target.value)} placeholder="league, wet, reference" className="mt-1.5 h-10 w-full border border-trace-divider bg-trace-deep px-3 text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-purple" />
      </label>
      <p className="mt-2 text-[9px] leading-4 text-trace-dim">Separate up to 12 tags with commas. Name, driver, ownership, and tags are included in search.</p>
      <div className="mt-4 grid grid-cols-2 gap-2">
        <button type="button" disabled={saving} onClick={onCancel} className="border border-trace-divider bg-transparent px-3 py-2.5 text-[11px] font-bold text-trace-soft hover:bg-trace-raised disabled:text-trace-dim">Cancel</button>
        <button type="submit" disabled={saving} className="border border-trace-purple bg-trace-purple-wash px-3 py-2.5 text-[11px] font-bold text-trace-purple hover:bg-trace-purple hover:text-trace-black disabled:border-trace-divider disabled:text-trace-dim">{saving ? "Saving…" : "Save"}</button>
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

function LapValidity({ lap }: { lap: RecordedSessionSummary["laps"][number] }) {
  const hasTrackLimitEvidence = lap.maxTyresOut != null && lap.maxTyresOut > 0;
  const label = hasTrackLimitEvidence
    ? `${lap.maxTyresOut} ${lap.maxTyresOut === 1 ? "TYRE" : "TYRES"} OUT`
    : lap.validity === "unknown"
      ? "RECORDED"
    : lap.validityReason?.includes("partial")
      ? "PARTIAL"
      : lap.validity.toUpperCase();
  const className = `justify-end text-right text-[9px] font-bold tracking-[.08em] ${hasTrackLimitEvidence || lap.validity === "invalid" ? "text-trace-warning" : lap.validity === "valid" ? "text-trace-accent" : "text-trace-soft"}`;
  return lap.validityReason
    ? <Tooltip className={className} content={lap.validityReason}>{label}</Tooltip>
    : <span className={className}>{label}</span>;
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
  return "Recorded drive";
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

function lapTimeMs(value: string) {
  const match = /^(\d+):(\d{2})\.(\d{3})$/.exec(value);
  if (!match) return Number.POSITIVE_INFINITY;
  return Number(match[1]) * 60_000 + Number(match[2]) * 1_000 + Number(match[3]);
}

function formatSessionDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function Footer({ status }: { status: TelemetryStatus | null }) {
  return (
    <footer className="col-span-full flex items-center gap-6 border-t border-trace-divider bg-trace-black px-[14px] font-mono text-[10px] tracking-[.06em] text-trace-dim">
      <span>TRACE ENGINE <b className="ml-1 text-trace-accent">READY</b></span>
      <span>{status?.simulatorShortName ?? "SIM"} MODULE <b className="ml-1 text-trace-accent">LIFECYCLE</b></span>
      <span>STORAGE <b className="ml-1 text-trace-accent">LOCAL</b></span>
      <span className="ml-auto">V0.1.0 / PHASE 2</span>
    </footer>
  );
}

function SectionHeading({ index, children }: { index: string; children: ReactNode }) {
  return <div className="text-[11px] font-extrabold tracking-[.14em] text-trace-soft"><span className="mr-2.5 text-trace-accent">{index}</span>{children}</div>;
}

function PanelTitle({ children }: { children: ReactNode }) {
  return <div className="border-b border-trace-divider px-4 py-[14px] text-[11px] font-extrabold tracking-[.14em] text-trace-soft">{children}</div>;
}

function Metric({ label, value, accent = false }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="min-h-[92px] border-r border-trace-divider bg-trace-surface p-[18px] last:border-r-0 max-[900px]:[&:nth-child(-n+2)]:border-b max-[900px]:[&:nth-child(even)]:border-r-0">
      <span className="block text-[11px] font-extrabold tracking-[.12em] text-trace-muted">{label}</span>
      <strong className={`mt-[15px] block font-mono text-base font-bold ${accent ? "text-trace-accent" : ""}`}>{value}</strong>
    </div>
  );
}
