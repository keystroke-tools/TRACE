export type ConnectionState = "waiting" | "recording" | "error" | "searching" | "connected" | "replay" | "offline";

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
	completedSessionId?: string | null;
	channels: ChannelCapability[];
}

export interface LivePedalTelemetry {
	connection: ConnectionState;
	simulatorName: string;
	session: string;
	sequence: number;
	throttlePercent?: number | null;
	brakePercent?: number | null;
	clutchPercent?: number | null;
	steeringDegrees?: number | null;
}

export interface RecordedLapSummary {
	index: number;
	time: string;
	durationNs?: number | null;
	validity: "valid" | "invalid" | "unknown";
	validityReason?: string | null;
	maxTyresOut?: number | null;
	isFastest?: boolean;
	sectors: RecordedSectorSummary[];
}

export interface RecordedSectorSummary {
	index: number;
	time: string;
	durationNs: number;
}

export interface RecordedLapMetrics {
	lapIndex: number;
	fuelStartLitres?: number | null;
	fuelEndLitres?: number | null;
	fuelUsedLitres?: number | null;
	fuelCapacityLitres?: number | null;
	maxSpeedKmh?: number | null;
	tyreWearStart: Array<number | null>;
	tyreWearEnd: Array<number | null>;
	tyreWearMinimum: Array<number | null>;
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
	ambientTemperatureC?: string | null;
	roadTemperatureC?: string | null;
	weatherName?: string | null;
	trackGripPercent?: number | null;
	exportable: boolean;
	deletable: boolean;
	laps: RecordedLapSummary[];
}

export interface LapComparisonSample {
	distanceM: number;
	deltaSeconds?: number | null;
	referenceElapsedSeconds?: number | null;
	comparisonElapsedSeconds?: number | null;
	referenceSpeedKmh?: number | null;
	comparisonSpeedKmh?: number | null;
	referenceThrottlePercent?: number | null;
	comparisonThrottlePercent?: number | null;
	referenceBrakePercent?: number | null;
	comparisonBrakePercent?: number | null;
	referenceSteeringDegrees?: number | null;
	comparisonSteeringDegrees?: number | null;
	referenceRpm?: number | null;
	comparisonRpm?: number | null;
	sectorIndex?: number | null;
	referenceGear?: number | null;
	comparisonGear?: number | null;
	referencePositionXM?: number | null;
	referencePositionZM?: number | null;
	comparisonPositionXM?: number | null;
	comparisonPositionZM?: number | null;
	referenceAirTemperatureC?: number | null;
	referenceTrackTemperatureC?: number | null;
	comparisonAirTemperatureC?: number | null;
	comparisonTrackTemperatureC?: number | null;
}

export type CornerPhase = "entry" | "mid" | "exit";

export interface CornerPhaseAnalysis {
	phase: CornerPhase;
	startDistanceM: number;
	endDistanceM: number;
	lossSeconds?: number | null;
}

export interface CornerMetrics {
	referenceBrakingPointM?: number | null;
	comparisonBrakingPointM?: number | null;
	referenceBrakeReleasePointM?: number | null;
	comparisonBrakeReleasePointM?: number | null;
	referencePeakBrakePercent?: number | null;
	comparisonPeakBrakePercent?: number | null;
	referenceMinimumSpeedKmh?: number | null;
	comparisonMinimumSpeedKmh?: number | null;
	referenceThrottlePointM?: number | null;
	comparisonThrottlePointM?: number | null;
}

export interface CornerAnalysis {
	index: number;
	label: string;
	startDistanceM: number;
	apexDistanceM: number;
	endDistanceM: number;
	totalLossSeconds?: number | null;
	phases: CornerPhaseAnalysis[];
	metrics: CornerMetrics;
}

export interface CornerComparisonAnalysis {
	corners: CornerAnalysis[];
}

export type DrivingObservationKind =
	| "braking_earlier"
	| "braking_later"
	| "brake_release_earlier"
	| "brake_release_later"
	| "lower_minimum_speed"
	| "later_throttle"
	| "entry_loss"
	| "mid_corner_loss"
	| "exit_loss"
	| "more_steering_corrections";

export interface DrivingObservation {
	kind: DrivingObservationKind;
	tier: "high" | "low";
	confidence: number;
	cornerIndices: number[];
	eligibleCornerCount: number;
	meanDifference: number;
	unit: "metre" | "kilometres_per_hour" | "second" | "count";
}

export interface DrivingAnalysis {
	observations: DrivingObservation[];
}

export interface StructuredAnalysisResult<T> {
	availability: unknown;
	value?: T | null;
	confidence: number;
}

export interface LapTraceSample {
	distanceM: number;
	elapsedSeconds?: number | null;
	sectorIndex?: number | null;
	speedKmh?: number | null;
	throttlePercent?: number | null;
	brakePercent?: number | null;
	clutchPercent?: number | null;
	steeringDegrees?: number | null;
	rpm?: number | null;
	gear?: number | null;
	positionXM?: number | null;
	positionZM?: number | null;
	airTemperatureC?: number | null;
	trackTemperatureC?: number | null;
}

export interface TrackMapAsset {
	centreLine: TrackMapPoint[];
	leftBoundary: TrackMapPoint[];
	rightBoundary: TrackMapPoint[];
}

export interface TrackMapPoint {
	xM: number;
	zM: number;
}

export interface LapTrace {
	sessionId: string;
	simulatorId: string;
	sourceTrackId?: string | null;
	layoutId?: string | null;
	sourceCarId?: string | null;
	lapIndex: number;
	lapTime: string;
	track: string;
	car: string;
	lapLengthM: number;
	trackMap?: TrackMapAsset | null;
	samples: LapTraceSample[];
}

export interface TracerReferenceStatus {
	installed: boolean;
	installPath: string;
	referencePath: string;
	sessionId: string;
	lapIndex: number;
	lapTime: string;
	brakeZoneCount: number;
}

export interface LapComparison {
	referenceSessionId: string;
	referenceSessionTitle?: string | null;
	referenceTrack: string;
	referenceCar: string;
	comparisonSessionId: string;
	comparisonSessionTitle?: string | null;
	comparisonTrack: string;
	comparisonCar: string;
	trackMap?: TrackMapAsset | null;
	referenceLapIndex: number;
	referenceLapTime: string;
	comparisonLapIndex: number;
	comparisonLapTime: string;
	lapLengthM: number;
	cornerAnalysis: StructuredAnalysisResult<CornerComparisonAnalysis>;
	drivingAnalysis: StructuredAnalysisResult<DrivingAnalysis>;
	samples: LapComparisonSample[];
}

export type SessionExportFormat = "trace" | "arrow" | "csv";

export interface SessionExport {
	path: string;
	format: string;
	sampleCount: number;
}

export interface SessionDeletion {
	sessionId: string;
	cleanupWarning?: string | null;
}

export interface SessionImport {
	sessionId: string;
	lapCount: number;
	sampleCount: number;
	setupName?: string | null;
}

export interface GameInstallDirectory {
	simulatorId: string;
	simulatorName: string;
	path?: string | null;
	source: "manual" | "detected" | "missing";
}

export interface SetupFolder {
	path?: string | null;
	found: boolean;
	source: "detected" | "default";
}

export interface SetupImporterDescriptor {
	simulatorId: string;
	simulatorName: string;
	archiveLabel: string;
	archiveExtensions: string[];
	folderLabel: string;
	folderHint: string;
	archiveHint: string;
	fileLabel: string;
	fileExtensions: string[];
	fileHint: string;
}

export interface SetupFileImportOptions {
	simulatorId: string;
	setupPaths: string[];
	setupsFolder: string;
	sourceCarId?: string | null;
	sourceTrackId: string;
	layoutId?: string | null;
	overwrite: boolean;
}

export interface SessionSetupAttachmentOptions {
	sessionId: string;
	setupPath: string;
	setupsFolder: string;
	overwrite: boolean;
}

export interface SetupDiscoveryResult {
	indexed: number;
	ignored: number;
	errors: string[];
	limited: boolean;
}

export interface SetupLibraryEntry {
	id: string;
	simulatorId: string;
	simulatorName: string;
	sourceCarId: string;
	carName: string;
	sourceTrackId: string;
	trackName: string;
	layoutId?: string | null;
	name: string;
	installedPath: string;
	sourceArchive?: string | null;
	importedAt: string;
	linkedSessionCount: number;
	available: boolean;
}

export interface SetupDocumentValue {
	section: string;
	label: string;
	value: string;
	editable: boolean;
	description?: string | null;
	minimum?: string | null;
	maximum?: string | null;
	step?: string | null;
}

export interface SetupDocumentGroup {
	name: string;
	values: SetupDocumentValue[];
}

export interface SetupDocument {
	setupId: string;
	name: string;
	simulatorId: string;
	sourceCarId: string;
	metadataAvailable: boolean;
	groups: SetupDocumentGroup[];
}

export interface SaveSetupCopyOptions {
	sourceSetupId: string;
	name: string;
	values: { section: string; value: string }[];
}

export interface SetupSaveResult {
	setupId: string;
	name: string;
}

export interface SetupImportResult {
	archiveName: string;
	car?: string | null;
	track?: string | null;
	files: string[];
	destination?: string | null;
	skipped: string[];
	error?: string | null;
	indexWarning?: string | null;
	success: boolean;
}

export interface CompatibleSetup {
	id: string;
	name: string;
	installedPath: string;
	sourceArchive?: string | null;
	importedAt: string;
	confirmed: boolean;
	confirmedAt?: string | null;
	confirmationSource?: "user_confirmed" | "package_confirmed" | null;
}

export interface SetupValueChange {
	key: string;
	baselineValue?: string | null;
	alternativeValue?: string | null;
}

export interface SetupComparisonSection {
	name: string;
	changes: SetupValueChange[];
}

export interface SetupComparison {
	baselineName: string;
	alternativeName: string;
	changedValues: number;
	unchangedValues: number;
	sections: SetupComparisonSection[];
}

export interface DriverProfile {
	name?: string | null;
}

export interface StartupSettings {
	supported: boolean;
	enabled: boolean;
}

export interface AppBehaviorSettings {
	closeToTrayEnabled: boolean;
}

export interface LiveSettings {
	endpoint: string;
	autoStream: LiveAutomationSettings;
	discordActivityEnabled: boolean;
}

export interface DiscordReviewActivity {
	kind: "sessions" | "session" | "lap" | "comparison";
	simulator?: string | null;
	track?: string | null;
	car?: string | null;
	sessionType?: string | null;
	lapIndex?: number | null;
}

export interface LiveAutomationSettings {
	enabled: boolean;
	mode: LiveBroadcastOptions["mode"];
	localPort?: number | null;
	simulatorSessionTypes: Record<string, string[]>;
}

export type LiveBroadcastPhase = "idle" | "connecting" | "reconnecting" | "live" | "ending" | "ended" | "error";

export interface LiveBroadcastStatus {
	phase: LiveBroadcastPhase;
	sourceSessionId?: string | null;
	liveSessionId?: string | null;
	spectatorUrl?: string | null;
	elapsedNs: number;
	durationNs: number;
	error?: string | null;
	automatic: boolean;
}

export interface LiveBroadcastOptions {
	mode: "hosted" | "local";
	localPort?: number;
}

export interface SavedComparison {
	id: string;
	name: string;
	referenceSessionId: string;
	referenceLapIndex: number;
	referenceDurationNs: number;
	referenceStartedAt: string;
	analysedSessionId: string;
	analysedLapIndex: number;
	analysedDurationNs: number;
	analysedStartedAt: string;
	simulatorKey: string;
	track: string;
	car: string;
	createdAt: string;
}

export interface TelemetryDataSource {
	getStatus(): Promise<TelemetryStatus>;
	getLivePedalTelemetry(): Promise<LivePedalTelemetry>;
	selectSimulator(simulatorId: string): Promise<void>;
	getSessions(): Promise<RecordedSessionSummary[]>;
	getSessionLapMetrics(sessionId: string): Promise<RecordedLapMetrics[]>;
	visualizeSessionLap(sessionId: string, lapIndex: number): Promise<LapTrace>;
	prepareTracerReference(sessionId: string, lapIndex: number): Promise<TracerReferenceStatus>;
	compareSessionLaps(referenceSessionId: string, referenceLapIndex: number, comparisonSessionId: string, comparisonLapIndex: number): Promise<LapComparison>;
	getGameInstallDirectories(): Promise<GameInstallDirectory[]>;
	setGameInstallDirectory(simulatorId: string, customPath: string | null): Promise<GameInstallDirectory>;
	getSetupImporters(): Promise<SetupImporterDescriptor[]>;
	detectSetupFolder(simulatorId: string): Promise<SetupFolder>;
	importSetupArchives(simulatorId: string, archivePaths: string[], setupsFolder: string, overwrite: boolean): Promise<SetupImportResult[]>;
	importSetupFiles(options: SetupFileImportOptions): Promise<SetupImportResult[]>;
	attachSessionSetup(options: SessionSetupAttachmentOptions): Promise<CompatibleSetup[]>;
	indexExistingSetups(simulatorId: string, setupsFolder: string): Promise<SetupDiscoveryResult>;
	getSetupLibrary(): Promise<SetupLibraryEntry[]>;
	getSetupDocument(setupId: string): Promise<SetupDocument>;
	saveSetupCopy(options: SaveSetupCopyOptions): Promise<SetupSaveResult>;
	getCompatibleSetups(sessionId: string): Promise<CompatibleSetup[]>;
	confirmSessionSetup(sessionId: string, setupId: string): Promise<CompatibleSetup[]>;
	clearSessionSetup(sessionId: string): Promise<CompatibleSetup[]>;
	compareSetups(baselineSetupId: string, alternativeSetupId: string): Promise<SetupComparison>;
	getDriverProfile(): Promise<DriverProfile>;
	setDriverProfile(name: string | null): Promise<DriverProfile>;
	getStartupSettings(): Promise<StartupSettings>;
	setLaunchOnStartup(enabled: boolean): Promise<StartupSettings>;
	getAppBehaviorSettings(): Promise<AppBehaviorSettings>;
	setAppBehaviorSettings(settings: AppBehaviorSettings): Promise<AppBehaviorSettings>;
	getLiveSettings(): Promise<LiveSettings>;
	setLiveSettings(settings: LiveSettings): Promise<LiveSettings>;
	setDiscordReviewActivity(activity: DiscordReviewActivity | null): Promise<void>;
	getLiveBroadcastStatus(): Promise<LiveBroadcastStatus>;
	startActiveLiveBroadcast(options: LiveBroadcastOptions): Promise<LiveBroadcastStatus>;
	startRecordedLiveBroadcast(sessionId: string, options: LiveBroadcastOptions): Promise<LiveBroadcastStatus>;
	stopLiveBroadcast(): Promise<LiveBroadcastStatus>;
	getSavedComparisons(): Promise<SavedComparison[]>;
	saveComparison(
		name: string,
		referenceSessionId: string,
		referenceLapIndex: number,
		analysedSessionId: string,
		analysedLapIndex: number,
	): Promise<SavedComparison[]>;
	deleteSavedComparison(comparisonId: string): Promise<SavedComparison[]>;
	renameSavedComparison(comparisonId: string, name: string): Promise<SavedComparison[]>;
	exportSession(sessionId: string, format: SessionExportFormat): Promise<SessionExport>;
	importSession(path: string): Promise<SessionImport>;
	deleteSession(sessionId: string): Promise<SessionDeletion>;
	updateSessionDetails(
		sessionId: string,
		title: string | null,
		driver: string | null,
		ownership: RecordedSessionSummary["ownership"],
		tags: string[],
	): Promise<void>;
}

const deletedFixtureSessionIds = new Set<string>();
const fixtureSessionDetails = new Map<
	string,
	{ title: string | null; driver: string | null; ownership: RecordedSessionSummary["ownership"]; tags: string[] }
>();
let fixtureDriverProfile: DriverProfile = { name: null };
let fixtureStartupSettings: StartupSettings = { supported: true, enabled: false };
let fixtureAppBehaviorSettings: AppBehaviorSettings = { closeToTrayEnabled: false };
let fixtureLiveSettings: LiveSettings = {
	endpoint: "https://live.simtrace.run",
	discordActivityEnabled: false,
	autoStream: {
		enabled: false,
		mode: "hosted",
		localPort: null,
		simulatorSessionTypes: {
			"assetto-corsa": ["practice", "qualifying", "race", "hotlap", "time attack", "drift", "drag"],
		},
	},
};
let fixtureLiveBroadcastStatus: LiveBroadcastStatus = {
	phase: "idle",
	elapsedNs: 0,
	durationNs: 0,
	automatic: false,
};
let fixtureSavedComparisons: SavedComparison[] = [];

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
				{ id: "inputs.clutch", label: "Clutch", category: "DRIVER INPUTS", detail: "Pedal position", available: true },
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
				{
					id: "wheels.tyre_core_temperature",
					label: "Tyre core temperature",
					category: "WHEELS",
					detail: "Degrees Celsius at all four corners",
					available: true,
				},
				{ id: "wheels.suspension_travel", label: "Suspension travel", category: "WHEELS", detail: "Metres at all four corners", available: true },
				{
					id: "native.inputs",
					label: "Signed steering source value",
					category: "AC-NATIVE · INPUTS",
					detail: "Exact AC source steering field",
					available: true,
				},
				{
					id: "native.tyres.dynamics",
					label: "Slip, load, pressure & angular speed",
					category: "AC-NATIVE · TYRES & WHEELS",
					detail: "All four corners",
					available: true,
				},
				{
					id: "native.tyres.condition",
					label: "Wear, dirt, camber & core temperature",
					category: "AC-NATIVE · TYRES & WHEELS",
					detail: "All four corners",
					available: true,
				},
				{
					id: "native.tyres.temperatures",
					label: "Inner, middle & outer temperatures",
					category: "AC-NATIVE · TYRES & WHEELS",
					detail: "Includes brake temperatures",
					available: true,
				},
				{
					id: "native.tyres.contact",
					label: "Contact points, normals & headings",
					category: "AC-NATIVE · TYRES & WHEELS",
					detail: "Contact geometry",
					available: true,
				},
				{
					id: "native.powertrain.electronics",
					label: "TC, ABS, DRS, KERS & ERS",
					category: "AC-NATIVE · POWERTRAIN",
					detail: "States and settings",
					available: true,
				},
				{
					id: "native.powertrain.engine",
					label: "Turbo, engine brake & air density",
					category: "AC-NATIVE · POWERTRAIN",
					detail: "Dynamic and static limits",
					available: true,
				},
				{
					id: "native.chassis.orientation",
					label: "Heading, pitch, roll & angular velocity",
					category: "AC-NATIVE · CHASSIS",
					detail: "Orientation and motion",
					available: true,
				},
				{
					id: "native.chassis.state",
					label: "Ride height, damage, ballast & brake bias",
					category: "AC-NATIVE · CHASSIS",
					detail: "Chassis state",
					available: true,
				},
				{
					id: "native.chassis.controls",
					label: "Pit limiter, tyres out, auto shift & FFB",
					category: "AC-NATIVE · CHASSIS",
					detail: "Control state",
					available: true,
				},
				{
					id: "native.session.timing",
					label: "Last/best laps, splits & session time",
					category: "AC-NATIVE · SESSION",
					detail: "Complete timing state",
					available: true,
				},
				{
					id: "native.session.race_control",
					label: "Flags, pits, penalties & mandatory stop",
					category: "AC-NATIVE · SESSION",
					detail: "Race-control state",
					available: true,
				},
				{
					id: "native.session.conditions",
					label: "Grip, wind & replay speed",
					category: "AC-NATIVE · SESSION",
					detail: "Conditions and compound",
					available: true,
				},
				{
					id: "native.static.identities",
					label: "Car, track, layout & skin IDs",
					category: "AC-NATIVE · CAR & TRACK",
					detail: "Static identity fields",
					available: true,
				},
				{
					id: "native.static.limits",
					label: "Car limits & track length",
					category: "AC-NATIVE · CAR & TRACK",
					detail: "Vehicle and circuit limits",
					available: true,
				},
				{
					id: "native.static.configuration",
					label: "Assists, rates & pit window",
					category: "AC-NATIVE · CAR & TRACK",
					detail: "Session configuration",
					available: true,
				},
			],
		};
	},
	async getLivePedalTelemetry() {
		const phase = performance.now() / 1_000;
		return {
			connection: "replay",
			simulatorName: "Assetto Corsa",
			session: "MUGELLO / TATUUS FA01",
			sequence: Math.floor(performance.now() / 16),
			throttlePercent: Math.max(0, Math.sin(phase) * 76 + 22),
			brakePercent: Math.max(0, Math.sin(phase + Math.PI) * 88),
			clutchPercent: Math.max(0, Math.sin(phase * 0.45 + 2) * 35),
			steeringDegrees: Math.sin(phase * 0.8) * 155,
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
					{
						index: 1,
						time: "1:52.418",
						durationNs: 112_418_000_000,
						validity: "valid",
						maxTyresOut: 0,
						sectors: [
							{ index: 1, time: "0:37.518", durationNs: 37_518_000_000 },
							{ index: 2, time: "0:38.406", durationNs: 38_406_000_000 },
							{ index: 3, time: "0:36.494", durationNs: 36_494_000_000 },
						],
					},
					{
						index: 2,
						time: "1:50.906",
						durationNs: 110_906_000_000,
						validity: "valid",
						maxTyresOut: 0,
						isFastest: true,
						sectors: [
							{ index: 1, time: "0:36.901", durationNs: 36_901_000_000 },
							{ index: 2, time: "0:37.802", durationNs: 37_802_000_000 },
							{ index: 3, time: "0:36.203", durationNs: 36_203_000_000 },
						],
					},
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
			format: format === "trace" ? "TRACE session" : format === "arrow" ? "Arrow IPC" : "CSV",
			sampleCount: 0,
		};
	},
	async importSession(_path) {
		return { sessionId: "imported-fixture", lapCount: 3, sampleCount: 3_600, setupName: "shared-race.ini" };
	},
	async getSessionLapMetrics(_sessionId) {
		return [
			{
				lapIndex: 1,
				fuelStartLitres: 22,
				fuelEndLitres: 21.2,
				fuelUsedLitres: 0.8,
				fuelCapacityLitres: 30,
				maxSpeedKmh: 226.4,
				tyreWearStart: [100, 100, 100, 100],
				tyreWearEnd: [99.6, 99.6, 99.8, 99.8],
				tyreWearMinimum: [99.6, 99.6, 99.8, 99.8],
			},
			{
				lapIndex: 2,
				fuelStartLitres: 21.2,
				fuelEndLitres: 20.4,
				fuelUsedLitres: 0.8,
				fuelCapacityLitres: 30,
				maxSpeedKmh: 231.1,
				tyreWearStart: [99.6, 99.6, 99.8, 99.8],
				tyreWearEnd: [99.2, 99.2, 99.6, 99.6],
				tyreWearMinimum: [99.2, 99.2, 99.6, 99.6],
			},
		];
	},
	async compareSessionLaps(referenceSessionId, referenceLapIndex, comparisonSessionId, comparisonLapIndex) {
		const samples = Array.from({ length: 201 }, (_, index) => {
			const distanceM = index * 25;
			const phase = (index / 200) * Math.PI * 8;
			return {
				distanceM,
				deltaSeconds: index === 0 ? 0 : Math.sin(phase * 0.35) * 0.18 + (index / 200) * 0.42,
				referenceElapsedSeconds: (index / 200) * 110.906,
				comparisonElapsedSeconds: (index / 200) * 111.328,
				referenceSpeedKmh: 178 + Math.sin(phase) * 48,
				comparisonSpeedKmh: 174 + Math.sin(phase + 0.08) * 47,
				referenceThrottlePercent: Math.sin(phase) > -0.35 ? 100 : 15,
				comparisonThrottlePercent: Math.sin(phase + 0.1) > -0.3 ? 100 : 12,
				referenceBrakePercent: Math.sin(phase) < -0.55 ? 72 : 0,
				comparisonBrakePercent: Math.sin(phase + 0.1) < -0.5 ? 78 : 0,
				referenceSteeringDegrees: Math.sin(phase) * 42,
				comparisonSteeringDegrees: Math.sin(phase + 0.07) * 45,
				referenceRpm: 6_200 + Math.sin(phase * 1.4) * 1_400,
				comparisonRpm: 6_050 + Math.sin(phase * 1.4 + 0.1) * 1_450,
				sectorIndex: Math.min(3, Math.floor(index / 67) + 1),
				referenceGear: Math.max(2, Math.min(6, Math.round(4 + Math.sin(phase) * 2))),
				comparisonGear: Math.max(2, Math.min(6, Math.round(4 + Math.sin(phase + 0.08) * 2))),
				referencePositionXM: Math.cos((index / 200) * Math.PI * 2) * (260 + Math.sin(phase) * 60),
				referencePositionZM: Math.sin((index / 200) * Math.PI * 2) * (180 + Math.cos(phase) * 35),
				comparisonPositionXM: Math.cos((index / 200) * Math.PI * 2 + 0.004) * (260 + Math.sin(phase) * 60),
				comparisonPositionZM: Math.sin((index / 200) * Math.PI * 2 + 0.004) * (180 + Math.cos(phase) * 35),
				referenceAirTemperatureC: 22,
				referenceTrackTemperatureC: 31.5,
				comparisonAirTemperatureC: 22,
				comparisonTrackTemperatureC: 31.5,
			};
		});
		const cornerAnalysis: StructuredAnalysisResult<CornerComparisonAnalysis> = {
			availability: "Available",
			confidence: 0.82,
			value: {
				corners: [
					{
						index: 1,
						label: "T1",
						startDistanceM: 425,
						apexDistanceM: 575,
						endDistanceM: 750,
						totalLossSeconds: 0.184,
						phases: [
							{ phase: "entry", startDistanceM: 425, endDistanceM: 525, lossSeconds: 0.021 },
							{ phase: "mid", startDistanceM: 525, endDistanceM: 625, lossSeconds: 0.069 },
							{ phase: "exit", startDistanceM: 625, endDistanceM: 750, lossSeconds: 0.094 },
						],
						metrics: {
							referenceBrakingPointM: 450,
							comparisonBrakingPointM: 475,
							referenceBrakeReleasePointM: 550,
							comparisonBrakeReleasePointM: 575,
							referencePeakBrakePercent: 72,
							comparisonPeakBrakePercent: 78,
							referenceMinimumSpeedKmh: 91,
							comparisonMinimumSpeedKmh: 84,
							referenceThrottlePointM: 625,
							comparisonThrottlePointM: 650,
						},
					},
					{
						index: 2,
						label: "T2",
						startDistanceM: 1_675,
						apexDistanceM: 1_825,
						endDistanceM: 2_025,
						totalLossSeconds: 0.112,
						phases: [
							{ phase: "entry", startDistanceM: 1_675, endDistanceM: 1_775, lossSeconds: -0.008 },
							{ phase: "mid", startDistanceM: 1_775, endDistanceM: 1_875, lossSeconds: 0.047 },
							{ phase: "exit", startDistanceM: 1_875, endDistanceM: 2_025, lossSeconds: 0.073 },
						],
						metrics: {
							referenceBrakingPointM: 1_700,
							comparisonBrakingPointM: 1_725,
							referenceBrakeReleasePointM: 1_800,
							comparisonBrakeReleasePointM: 1_825,
							referencePeakBrakePercent: 68,
							comparisonPeakBrakePercent: 74,
							referenceMinimumSpeedKmh: 118,
							comparisonMinimumSpeedKmh: 114,
							referenceThrottlePointM: 1_875,
							comparisonThrottlePointM: 1_900,
						},
					},
				],
			},
		};
		const drivingAnalysis: StructuredAnalysisResult<DrivingAnalysis> = {
			availability: "Available",
			confidence: 0.88,
			value: {
				observations: [
					{
						kind: "later_throttle",
						tier: "high",
						confidence: 0.88,
						cornerIndices: [1, 2, 3],
						eligibleCornerCount: 4,
						meanDifference: 18,
						unit: "metre",
					},
					{
						kind: "lower_minimum_speed",
						tier: "low",
						confidence: 0.61,
						cornerIndices: [1, 2],
						eligibleCornerCount: 4,
						meanDifference: -5.5,
						unit: "kilometres_per_hour",
					},
				],
			},
		};
		return {
			referenceSessionId,
			referenceSessionTitle: "Sunday practice",
			referenceTrack: "MUGELLO",
			referenceCar: "TATUUS FA01",
			comparisonSessionId,
			comparisonSessionTitle: "Evening run",
			comparisonTrack: "MUGELLO",
			comparisonCar: "TATUUS FA01",
			referenceLapIndex,
			referenceLapTime: "1:50.906",
			comparisonLapIndex,
			comparisonLapTime: "1:51.328",
			lapLengthM: 5_000,
			cornerAnalysis,
			drivingAnalysis,
			samples,
		};
	},
	async visualizeSessionLap(sessionId, lapIndex) {
		const comparison = await this.compareSessionLaps(sessionId, lapIndex, sessionId, lapIndex + 1);
		return {
			sessionId,
			simulatorId: "assetto-corsa",
			sourceTrackId: "mugello",
			layoutId: null,
			sourceCarId: "tatuusfa1",
			lapIndex,
			lapTime: comparison.referenceLapTime,
			track: comparison.referenceTrack,
			car: comparison.referenceCar,
			lapLengthM: comparison.lapLengthM,
			samples: comparison.samples.map((sample) => ({
				distanceM: sample.distanceM,
				sectorIndex: sample.sectorIndex,
				speedKmh: sample.referenceSpeedKmh,
				throttlePercent: sample.referenceThrottlePercent,
				brakePercent: sample.referenceBrakePercent,
				steeringDegrees: sample.referenceSteeringDegrees,
				rpm: sample.referenceRpm,
				gear: sample.referenceGear,
				positionXM: sample.referencePositionXM,
				positionZM: sample.referencePositionZM,
				airTemperatureC: sample.referenceAirTemperatureC,
				trackTemperatureC: sample.referenceTrackTemperatureC,
			})),
		};
	},
	async prepareTracerReference(sessionId, lapIndex) {
		return {
			installed: true,
			installPath: "C:\\Assetto Corsa\\apps\\lua\\TRACE_Tracer",
			referencePath: "C:\\Documents\\Assetto Corsa\\cfg\\extension\\state\\lua\\app\\TRACE_Tracer\\reference.json",
			sessionId,
			lapIndex,
			lapTime: "1:48.214",
			brakeZoneCount: 8,
		};
	},
	async getGameInstallDirectories() {
		return [
			{
				simulatorId: "assetto-corsa",
				simulatorName: "Assetto Corsa",
				path: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\assettocorsa",
				source: "detected",
			},
		];
	},
	async setGameInstallDirectory(simulatorId, customPath) {
		return { simulatorId, simulatorName: "Assetto Corsa", path: customPath, source: customPath ? "manual" : "detected" };
	},
	async getSetupImporters() {
		return [
			{
				simulatorId: "assetto-corsa",
				simulatorName: "Assetto Corsa",
				archiveLabel: "Assetto Corsa setup archives",
				archiveExtensions: ["zip"],
				folderLabel: "Assetto Corsa setups folder",
				folderHint: "Usually Documents\\Assetto Corsa\\setups. Change it if your Documents folder lives elsewhere.",
				archiveHint: "TRACE uses an .ld telemetry filename to identify the car and track, then installs every .ini setup in the archive.",
				fileLabel: "Assetto Corsa setup files",
				fileExtensions: ["ini"],
				fileHint: "Choose standalone .ini files, then provide the track.",
			},
		];
	},
	async detectSetupFolder(_simulatorId) {
		return { path: "C:\\Users\\Driver\\Documents\\Assetto Corsa\\setups", found: true, source: "detected" };
	},
	async importSetupArchives(_simulatorId, archivePaths, setupsFolder, _overwrite) {
		return archivePaths.map((path) => ({
			archiveName: path.split(/[\\/]/).at(-1) ?? path,
			car: "ks_mazda_mx5_cup",
			track: "ks_zandvoort",
			files: ["shared-race.ini"],
			destination: `${setupsFolder}\\ks_mazda_mx5_cup\\ks_zandvoort`,
			skipped: [],
			success: true,
		}));
	},
	async importSetupFiles(options) {
		return options.setupPaths.map((path) => ({
			archiveName: path.split(/[\\/]/).pop() ?? path,
			car: options.sourceCarId || "ks_mazda_mx5_cup",
			track: options.sourceTrackId,
			files: [path.split(/[\\/]/).pop() ?? path],
			destination: `${options.setupsFolder}\\${options.sourceCarId || "ks_mazda_mx5_cup"}\\${options.sourceTrackId}`,
			skipped: [],
			success: true,
		}));
	},
	async attachSessionSetup(options) {
		return (await this.getCompatibleSetups(options.sessionId)).map((setup, index) => ({ ...setup, confirmed: index === 0 }));
	},
	async indexExistingSetups() {
		return { indexed: 24, ignored: 2, errors: [], limited: false };
	},
	async getSetupLibrary() {
		return [
			{
				id: "setup-race",
				simulatorId: "assetto-corsa",
				simulatorName: "Assetto Corsa",
				sourceCarId: "ks_mazda_mx5_cup",
				carName: "Mazda MX-5 Cup",
				sourceTrackId: "ks_zandvoort",
				trackName: "Zandvoort",
				name: "race.ini",
				installedPath: "C:\\Users\\Driver\\Documents\\Assetto Corsa\\setups\\ks_mazda_mx5_cup\\ks_zandvoort\\race.ini",
				sourceArchive: "team-pack.zip",
				importedAt: "2026-08-29T18:45:00Z",
				linkedSessionCount: 2,
				available: true,
			},
			{
				id: "setup-qualifying",
				simulatorId: "assetto-corsa",
				simulatorName: "Assetto Corsa",
				sourceCarId: "ks_mazda_mx5_cup",
				carName: "Mazda MX-5 Cup",
				sourceTrackId: "ks_zandvoort",
				trackName: "Zandvoort",
				name: "qualifying.ini",
				installedPath: "C:\\Users\\Driver\\Documents\\Assetto Corsa\\setups\\ks_mazda_mx5_cup\\ks_zandvoort\\qualifying.ini",
				importedAt: "2026-08-28T12:10:00Z",
				linkedSessionCount: 0,
				available: true,
			},
		];
	},
	async getSetupDocument(setupId) {
		return {
			setupId,
			name: setupId === "setup-qualifying" ? "qualifying.ini" : "race.ini",
			simulatorId: "assetto-corsa",
			sourceCarId: "ks_mazda_mx5_cup",
			metadataAvailable: true,
			groups: [
				{
					name: "Tyres",
					values: [
						{
							section: "PRESSURE_LF",
							label: "Pressure LF",
							value: "18",
							editable: true,
							description: "Adjust the starting pressure for the left-front tyre.",
							minimum: "15",
							maximum: "35",
							step: "1",
						},
						{ section: "PRESSURE_RF", label: "Pressure RF", value: "18", editable: true, minimum: "15", maximum: "35", step: "1" },
					],
				},
				{
					name: "Suspension",
					values: [
						{
							section: "ARB_FRONT",
							label: "Front anti-roll bar",
							value: "3",
							editable: true,
							description: "Higher values reduce front roll and sharpen initial response.",
							minimum: "0",
							maximum: "5",
							step: "1",
						},
					],
				},
			],
		};
	},
	async saveSetupCopy(options) {
		return { setupId: `fixture-${options.name}`, name: options.name.toLowerCase().endsWith(".ini") ? options.name : `${options.name}.ini` };
	},
	async getCompatibleSetups(_sessionId) {
		return [
			{
				id: "setup-race",
				name: "shared-race.ini",
				installedPath: "C:\\Users\\Driver\\Documents\\Assetto Corsa\\setups\\ks_mazda_mx5_cup\\ks_zandvoort\\shared-race.ini",
				sourceArchive: "team-zandvoort.zip",
				importedAt: "2026-08-23T08:00:00Z",
				confirmed: true,
				confirmedAt: "2026-08-23T08:05:00Z",
				confirmationSource: "user_confirmed",
			},
			{
				id: "setup-qualifying",
				name: "qualifying.ini",
				installedPath: "C:\\Users\\Driver\\Documents\\Assetto Corsa\\setups\\ks_mazda_mx5_cup\\ks_zandvoort\\qualifying.ini",
				sourceArchive: "sprint-pack.zip",
				importedAt: "2026-08-22T18:30:00Z",
				confirmed: false,
			},
		];
	},
	async confirmSessionSetup(sessionId, setupId) {
		const setups = await this.getCompatibleSetups(sessionId);
		return setups.map((setup) => ({
			...setup,
			confirmed: setup.id === setupId,
			confirmedAt: setup.id === setupId ? new Date().toISOString() : null,
			confirmationSource: setup.id === setupId ? "user_confirmed" : null,
		}));
	},
	async clearSessionSetup(sessionId) {
		return (await this.getCompatibleSetups(sessionId)).map((setup) => ({ ...setup, confirmed: false, confirmedAt: null, confirmationSource: null }));
	},
	async compareSetups(_baselineSetupId, _alternativeSetupId) {
		return {
			baselineName: "shared-race.ini",
			alternativeName: "qualifying.ini",
			changedValues: 3,
			unchangedValues: 18,
			sections: [
				{
					name: "TYRES",
					changes: [
						{ key: "PRESSURE_LF", baselineValue: "20", alternativeValue: "21" },
						{ key: "PRESSURE_RF", baselineValue: "20", alternativeValue: "21" },
					],
				},
				{ name: "ARB", changes: [{ key: "FRONT", baselineValue: "3", alternativeValue: "4" }] },
			],
		};
	},
	async getDriverProfile() {
		return fixtureDriverProfile;
	},
	async setDriverProfile(name) {
		fixtureDriverProfile = { name };
		return fixtureDriverProfile;
	},
	async getStartupSettings() {
		return fixtureStartupSettings;
	},
	async setLaunchOnStartup(enabled) {
		fixtureStartupSettings = { supported: true, enabled };
		return fixtureStartupSettings;
	},
	async getAppBehaviorSettings() {
		return fixtureAppBehaviorSettings;
	},
	async setAppBehaviorSettings(settings) {
		fixtureAppBehaviorSettings = settings;
		return fixtureAppBehaviorSettings;
	},
	async getLiveSettings() {
		return fixtureLiveSettings;
	},
	async setLiveSettings(settings) {
		fixtureLiveSettings = settings;
		return fixtureLiveSettings;
	},
	async setDiscordReviewActivity(_activity) {},
	async getLiveBroadcastStatus() {
		return fixtureLiveBroadcastStatus;
	},
	async startActiveLiveBroadcast(options) {
		fixtureLiveBroadcastStatus = {
			phase: "live",
			sourceSessionId: "active-capture",
			liveSessionId: "preview-active-session",
			spectatorUrl: `${options.mode === "local" ? "http://127.0.0.1:8080" : "https://live.simtrace.run"}/live/preview-active-session`,
			elapsedNs: 0,
			durationNs: 0,
			automatic: false,
		};
		return fixtureLiveBroadcastStatus;
	},
	async startRecordedLiveBroadcast(sessionId, options) {
		const local = options.mode === "local";
		fixtureLiveBroadcastStatus = {
			phase: "live",
			sourceSessionId: sessionId,
			liveSessionId: "preview-live-session",
			spectatorUrl: `${local ? "http://127.0.0.1:8080" : "https://live.simtrace.run"}/live/preview-live-session`,
			elapsedNs: 0,
			durationNs: 110_906_000_000,
			automatic: false,
		};
		return fixtureLiveBroadcastStatus;
	},
	async stopLiveBroadcast() {
		fixtureLiveBroadcastStatus = { phase: "idle", elapsedNs: 0, durationNs: 0, automatic: false };
		return fixtureLiveBroadcastStatus;
	},
	async getSavedComparisons() {
		return fixtureSavedComparisons;
	},
	async saveComparison(name, referenceSessionId, referenceLapIndex, analysedSessionId, analysedLapIndex) {
		fixtureSavedComparisons = [
			{
				id: `comparison-${Date.now()}`,
				name,
				referenceSessionId,
				referenceLapIndex,
				referenceDurationNs: 110_906_000_000,
				referenceStartedAt: "2026-08-21T14:32:00Z",
				analysedSessionId,
				analysedLapIndex,
				analysedDurationNs: 111_328_000_000,
				analysedStartedAt: "2026-08-21T16:18:00Z",
				simulatorKey: "assetto-corsa",
				track: "MUGELLO",
				car: "TATUUS FA01",
				createdAt: new Date().toISOString(),
			},
			...fixtureSavedComparisons,
		];
		return fixtureSavedComparisons;
	},
	async deleteSavedComparison(comparisonId) {
		fixtureSavedComparisons = fixtureSavedComparisons.filter((comparison) => comparison.id !== comparisonId);
		return fixtureSavedComparisons;
	},
	async renameSavedComparison(comparisonId, name) {
		fixtureSavedComparisons = fixtureSavedComparisons.map((comparison) =>
			comparison.id === comparisonId ? { ...comparison, name: name.trim() } : comparison,
		);
		return fixtureSavedComparisons;
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
	getLivePedalTelemetry() {
		return invoke<LivePedalTelemetry>("live_pedal_telemetry");
	},
	selectSimulator(simulatorId) {
		return invoke<void>("select_simulator", { simulatorId });
	},
	getSessions() {
		return invoke<RecordedSessionSummary[]>("recent_sessions");
	},
	getSessionLapMetrics(sessionId) {
		return invoke<RecordedLapMetrics[]>("session_lap_metrics", { sessionId });
	},
	visualizeSessionLap(sessionId, lapIndex) {
		return invoke<LapTrace>("visualize_session_lap", { sessionId, lapIndex });
	},
	prepareTracerReference(sessionId, lapIndex) {
		return invoke<TracerReferenceStatus>("prepare_tracer_reference", { sessionId, lapIndex });
	},
	compareSessionLaps(referenceSessionId, referenceLapIndex, comparisonSessionId, comparisonLapIndex) {
		return invoke<LapComparison>("compare_session_laps", { referenceSessionId, referenceLapIndex, comparisonSessionId, comparisonLapIndex });
	},
	getGameInstallDirectories() {
		return invoke<GameInstallDirectory[]>("game_install_directories");
	},
	setGameInstallDirectory(simulatorId, customPath) {
		return invoke<GameInstallDirectory>("set_game_install_directory", { simulatorId, customPath });
	},
	getSetupImporters() {
		return invoke<SetupImporterDescriptor[]>("setup_importers");
	},
	detectSetupFolder(simulatorId) {
		return invoke<SetupFolder>("detect_setup_folder", { simulatorId });
	},
	importSetupArchives(simulatorId, archivePaths, setupsFolder, overwrite) {
		return invoke<SetupImportResult[]>("import_setup_archives", { simulatorId, archivePaths, setupsFolder, overwrite });
	},
	importSetupFiles(options) {
		return invoke<SetupImportResult[]>("import_setup_files", { options });
	},
	attachSessionSetup(options) {
		return invoke<CompatibleSetup[]>("attach_session_setup", { options });
	},
	indexExistingSetups(simulatorId, setupsFolder) {
		return invoke<SetupDiscoveryResult>("index_existing_setups", { simulatorId, setupsFolder });
	},
	getSetupLibrary() {
		return invoke<SetupLibraryEntry[]>("setup_library");
	},
	getSetupDocument(setupId) {
		return invoke<SetupDocument>("setup_document", { setupId });
	},
	saveSetupCopy(options) {
		return invoke<SetupSaveResult>("save_setup_copy", { options });
	},
	getCompatibleSetups(sessionId) {
		return invoke<CompatibleSetup[]>("compatible_setups", { sessionId });
	},
	confirmSessionSetup(sessionId, setupId) {
		return invoke<CompatibleSetup[]>("confirm_session_setup", { sessionId, setupId });
	},
	clearSessionSetup(sessionId) {
		return invoke<CompatibleSetup[]>("clear_session_setup", { sessionId });
	},
	compareSetups(baselineSetupId, alternativeSetupId) {
		return invoke<SetupComparison>("compare_setups", { baselineSetupId, alternativeSetupId });
	},
	getDriverProfile() {
		return invoke<DriverProfile>("driver_profile");
	},
	setDriverProfile(name) {
		return invoke<DriverProfile>("set_driver_profile", { name });
	},
	getStartupSettings() {
		return invoke<StartupSettings>("startup_settings");
	},
	setLaunchOnStartup(enabled) {
		return invoke<StartupSettings>("set_launch_on_startup", { enabled });
	},
	getAppBehaviorSettings() {
		return invoke<AppBehaviorSettings>("app_behavior_settings");
	},
	setAppBehaviorSettings(settings) {
		return invoke<AppBehaviorSettings>("set_app_behavior_settings", {
			closeToTrayEnabled: settings.closeToTrayEnabled,
		});
	},
	getLiveSettings() {
		return invoke<LiveSettings>("live_settings");
	},
	setLiveSettings(settings) {
		return invoke<LiveSettings>("set_live_settings", {
			endpoint: settings.endpoint,
			autoStream: settings.autoStream,
			discordActivityEnabled: settings.discordActivityEnabled,
		});
	},
	setDiscordReviewActivity(activity) {
		return invoke<void>("set_discord_review_activity", { activity });
	},
	getLiveBroadcastStatus() {
		return invoke<LiveBroadcastStatus>("live_broadcast_status");
	},
	startActiveLiveBroadcast(options) {
		return invoke<LiveBroadcastStatus>("start_active_live_broadcast", { options });
	},
	startRecordedLiveBroadcast(sessionId, options) {
		return invoke<LiveBroadcastStatus>("start_recorded_live_broadcast", { sessionId, options });
	},
	stopLiveBroadcast() {
		return invoke<LiveBroadcastStatus>("stop_live_broadcast");
	},
	getSavedComparisons() {
		return invoke<SavedComparison[]>("saved_comparisons");
	},
	saveComparison(name, referenceSessionId, referenceLapIndex, analysedSessionId, analysedLapIndex) {
		return invoke<SavedComparison[]>("save_comparison", { name, referenceSessionId, referenceLapIndex, analysedSessionId, analysedLapIndex });
	},
	deleteSavedComparison(comparisonId) {
		return invoke<SavedComparison[]>("delete_saved_comparison", { comparisonId });
	},
	renameSavedComparison(comparisonId, name) {
		return invoke<SavedComparison[]>("rename_saved_comparison", { comparisonId, name });
	},
	exportSession(sessionId, exportFormat) {
		return invoke<SessionExport>("export_session", { sessionId, exportFormat });
	},
	importSession(path) {
		return invoke<SessionImport>("import_session", { path });
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
