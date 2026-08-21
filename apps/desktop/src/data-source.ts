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

export interface TelemetryDataSource {
  getStatus(): Promise<TelemetryStatus>;
  getSessions(): Promise<RecordedSessionSummary[]>;
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
};

export const tauriDataSource: TelemetryDataSource = {
  getStatus() {
    return invoke<TelemetryStatus>("foundation_status");
  },
  getSessions() {
    return invoke<RecordedSessionSummary[]>("recent_sessions");
  },
};

export const telemetryDataSource = isTauri() ? tauriDataSource : fixtureDataSource;
import { invoke, isTauri } from "@tauri-apps/api/core";
