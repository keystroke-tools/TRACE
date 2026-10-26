import { channelColours, formatGear, type LapComparisonSample } from "./telemetry-chart";
import { telemetryPlaybackSpeeds } from "./telemetry-playback";

export interface LapTelemetryHudProps {
	identity: string;
	secondary: string;
	samples: LapComparisonSample[];
	cursorIndex: number | null;
	onSeek: (index: number) => void;
	layout?: "fixed" | "embedded";
	distanceUnit?: string;
	airTemperatureC?: number | null;
	trackTemperatureC?: number | null;
	playing?: boolean;
	onTogglePlayback?: () => void;
	playbackSpeed?: number;
	onPlaybackSpeed?: (speed: number) => void;
}

export function LapTelemetryHud({
	identity,
	secondary,
	samples,
	cursorIndex,
	onSeek,
	layout = "fixed",
	distanceUnit = "M",
	airTemperatureC,
	trackTemperatureC,
	playing,
	onTogglePlayback,
	playbackSpeed,
	onPlaybackSpeed,
}: LapTelemetryHudProps) {
	const sample = samples[cursorIndex ?? 0] ?? null;
	const airTemperature = sample?.referenceAirTemperatureC ?? airTemperatureC;
	const trackTemperature = sample?.referenceTrackTemperatureC ?? trackTemperatureC;
	const showConditions = hasHudConditions(airTemperature, trackTemperature);
	const frame =
		layout === "fixed"
			? `trace-telemetry-hud trace-telemetry-hud--single trace-telemetry-hud--fixed fixed bottom-12 left-[calc(var(--trace-sidebar)+24px)] right-6 z-30 grid h-[108px] ${showConditions ? "trace-telemetry-hud--with-conditions grid-cols-[minmax(190px,1fr)_95px_130px_90px_100px_130px_130px_150px]" : "grid-cols-[minmax(190px,1fr)_95px_130px_90px_100px_130px_130px]"} grid-rows-[28px_48px] items-center gap-x-4 gap-y-2 overflow-hidden border border-trace-divider bg-trace-black/95 px-5 py-3 shadow-[0_12px_40px_rgba(0,0,0,.55)] backdrop-blur`
			: `trace-telemetry-hud trace-telemetry-hud--single trace-telemetry-hud--embedded ${showConditions ? "trace-telemetry-hud--with-conditions" : ""} grid min-w-0 grid-cols-[repeat(auto-fit,minmax(105px,1fr))] items-center gap-x-4 gap-y-3 overflow-hidden border border-trace-divider bg-trace-black px-4 py-3`;
	return (
		<div className={frame}>
			<div className="trace-telemetry-hud__seek col-span-full min-w-0 border-b border-trace-divider pb-2">
				<TelemetrySeek
					samples={samples}
					cursorIndex={cursorIndex}
					onSeek={onSeek}
					distanceUnit={distanceUnit}
					playing={playing}
					onTogglePlayback={onTogglePlayback}
					playbackSpeed={playbackSpeed}
					onPlaybackSpeed={onPlaybackSpeed}
				/>
			</div>
			<div className="trace-telemetry-hud__identity min-w-0">
				<span className="block truncate text-[13px] font-black">{identity}</span>
				<span className="block truncate font-mono text-[11px] text-trace-dim">{secondary}</span>
			</div>
			<HudValue label="DISTANCE" value={sample ? `${Math.round(sample.distanceM)} ${distanceUnit}` : "—"} />
			<HudValue
				label="SPEED / GEAR"
				value={sample?.referenceSpeedKmh == null ? "—" : `${Math.round(sample.referenceSpeedKmh)} · ${formatGear(sample.referenceGear)}`}
				colour={channelColours.speed}
			/>
			<HudValue label="RPM" value={sample?.referenceRpm == null ? "—" : String(Math.round(sample.referenceRpm))} colour={channelColours.rpm} />
			<HudSteering value={sample?.referenceSteeringDegrees} colour={channelColours.steering} />
			<HudProgress label="THROTTLE" value={sample?.referenceThrottlePercent} colour={channelColours.throttle} />
			<HudProgress label="BRAKE" value={sample?.referenceBrakePercent} colour={channelColours.brake} />
			{showConditions && <HudConditions air={airTemperature} track={trackTemperature} />}
		</div>
	);
}

export function TelemetrySeek({
	samples,
	cursorIndex,
	onSeek,
	distanceUnit = "M",
	playing,
	onTogglePlayback,
	playbackSpeed,
	onPlaybackSpeed,
}: Pick<LapTelemetryHudProps, "samples" | "cursorIndex" | "onSeek" | "playing" | "onTogglePlayback" | "playbackSpeed" | "onPlaybackSpeed"> & {
	distanceUnit?: string;
}) {
	const index = Math.min(Math.max(cursorIndex ?? 0, 0), Math.max(samples.length - 1, 0));
	const start = samples[0]?.distanceM ?? 0;
	const end = samples.at(-1)?.distanceM ?? 0;
	return (
		<div
			className={`trace-telemetry-seek grid h-8 ${onTogglePlayback ? "grid-cols-[auto_48px_minmax(0,1fr)_52px]" : "grid-cols-[52px_minmax(0,1fr)_56px]"} items-center gap-2 font-mono text-[10px] tabular-nums text-trace-dim`}
		>
			{onTogglePlayback && (
				<div className="mr-1 flex items-center gap-2 border-r border-trace-divider pr-3">
					<button
						type="button"
						onClick={onTogglePlayback}
						disabled={samples.length < 2}
						className={`grid h-8 w-11 place-items-center border ${playing ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:border-trace-soft hover:text-trace-text"} disabled:cursor-not-allowed disabled:opacity-35`}
						aria-label={playing ? "Pause telemetry playback" : "Play telemetry playback"}
					>
						{playing ? (
							<svg className="size-3 fill-current" viewBox="0 0 12 12" aria-hidden="true">
								<path d="M2 1h3v10H2zm5 0h3v10H7z" />
							</svg>
						) : (
							<svg className="size-3 fill-current" viewBox="0 0 12 12" aria-hidden="true">
								<path d="m3 1 8 5-8 5z" />
							</svg>
						)}
					</button>
					{onPlaybackSpeed && (
						<select
							value={playbackSpeed ?? 1}
							onChange={(event) => onPlaybackSpeed(Number(event.target.value))}
							className="h-8 w-11 cursor-pointer appearance-none border border-trace-divider bg-trace-deep p-0 text-center text-[10px] font-black tabular-nums text-trace-soft outline-none hover:border-trace-soft hover:text-trace-text focus:border-trace-accent"
							aria-label="Telemetry playback speed"
						>
							{telemetryPlaybackSpeeds.map((speed) => (
								<option value={speed} key={speed}>
									{speed}×
								</option>
							))}
						</select>
					)}
				</div>
			)}
			<span>
				{Math.round(start)} {distanceUnit}
			</span>
			<input
				className="trace-seek trace-seek-lg w-full"
				type="range"
				min="0"
				max={Math.max(samples.length - 1, 0)}
				step="1"
				value={index}
				disabled={samples.length < 2}
				onChange={(event) => onSeek(Number(event.target.value))}
				aria-label="Seek through lap distance"
			/>
			<span className="text-right">
				{Math.round(end)} {distanceUnit}
			</span>
		</div>
	);
}

export function HudValue({ label, value, colour }: { label: string; value: string; colour?: string }) {
	return (
		<div className="h-10 min-w-0 overflow-hidden font-mono">
			<span className="block truncate whitespace-nowrap text-[10px] font-bold leading-3 tracking-[.1em] text-trace-dim">{label}</span>
			<strong className="mt-1 block truncate whitespace-nowrap text-[15px] leading-5 tabular-nums" style={{ color: colour }}>
				{value}
			</strong>
		</div>
	);
}

export function HudSteering({ value, colour }: { value?: number | null; colour: string }) {
	const rotation = value == null || !Number.isFinite(value) ? 0 : Math.min(450, Math.max(-450, value));
	return (
		<div className="flex h-10 min-w-0 items-center gap-2 font-mono">
			<svg
				className="size-9 shrink-0"
				viewBox="0 0 36 36"
				role="img"
				aria-label={value == null ? "Steering unavailable" : `Steering angle ${Math.round(value)} degrees`}
			>
				<circle cx="18" cy="18" r="15" fill="var(--trace-deep)" stroke={colour} strokeWidth="2" />
				<g transform={`rotate(${rotation} 18 18)`} stroke={colour} strokeWidth="2" strokeLinecap="round">
					<line x1="18" y1="18" x2="18" y2="31" />
					<line x1="7" y1="17" x2="29" y2="17" />
				</g>
				<circle cx="18" cy="18" r="2.5" fill={colour} />
			</svg>
			<span className="min-w-0">
				<span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">STEER</span>
				<strong className="block truncate text-[12px] leading-4 tabular-nums" style={{ color: colour }}>
					{value == null ? "—" : `${Math.round(value)}°`}
				</strong>
			</span>
		</div>
	);
}

export function HudProgress({ label, value, colour }: { label: string; value?: number | null; colour: string }) {
	const primary = Math.min(100, Math.max(0, value ?? 0));
	return (
		<div className="h-10 overflow-hidden font-mono">
			<span className="flex h-3 justify-between whitespace-nowrap text-[10px] font-bold leading-3 tracking-[.08em] tabular-nums text-trace-dim">
				<span className="truncate">{label}</span>
				<span>{Math.round(primary)}%</span>
			</span>
			<span className="mt-2 block h-2 overflow-hidden bg-trace-divider">
				<span className="block h-full transition-[width] duration-75" style={{ width: `${primary}%`, backgroundColor: colour }} />
			</span>
		</div>
	);
}

export function HudConditions({ air, track }: { air?: number | null; track?: number | null }) {
	return (
		<div className="h-10 min-w-0 overflow-hidden font-mono">
			<span className="block text-[9px] font-bold leading-3 tracking-[.08em] text-trace-dim">CONDITIONS</span>
			<span className="mt-1 flex items-center gap-2 whitespace-nowrap text-[10px] leading-4 tabular-nums">
				{air != null && (
					<span className="text-trace-dim">
						AIR <strong className="text-trace-text">{Math.round(air)}°C</strong>
					</span>
				)}
				{track != null && (
					<span className="text-trace-dim">
						TRACK <strong className="text-trace-text">{Math.round(track)}°C</strong>
					</span>
				)}
			</span>
		</div>
	);
}

export function hasHudConditions(air?: number | null, track?: number | null) {
	return (air != null && Number.isFinite(air)) || (track != null && Number.isFinite(track));
}

export function HudPedals({ throttle, brake }: { throttle?: number | null; brake?: number | null }) {
	return (
		<div className="grid h-10 w-full min-w-0 max-w-80 grid-cols-2 gap-3 justify-self-center">
			<HudProgress label="THROTTLE" value={throttle} colour={channelColours.throttle} />
			<HudProgress label="BRAKE" value={brake} colour={channelColours.brake} />
		</div>
	);
}
