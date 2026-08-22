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

export interface SimulatorOption {
  id: string;
  name: string;
  shortName: string;
  available: boolean;
}

export interface TelemetryStatus {
  simulatorId: string;
  simulatorName: string;
  simulatorShortName: string;
  simulators: SimulatorOption[];
  connection: ConnectionState;
  source: string;
  sampleRateHz: number | null;
  session: string | null;
  channels: ChannelCapability[];
}

export interface RecordedLapSummary {
  index: number;
  time: string;
  durationNs?: number | null;
  validity: "valid" | "invalid" | "unknown";
  validityReason?: string | null;
  isFastest?: boolean;
  sectors: RecordedSectorSummary[];
}

export interface RecordedSectorSummary {
  index: number;
  time: string;
  durationNs: number;
}

export interface RecordedSessionSummary {
  id: string;
  simulatorId: string;
  simulatorName: string;
  title?: string | null;
  driver?: string | null;
  ownership: "mine" | "other" | "unknown";
  tags: string[];
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
  selectSimulator(simulatorId: string): Promise<void>;
  getSessions(): Promise<RecordedSessionSummary[]>;
  exportSession(sessionId: string, format: SessionExportFormat): Promise<SessionExport>;
  deleteSession(sessionId: string): Promise<SessionDeletion>;
  updateSessionDetails(sessionId: string, title: string | null, driver: string | null, ownership: RecordedSessionSummary["ownership"], tags: string[]): Promise<void>;
}

const deletedFixtureSessionIds = new Set<string>();
const fixtureSessionDetails = new Map<string, { title: string | null; driver: string | null; ownership: RecordedSessionSummary["ownership"]; tags: string[] }>();

export const fixtureDataSource: TelemetryDataSource = {
  async getStatus() {
    return {
      simulatorId: "assetto-corsa",
      simulatorName: "Assetto Corsa",
      simulatorShortName: "AC",
      simulators: [{ id: "assetto-corsa", name: "Assetto Corsa", shortName: "AC", available: true }],
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
        { id: "native.inputs", label: "Clutch & steering source values", category: "AC-NATIVE · INPUTS", detail: "Exact AC source fields", available: true },
        { id: "native.tyres.dynamics", label: "Slip, load, pressure & angular speed", category: "AC-NATIVE · TYRES & WHEELS", detail: "All four corners", available: true },
        { id: "native.tyres.condition", label: "Wear, dirt, camber & core temperature", category: "AC-NATIVE · TYRES & WHEELS", detail: "All four corners", available: true },
        { id: "native.tyres.temperatures", label: "Inner, middle & outer temperatures", category: "AC-NATIVE · TYRES & WHEELS", detail: "Includes brake temperatures", available: true },
        { id: "native.tyres.contact", label: "Contact points, normals & headings", category: "AC-NATIVE · TYRES & WHEELS", detail: "Contact geometry", available: true },
        { id: "native.powertrain.electronics", label: "TC, ABS, DRS, KERS & ERS", category: "AC-NATIVE · POWERTRAIN", detail: "States and settings", available: true },
        { id: "native.powertrain.engine", label: "Turbo, engine brake & air density", category: "AC-NATIVE · POWERTRAIN", detail: "Dynamic and static limits", available: true },
        { id: "native.chassis.orientation", label: "Heading, pitch, roll & angular velocity", category: "AC-NATIVE · CHASSIS", detail: "Orientation and motion", available: true },
        { id: "native.chassis.state", label: "Ride height, damage, ballast & brake bias", category: "AC-NATIVE · CHASSIS", detail: "Chassis state", available: true },
        { id: "native.chassis.controls", label: "Pit limiter, tyres out, auto shift & FFB", category: "AC-NATIVE · CHASSIS", detail: "Control state", available: true },
        { id: "native.session.timing", label: "Last/best laps, splits & session time", category: "AC-NATIVE · SESSION", detail: "Complete timing state", available: true },
        { id: "native.session.race_control", label: "Flags, pits, penalties & mandatory stop", category: "AC-NATIVE · SESSION", detail: "Race-control state", available: true },
        { id: "native.session.conditions", label: "Grip, wind & replay speed", category: "AC-NATIVE · SESSION", detail: "Conditions and compound", available: true },
        { id: "native.static.identities", label: "Car, track, layout & skin IDs", category: "AC-NATIVE · CAR & TRACK", detail: "Static identity fields", available: true },
        { id: "native.static.limits", label: "Car limits & track length", category: "AC-NATIVE · CAR & TRACK", detail: "Vehicle and circuit limits", available: true },
        { id: "native.static.configuration", label: "Assists, rates & pit window", category: "AC-NATIVE · CAR & TRACK", detail: "Session configuration", available: true },
      ],
    };
  },
  async selectSimulator(simulatorId) {
    if (simulatorId !== "assetto-corsa") throw new Error("That simulator adapter is not installed.");
  },
  async getSessions() {
    const sessions: RecordedSessionSummary[] = [
      {
        id: "replay-mugello-001",
        simulatorId: "assetto-corsa",
        simulatorName: "Assetto Corsa",
        title: null,
        driver: null,
        ownership: "unknown",
        tags: [],
        track: "MUGELLO",
        car: "TATUUS FA01",
        sessionType: "REPLAY FIXTURE",
        startedAt: "21 AUG / 14:32",
        source: "TRACE REPLAY",
        exportable: true,
        deletable: true,
        laps: [
          { index: 1, time: "1:52.418", durationNs: 112_418_000_000, validity: "valid", sectors: [
            { index: 1, time: "0:37.518", durationNs: 37_518_000_000 },
            { index: 2, time: "0:38.406", durationNs: 38_406_000_000 },
            { index: 3, time: "0:36.494", durationNs: 36_494_000_000 },
          ] },
          { index: 2, time: "1:50.906", durationNs: 110_906_000_000, validity: "valid", isFastest: true, sectors: [
            { index: 1, time: "0:36.901", durationNs: 36_901_000_000 },
            { index: 2, time: "0:37.802", durationNs: 37_802_000_000 },
            { index: 3, time: "0:36.203", durationNs: 36_203_000_000 },
          ] },
          { index: 3, time: "—", validity: "unknown", sectors: [] },
        ],
      },
    ];
    return sessions
      .filter((session) => !deletedFixtureSessionIds.has(session.id))
      .map((session) => ({ ...session, ...fixtureSessionDetails.get(session.id) }));
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
  async updateSessionDetails(sessionId, title, driver, ownership, tags) {
    fixtureSessionDetails.set(sessionId, { title, driver, ownership, tags });
  },
};

export const tauriDataSource: TelemetryDataSource = {
  getStatus() {
    return invoke<TelemetryStatus>("foundation_status");
  },
  selectSimulator(simulatorId) {
    return invoke<void>("select_simulator", { simulatorId });
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
  updateSessionDetails(sessionId, title, driver, ownership, tags) {
    return invoke<void>("update_session_details", { sessionId, title, driver, ownership, tags });
  },
};

export const telemetryDataSource = isTauri() ? tauriDataSource : fixtureDataSource;
import { invoke, isTauri } from "@tauri-apps/api/core";
