import { useEffect, useState, type ReactNode } from "react";
import {
  telemetryDataSource,
  type RecordedSessionSummary,
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
        {section === "SESSIONS" ? (
          <Sessions sessions={sessions} />
        ) : (
          <SystemStatus status={status} section={section} />
        )}
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

function SystemStatus({ status, section }: { status: TelemetryStatus | null; section: Section }) {
  return (
    <>
      <SectionHeading index="01">{section === "LIVE" ? "SYSTEM STATUS" : `${section} / LOCKED`}</SectionHeading>
      <div className="my-[14px] mb-6 grid grid-cols-4 border border-trace-divider max-[900px]:grid-cols-2">
        <Metric label="SOURCE" value={status?.source ?? "INITIALISING"} accent />
        <Metric label="STATE" value={status?.connection.toUpperCase() ?? "WAIT"} />
        <Metric label="SAMPLE RATE" value={status?.sampleRateHz ? `${status.sampleRateHz} HZ` : "—"} />
        <Metric label="BACKEND" value="OFFLINE / LOCAL" />
      </div>

      <div className="border border-trace-divider bg-trace-surface">
        <PanelTitle>CHANNEL CAPABILITIES</PanelTitle>
        <div role="table" aria-label="Telemetry channels">
          {status?.channels.map((channel) => (
            <div
              className="grid min-h-[46px] grid-cols-[1fr_150px_1.4fr] items-center border-b border-trace-divider text-xs last:border-b-0 max-[900px]:grid-cols-[1fr_120px]"
              role="row"
              key={channel.id}
            >
              <span className="px-4" role="cell">{channel.label}</span>
              <span
                className={`px-4 font-mono text-[11px] font-bold tracking-[.06em] ${channel.available ? "text-trace-accent" : "text-trace-dim"}`}
                role="cell"
              >
                {channel.available ? "AVAILABLE" : "UNAVAILABLE"}
              </span>
              <code className="px-4 text-[11px] text-trace-faint max-[900px]:hidden" role="cell">{channel.id}</code>
            </div>
          ))}
        </div>
      </div>

      <div className="flex min-h-[190px] items-center justify-center gap-6 border border-t-0 border-trace-divider bg-trace-deep">
        <span className="trace-crosshair" aria-hidden="true" />
        <div>
          <strong className="font-mono text-xs tracking-[.14em]">PHASE 2 / AC CAPTURE</strong>
          <p className="mt-2 text-[13px] text-trace-faint">Local capture worker active. Completed sessions persist without a network dependency.</p>
        </div>
      </div>
    </>
  );
}

function Sessions({ sessions }: { sessions: RecordedSessionSummary[] }) {
  return (
    <>
      <div className="flex items-end justify-between">
        <div>
          <SectionHeading index="02">RECORDED SESSIONS</SectionHeading>
          <h1 className="mt-3 text-2xl font-black tracking-[-.02em]">LOCAL TELEMETRY ARCHIVE</h1>
        </div>
        <span className="font-mono text-[11px] tracking-[.1em] text-trace-faint">{sessions.length} SESSION(S)</span>
      </div>

      <div className="mt-6 border border-trace-divider bg-trace-surface">
        {sessions.length === 0 ? (
          <div className="p-12 text-center font-mono text-xs text-trace-faint">NO RECORDED SESSIONS</div>
        ) : sessions.map((session) => <SessionRow key={session.id} session={session} />)}
      </div>
    </>
  );
}

function SessionRow({ session }: { session: RecordedSessionSummary }) {
  return (
    <article className="grid grid-cols-[1.4fr_1fr] border-b border-trace-divider last:border-b-0 max-[900px]:grid-cols-1">
      <div className="border-r border-trace-divider p-5 max-[900px]:border-b max-[900px]:border-r-0">
        <div className="flex items-start justify-between gap-4">
          <div>
            <span className="text-[11px] font-extrabold tracking-[.12em] text-trace-accent">{session.sessionType}</span>
            <h2 className="mt-2 text-lg font-black tracking-[.04em]">{session.track}</h2>
            <p className="mt-1 text-[13px] text-trace-muted">{session.car}</p>
          </div>
          <div className="text-right font-mono text-[11px] leading-5 text-trace-faint">
            <div>{session.startedAt}</div>
            <div>{session.source}</div>
          </div>
        </div>
      </div>
      <div className="divide-y divide-trace-divider">
        {session.laps.map((lap) => (
          <div className="grid min-h-12 grid-cols-[72px_1fr_80px] items-center px-4 font-mono text-[11px]" key={lap.index}>
            <span className="text-trace-faint">LAP {String(lap.index).padStart(2, "0")}</span>
            <strong className={lap.validity === "valid" ? "text-trace-text" : "text-trace-dim"}>{lap.time}</strong>
            <span
              className={`text-right text-[10px] font-bold tracking-[.08em] ${lap.validity === "valid" ? "text-trace-accent" : "text-trace-warning"}`}
              title={lap.validityReason ?? undefined}
            >
              {lap.validity === "unknown"
                ? "UNVERIFIED"
                : lap.validityReason?.includes("partial")
                  ? "PARTIAL"
                  : lap.validity.toUpperCase()}
            </span>
          </div>
        ))}
      </div>
    </article>
  );
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
