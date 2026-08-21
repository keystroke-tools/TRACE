export type ConnectionState = "searching" | "connected" | "replay" | "offline";

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

export interface TelemetryDataSource {
  getStatus(): Promise<TelemetryStatus>;
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
};

export const tauriDataSource: TelemetryDataSource = {
  getStatus() {
    return invoke<TelemetryStatus>("foundation_status");
  },
};

export const telemetryDataSource = isTauri() ? tauriDataSource : fixtureDataSource;
import { invoke, isTauri } from "@tauri-apps/api/core";
