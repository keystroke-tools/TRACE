export type ConnectionState =
  | "waiting"
  | "recording"
  | "error"
  | "searching"
  | "connected"
  | "replay"
  | "offline";

export interface ChannelCapability {
  id: string;
  label: string;
  available: boolean;
}

export interface TelemetryStatus {
  connection: ConnectionState;
  source: string;
  sampleRateHz: number | null;
  session: string | null;
  channels: ChannelCapability[];
}

export interface RecordedLapSummary {
  index: number;
  time: string;
  validity: "valid" | "invalid" | "unknown";
  validityReason?: string | null;
}

export interface RecordedSessionSummary {
  id: string;
  track: string;
  car: string;
  sessionType: string;
  startedAt: string;
  source: string;
  laps: RecordedLapSummary[];
}

export type SessionExportFormat = "arrow" | "csv";

export interface SessionExport {
  path: string;
  format: string;
  sampleCount: number;
}

export interface TelemetryDataSource {
  getStatus(): Promise<TelemetryStatus>;
  getSessions(): Promise<RecordedSessionSummary[]>;
  exportSession(sessionId: string, format: SessionExportFormat): Promise<SessionExport>;
}

export const fixtureDataSource: TelemetryDataSource = {
  async getStatus() {
    return {
      connection: "replay",
      source: "TRACE REPLAY",
      sampleRateHz: 100,
      session: "MUGELLO / TATUUS FA01",
      channels: [
        { id: "vehicle.speed", label: "SPEED", available: true },
        { id: "inputs.throttle", label: "THROTTLE", available: true },
        { id: "inputs.brake", label: "BRAKE", available: true },
        { id: "inputs.steering", label: "STEERING", available: false },
        { id: "tyres.brake_temperature", label: "BRAKE TEMP", available: false },
      ],
    };
  },
  async getSessions() {
    return [
      {
        id: "replay-mugello-001",
        track: "MUGELLO",
        car: "TATUUS FA01",
        sessionType: "REPLAY FIXTURE",
        startedAt: "21 AUG / 14:32",
        source: "TRACE REPLAY",
        laps: [
          { index: 1, time: "1:52.418", validity: "valid" },
          { index: 2, time: "1:50.906", validity: "valid" },
          { index: 3, time: "—", validity: "unknown" },
        ],
      },
    ];
  },
  async exportSession(_sessionId, format) {
    return {
      path: `Browser preview (${format.toUpperCase()})`,
      format: format === "arrow" ? "Arrow IPC" : "CSV",
      sampleCount: 0,
    };
  },
};

export const tauriDataSource: TelemetryDataSource = {
  getStatus() {
    return invoke<TelemetryStatus>("foundation_status");
  },
  getSessions() {
    return invoke<RecordedSessionSummary[]>("recent_sessions");
  },
  exportSession(sessionId, exportFormat) {
    return invoke<SessionExport>("export_session", { sessionId, exportFormat });
  },
};

export const telemetryDataSource = isTauri() ? tauriDataSource : fixtureDataSource;
import { invoke, isTauri } from "@tauri-apps/api/core";
