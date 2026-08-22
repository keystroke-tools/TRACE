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
  category: string;
  detail: string;
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
  exportable: boolean;
  deletable: boolean;
  laps: RecordedLapSummary[];
}

export type SessionExportFormat = "arrow" | "csv";

export interface SessionExport {
  path: string;
  format: string;
  sampleCount: number;
}

export interface SessionDeletion {
  sessionId: string;
  cleanupWarning?: string | null;
}

export interface TelemetryDataSource {
  getStatus(): Promise<TelemetryStatus>;
  getSessions(): Promise<RecordedSessionSummary[]>;
  exportSession(sessionId: string, format: SessionExportFormat): Promise<SessionExport>;
  deleteSession(sessionId: string): Promise<SessionDeletion>;
}

const deletedFixtureSessionIds = new Set<string>();

export const fixtureDataSource: TelemetryDataSource = {
  async getStatus() {
    return {
      connection: "replay",
      source: "TRACE REPLAY",
      sampleRateHz: 100,
      session: "MUGELLO / TATUUS FA01",
      channels: [
        { id: "inputs.throttle", label: "Throttle", category: "DRIVER INPUTS", detail: "Pedal position", available: true },
        { id: "inputs.brake", label: "Brake", category: "DRIVER INPUTS", detail: "Pedal position", available: true },
        { id: "vehicle.speed", label: "Speed", category: "VEHICLE", detail: "Metres per second", available: true },
        { id: "vehicle.engine_rpm", label: "Engine RPM", category: "VEHICLE", detail: "Revolutions per minute", available: true },
        { id: "vehicle.gear", label: "Gear", category: "VEHICLE", detail: "Reverse, neutral, or forward gear", available: true },
        { id: "vehicle.fuel", label: "Fuel", category: "VEHICLE", detail: "Litres remaining", available: true },
        { id: "lap.position", label: "Lap position", category: "LAP PROGRESS", detail: "Normalized track position", available: true },
        { id: "lap.current_time", label: "Current lap time", category: "LAP PROGRESS", detail: "Simulator timer", available: true },
        { id: "environment.air_temperature", label: "Air temperature", category: "CONDITIONS", detail: "Degrees Celsius", available: true },
        { id: "environment.track_temperature", label: "Track temperature", category: "CONDITIONS", detail: "Degrees Celsius", available: true },
        { id: "motion.position", label: "World position", category: "MOTION", detail: "Three-axis source-world coordinates", available: true },
        { id: "motion.velocity", label: "Velocity", category: "MOTION", detail: "Three-axis metres per second", available: true },
        { id: "motion.acceleration", label: "Acceleration", category: "MOTION", detail: "Three-axis metres per second squared", available: true },
        { id: "wheels.tyre_core_temperature", label: "Tyre core temperature", category: "WHEELS", detail: "Degrees Celsius at all four corners", available: true },
        { id: "wheels.suspension_travel", label: "Suspension travel", category: "WHEELS", detail: "Metres at all four corners", available: true },
        { id: "inputs.steering", label: "Steering angle", category: "NEEDS VALIDATION", detail: "AC does not document a reliable unit or sign", available: false },
        { id: "tyres.extended", label: "Extended tyre data", category: "NEEDS VALIDATION", detail: "Pressure, slip, load, and wear need fixture validation", available: false },
      ],
    };
  },
  async getSessions() {
    const sessions: RecordedSessionSummary[] = [
      {
        id: "replay-mugello-001",
        track: "MUGELLO",
        car: "TATUUS FA01",
        sessionType: "REPLAY FIXTURE",
        startedAt: "21 AUG / 14:32",
        source: "TRACE REPLAY",
        exportable: true,
        deletable: true,
        laps: [
          { index: 1, time: "1:52.418", validity: "valid" },
          { index: 2, time: "1:50.906", validity: "valid" },
          { index: 3, time: "—", validity: "unknown" },
        ],
      },
    ];
    return sessions.filter((session) => !deletedFixtureSessionIds.has(session.id));
  },
  async exportSession(_sessionId, format) {
    return {
      path: `Browser preview (${format.toUpperCase()})`,
      format: format === "arrow" ? "Arrow IPC" : "CSV",
      sampleCount: 0,
    };
  },
  async deleteSession(sessionId) {
    deletedFixtureSessionIds.add(sessionId);
    return { sessionId };
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
  deleteSession(sessionId) {
    return invoke<SessionDeletion>("delete_session", { sessionId });
  },
};

export const telemetryDataSource = isTauri() ? tauriDataSource : fixtureDataSource;
import { invoke, isTauri } from "@tauri-apps/api/core";
