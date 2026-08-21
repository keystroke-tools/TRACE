import { useEffect, useState } from "react";
import { telemetryDataSource, type TelemetryStatus } from "./data-source";

const navigation = ["LIVE", "SESSIONS", "COMPARE", "SETUPS"] as const;

export function App() {
  const [status, setStatus] = useState<TelemetryStatus | null>(null);

  useEffect(() => {
    void telemetryDataSource.getStatus().then(setStatus);
  }, []);

  return (
    <main className="shell">
      <header className="topbar">
        <div className="brand">TRACE<span>//</span></div>
        <div className="context">{status?.session ?? "NO ACTIVE SESSION"}</div>
        <button type="button" className="primary-action" disabled>GO LIVE</button>
      </header>

      <aside className="navigation" aria-label="Primary navigation">
        {navigation.map((item) => (
          <button key={item} type="button" className={item === "LIVE" ? "active" : ""}>
            <span className="nav-index">0{navigation.indexOf(item) + 1}</span>{item}
          </button>
        ))}
      </aside>

      <section className="workspace">
        <div className="section-heading"><span>01</span> SYSTEM STATUS</div>
        <div className="status-grid">
          <Metric label="SOURCE" value={status?.source ?? "INITIALISING"} accent />
          <Metric label="STATE" value={status?.connection.toUpperCase() ?? "WAIT"} />
          <Metric label="SAMPLE RATE" value={status?.sampleRateHz ? `${status.sampleRateHz} HZ` : "—"} />
          <Metric label="BACKEND" value="OFFLINE / LOCAL" />
        </div>

        <div className="panel">
          <div className="panel-title">CHANNEL CAPABILITIES</div>
          <div className="channel-table" role="table" aria-label="Telemetry channels">
            {status?.channels.map((channel) => (
              <div className="channel-row" role="row" key={channel.id}>
                <span role="cell">{channel.label}</span>
                <span role="cell" className={channel.available ? "available" : "unavailable"}>
                  {channel.available ? "AVAILABLE" : "UNAVAILABLE"}
                </span>
                <code role="cell">{channel.id}</code>
              </div>
            ))}
          </div>
        </div>

        <div className="empty-state">
          <span className="crosshair" aria-hidden="true" />
          <div>
            <strong>FOUNDATION MODE</strong>
            <p>Replay data source connected. Live AC acquisition begins in Phase 2.</p>
          </div>
        </div>
      </section>

      <footer className="statusbar">
        <span>TRACE ENGINE <b>READY</b></span>
        <span>AC MODULE <b>MAPPING</b></span>
        <span>STORAGE <b>LOCAL</b></span>
        <span className="build">V0.1.0 / FOUNDATION</span>
      </footer>
    </main>
  );
}

function Metric({ label, value, accent = false }: { label: string; value: string; accent?: boolean }) {
  return <div className="metric"><span>{label}</span><strong className={accent ? "accent" : ""}>{value}</strong></div>;
}
