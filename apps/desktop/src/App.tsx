import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  telemetryDataSource,
  type RecordedSessionSummary,
  type SessionExportFormat,
  type TelemetryStatus,
} from "./data-source";
import { TitleBar } from "./TitleBar";

const navigation = ["LIVE", "SESSIONS", "COMPARE", "SETUPS"] as const;
type Section = (typeof navigation)[number];

export function App() {
  const [status, setStatus] = useState<TelemetryStatus | null>(null);
  const [sessions, setSessions] = useState<RecordedSessionSummary[]>([]);
  const [section, setSection] = useState<Section>("LIVE");

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
        {section === "LIVE" && <Live status={status} onOpenSessions={() => setSection("SESSIONS")} />}
        {section === "SESSIONS" && <Sessions sessions={sessions} />}
        {section === "COMPARE" && <Compare />}
        {section === "SETUPS" && <Setups />}
      </section>
      <Footer />
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

function Live({ status, onOpenSessions }: { status: TelemetryStatus | null; onOpenSessions: () => void }) {
  const recording = status?.connection === "recording" || status?.connection === "replay";
  const availableChannels = status?.channels.filter((channel) => channel.available) ?? [];
  const unavailableChannels = status?.channels.filter((channel) => !channel.available) ?? [];
  const categories = Array.from(new Set(availableChannels.map((channel) => channel.category)));

  return (
    <>
      <PageIntro
        index="01"
        eyebrow="LIVE CAPTURE"
        title={recording ? "RECORDING ASSETTO CORSA" : "READY WHEN ASSETTO CORSA IS"}
        description={recording
          ? "TRACE is recording the current drive or replay automatically. Keep it running until the session ends."
          : "Start a drive or play a replay in Assetto Corsa. TRACE detects it and records locally—there is no record button to press."}
      />
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
            These are the verified Assetto Corsa channels TRACE exposes for analysis. This is adapter coverage, not a live sensor test.
          </p>
          <div className="grid grid-cols-2 gap-px p-4 max-[900px]:grid-cols-1">
            {categories.map((category) => (
              <div className="border border-trace-divider bg-trace-deep p-4" key={category}>
                <strong className="font-mono text-[10px] tracking-[.1em] text-trace-accent">{category}</strong>
                <div className="mt-3 flex flex-wrap gap-2">
                  {availableChannels.filter((channel) => channel.category === category).map((channel) => (
                    <span className="border border-trace-divider bg-trace-surface px-2.5 py-1.5 text-[12px] text-trace-soft" title={channel.detail} key={channel.id}>
                      {channel.label}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
        <div>
          <PanelTitle>HOW CAPTURE WORKS</PanelTitle>
          <ol className="space-y-5 p-5">
            <WorkflowStep number="1" title="Open Assetto Corsa">Start driving or play a replay at normal speed.</WorkflowStep>
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
            WHY SOME AC DATA IS NOT AVAILABLE
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

function Sessions({ sessions }: { sessions: RecordedSessionSummary[] }) {
  const [query, setQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [sortOrder, setSortOrder] = useState("newest");
  const visibleSessions = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return sessions
      .filter((session) => {
        const source = sessionSourceGroup(session);
        const matchesSource = sourceFilter === "all" || source === sourceFilter;
        const searchable = [session.track, session.car, session.sessionType, session.source]
          .join(" ")
          .toLocaleLowerCase();
        return matchesSource && (!normalizedQuery || searchable.includes(normalizedQuery));
      })
      .slice()
      .sort((left, right) => {
        const difference = new Date(right.startedAt).getTime() - new Date(left.startedAt).getTime();
        return sortOrder === "newest" ? difference : -difference;
      });
  }, [query, sessions, sortOrder, sourceFilter]);

  return (
    <>
      <div className="flex items-end justify-between gap-6">
        <div>
          <SectionHeading index="02">RECORDED SESSIONS</SectionHeading>
          <h1 className="mt-3 text-2xl font-black tracking-[-.02em]">LOCAL TELEMETRY ARCHIVE</h1>
          <p className="mt-2 text-[13px] text-trace-muted">Search recordings, expand a session to inspect its laps, or export telemetry for analysis.</p>
        </div>
        <span className="font-mono text-[11px] tracking-[.1em] text-trace-faint">
          {visibleSessions.length} / {sessions.length} SESSION(S)
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
            placeholder="SEARCH TRACK, CAR, SESSION..."
            className="h-12 min-w-0 flex-1 border-0 bg-transparent font-mono text-[11px] text-trace-text outline-none placeholder:text-trace-dim"
          />
        </label>
        <select
          aria-label="Filter sessions by source"
          value={sourceFilter}
          onChange={(event) => setSourceFilter(event.target.value)}
          className="h-12 border-0 border-r border-trace-divider bg-trace-surface px-4 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
        >
          <option value="all">ALL SOURCES</option>
          <option value="replay">REPLAYS</option>
          <option value="native">NATIVE CAPTURE</option>
          <option value="imported">IMPORTED</option>
        </select>
        <select
          aria-label="Sort sessions"
          value={sortOrder}
          onChange={(event) => setSortOrder(event.target.value)}
          className="h-12 border-0 bg-trace-surface px-4 font-mono text-[10px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
        >
          <option value="newest">NEWEST FIRST</option>
          <option value="oldest">OLDEST FIRST</option>
        </select>
      </div>

      <div className="mt-3 border border-trace-divider bg-trace-surface">
        {sessions.length === 0 ? (
          <div className="p-12 text-center font-mono text-xs text-trace-faint">NO RECORDED SESSIONS</div>
        ) : visibleSessions.length === 0 ? (
          <div className="p-12 text-center font-mono text-xs text-trace-faint">NO SESSIONS MATCH THESE FILTERS</div>
        ) : visibleSessions.map((session) => <SessionRow key={session.id} session={session} />)}
      </div>
    </>
  );
}

function SessionRow({ session }: { session: RecordedSessionSummary }) {
  const [expanded, setExpanded] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const exportMenu = useRef<HTMLDivElement>(null);
  const timedLaps = session.laps.filter((lap) => lap.time !== "—");
  const bestLap = timedLaps.slice().sort((left, right) => lapTimeMs(left.time) - lapTimeMs(right.time))[0];

  useEffect(() => {
    if (!exportOpen) return;
    function dismissOnPointerDown(event: PointerEvent) {
      if (!exportMenu.current?.contains(event.target as Node)) setExportOpen(false);
    }
    function dismissOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setExportOpen(false);
    }
    document.addEventListener("pointerdown", dismissOnPointerDown);
    document.addEventListener("keydown", dismissOnEscape);
    return () => {
      document.removeEventListener("pointerdown", dismissOnPointerDown);
      document.removeEventListener("keydown", dismissOnEscape);
    };
  }, [exportOpen]);

  async function exportTelemetry(exportFormat: SessionExportFormat) {
    setExporting(true);
    setExportMessage(null);
    try {
      const result = await telemetryDataSource.exportSession(session.id, exportFormat);
      setExportMessage(`${result.format} · ${result.sampleCount.toLocaleString()} samples · ${result.path}`);
      setExportOpen(false);
    } catch (error) {
      setExportMessage(`EXPORT FAILED · ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setExporting(false);
    }
  }

  return (
    <article className="relative border-b border-trace-divider last:border-b-0">
      <div className="flex min-h-[82px] items-stretch">
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
          className="grid min-w-0 flex-1 grid-cols-[minmax(170px,1.3fr)_minmax(145px,1fr)_100px_120px] items-center gap-5 border-0 bg-transparent px-5 text-left hover:bg-trace-raised max-[1050px]:grid-cols-[minmax(170px,1.3fr)_minmax(130px,1fr)_90px]"
        >
          <div className="min-w-0">
            <span className="block truncate text-[10px] font-extrabold tracking-[.12em] text-trace-accent">{session.sessionType}</span>
            <h2 className="mt-1.5 truncate text-base font-black tracking-[.03em]">{session.track}</h2>
            <span className="mt-1 block truncate font-mono text-[9px] text-trace-dim" title={session.startedAt}>{formatSessionDate(session.startedAt)}</span>
          </div>
          <div className="min-w-0">
            <span className="block truncate text-[12px] text-trace-soft">{session.car}</span>
            <span className="mt-1 block truncate font-mono text-[9px] tracking-[.07em] text-trace-dim">{session.source}</span>
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
          <div className="relative flex" ref={exportMenu}>
            <button
              type="button"
              aria-label={`Export ${session.track} session`}
              aria-expanded={exportOpen}
              disabled={!session.exportable}
              title={session.exportable ? "Export session" : "Session has not finalized"}
              onClick={() => setExportOpen((value) => !value)}
              className="grid w-12 place-items-center border-0 bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-accent disabled:text-trace-dim disabled:hover:bg-transparent"
            >
              <svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
                <path d="M8 2v8m0 0 3-3m-3 3L5 7M3 12.5h10" />
              </svg>
            </button>
            {exportOpen && (
              <div className="absolute right-0 top-[calc(100%-10px)] z-20 w-64 border border-trace-divider bg-trace-black p-2 shadow-[0_12px_30px_#000]">
                <span className="block px-2 pb-2 pt-1 font-mono text-[9px] font-bold tracking-[.1em] text-trace-dim">EXPORT TELEMETRY</span>
                <ExportOption
                  label="ARROW IPC"
                  detail="Full-fidelity TRACE recording"
                  disabled={exporting}
                  onClick={() => void exportTelemetry("arrow")}
                />
                <ExportOption
                  label="CSV"
                  detail="Core channels, broad compatibility"
                  disabled={exporting}
                  onClick={() => void exportTelemetry("csv")}
                />
              </div>
            )}
          </div>
          <button
            type="button"
            aria-label={expanded ? `Collapse ${session.track} laps` : `Show ${session.track} laps`}
            title={expanded ? "Hide laps" : "Show laps"}
            onClick={() => setExpanded((value) => !value)}
            className="grid w-12 place-items-center border-0 border-l border-trace-divider bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-text"
          >
            <svg className={`size-4 fill-none stroke-current transition-transform ${expanded ? "rotate-180" : ""}`} viewBox="0 0 16 16" aria-hidden="true">
              <path d="m4 6 4 4 4-4" />
            </svg>
          </button>
        </div>
      </div>
      {exportMessage && (
        <p className="border-t border-trace-divider bg-trace-deep px-5 py-2 break-all font-mono text-[9px] leading-4 text-trace-faint" role="status">
          {exportMessage}
        </p>
      )}
      {expanded && (
        <div className="border-t border-trace-divider bg-trace-deep px-5 py-4">
          <div className="mb-3 flex items-center justify-between font-mono text-[9px] tracking-[.09em] text-trace-dim">
            <span>LAP DETAILS</span>
            <span title={session.startedAt}>{formatSessionDate(session.startedAt)}</span>
          </div>
          <div className="max-h-56 overflow-y-auto border border-trace-divider bg-trace-surface">
            {session.laps.length === 0 ? (
              <div className="p-5 font-mono text-[10px] text-trace-dim">NO COMPLETED LAP BOUNDARIES</div>
            ) : session.laps.map((lap) => (
              <div className="grid min-h-11 grid-cols-[80px_1fr_100px] items-center border-b border-trace-divider px-4 font-mono text-[10px] last:border-b-0" key={lap.index}>
                <span className="text-trace-faint">LAP {String(lap.index).padStart(2, "0")}</span>
                <strong className={lap.validity === "valid" ? "text-trace-text" : "text-trace-soft"}>{lap.time}</strong>
                <LapValidity lap={lap} />
              </div>
            ))}
          </div>
        </div>
      )}
    </article>
  );
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

function LapValidity({ lap }: { lap: RecordedSessionSummary["laps"][number] }) {
  const label = lap.validity === "unknown"
    ? "UNVERIFIED"
    : lap.validityReason?.includes("partial")
      ? "PARTIAL"
      : lap.validity.toUpperCase();
  return (
    <span
      className={`text-right text-[9px] font-bold tracking-[.08em] ${lap.validity === "valid" ? "text-trace-accent" : "text-trace-warning"}`}
      title={lap.validityReason ?? undefined}
    >
      {label}
    </span>
  );
}

function sessionSourceGroup(session: RecordedSessionSummary) {
  const source = session.source.toLocaleLowerCase();
  if (source.includes("replay")) return "replay";
  if (source.includes("import")) return "imported";
  return "native";
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

function Footer() {
  return (
    <footer className="col-span-full flex items-center gap-6 border-t border-trace-divider bg-trace-black px-[14px] font-mono text-[10px] tracking-[.06em] text-trace-dim">
      <span>TRACE ENGINE <b className="ml-1 text-trace-accent">READY</b></span>
      <span>AC MODULE <b className="ml-1 text-trace-accent">LIFECYCLE</b></span>
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
