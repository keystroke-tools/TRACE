import { useEffect, useMemo, useRef, useState } from "react";
import { PageIntro, PanelTitle } from "../../components/layout";
import { telemetryDataSource, type LapTrace, type LivePedalTelemetry, type RecordedSessionSummary, type TelemetryStatus } from "../../data-source";
import { Tooltip } from "../../Tooltip";
import { useToast } from "../../Toast";
import {
	loadOverlaySettings,
	PEDAL_OVERLAY_WIDTH,
	PEDAL_OVERLAY_SETTINGS_KEY,
	PedalOverlaySettings,
	PedalOverlaySurface,
	saveOverlaySettings,
	type InputHistorySample,
	type OverlaySettings,
} from "../telemetry/PedalOverlay";
import { PedalOverlayLauncher } from "../telemetry/PedalOverlayLauncher";

type PreviewSource = "demo" | "live" | "recorded";

const OVERLAY_CATALOG = [
	{
		id: "pedal-inputs",
		name: "PEDAL INPUTS",
		description: "Live input history, pedal bars, and steering in a compact HUD.",
		defaultSize: `${PEDAL_OVERLAY_WIDTH} × 180`,
	},
] as const;

interface LapOption {
	key: string;
	session: RecordedSessionSummary;
	lapIndex: number;
	lapTime: string;
}

const dateFormatter = new Intl.DateTimeFormat(undefined, {
	day: "2-digit",
	month: "short",
	year: "numeric",
	hour: "2-digit",
	minute: "2-digit",
});

export function OverlaysPage({ sessions, status }: { sessions: RecordedSessionSummary[]; status: TelemetryStatus | null }) {
	const showToast = useToast();
	const [selectedOverlayId, setSelectedOverlayId] = useState<(typeof OVERLAY_CATALOG)[number]["id"]>("pedal-inputs");
	const selectedOverlay = OVERLAY_CATALOG.find((overlay) => overlay.id === selectedOverlayId) ?? OVERLAY_CATALOG[0];
	const [settings, setSettings] = useState<OverlaySettings>(loadOverlaySettings);
	const [settingsDockOpen, setSettingsDockOpen] = useState(true);
	const [source, setSource] = useState<PreviewSource>("demo");
	const lapOptions = useMemo<LapOption[]>(
		() =>
			sessions.flatMap((session) =>
				session.laps
					.filter((lap) => (lap.durationNs ?? 0) > 1_000_000_000)
					.map((lap) => ({ key: `${session.id}\u0000${lap.index}`, session, lapIndex: lap.index, lapTime: lap.time })),
			),
		[sessions],
	);
	const [selectedLapKey, setSelectedLapKey] = useState("");
	const selectedLap = lapOptions.find((option) => option.key === selectedLapKey) ?? lapOptions[0] ?? null;
	const selectedSessionId = selectedLap?.session.id ?? null;
	const selectedLapIndex = selectedLap?.lapIndex ?? null;
	const [trace, setTrace] = useState<LapTrace | null>(null);
	const [loadingTrace, setLoadingTrace] = useState(false);
	const [playbackSeconds, setPlaybackSeconds] = useState(0);
	const [playing, setPlaying] = useState(true);
	const [liveTelemetry, setLiveTelemetry] = useState<LivePedalTelemetry | null>(null);
	const [liveHistory, setLiveHistory] = useState<InputHistorySample[]>([]);
	const [demoTelemetry, setDemoTelemetry] = useState<LivePedalTelemetry>(() => demoPedals(0));
	const animationFrame = useRef(0);
	const liveSequence = useRef<number | null>(null);

	useEffect(() => {
		saveOverlaySettings(settings);
	}, [settings]);

	useEffect(() => {
		const syncSettings = (event: StorageEvent) => {
			if (event.key === PEDAL_OVERLAY_SETTINGS_KEY) setSettings(loadOverlaySettings());
		};
		window.addEventListener("storage", syncSettings);
		return () => window.removeEventListener("storage", syncSettings);
	}, []);

	useEffect(() => {
		if (!selectedLapKey && lapOptions[0]) setSelectedLapKey(lapOptions[0].key);
	}, [lapOptions, selectedLapKey]);

	useEffect(() => {
		if (source !== "live") return;
		let active = true;
		let timeout = 0;
		const poll = async () => {
			try {
				const next = await telemetryDataSource.getLivePedalTelemetry();
				if (active) {
					setLiveTelemetry(next);
					if (next.sequence !== liveSequence.current) {
						const now = performance.now() / 1_000;
						setLiveHistory((current) => [
							...current.filter((sample) => sample.timeSeconds >= now - settings.historySeconds - 0.25),
							{
								timeSeconds: now,
								throttlePercent: next.throttlePercent,
								brakePercent: next.brakePercent,
								clutchPercent: next.clutchPercent,
								steeringDegrees: next.steeringDegrees,
							},
						]);
						liveSequence.current = next.sequence;
					}
				}
			} catch {
				if (active) setLiveTelemetry(null);
			}
			if (active) timeout = window.setTimeout(poll, 33);
		};
		void poll();
		return () => {
			active = false;
			window.clearTimeout(timeout);
		};
	}, [settings.historySeconds, source]);

	useEffect(() => {
		if (source !== "recorded" || selectedSessionId === null || selectedLapIndex === null) return;
		let active = true;
		setLoadingTrace(true);
		setTrace(null);
		setPlaybackSeconds(0);
		void telemetryDataSource
			.visualizeSessionLap(selectedSessionId, selectedLapIndex)
			.then((value) => {
				if (active) {
					setTrace(value);
					setPlaying(true);
				}
			})
			.catch((error) => {
				if (!active) return;
				showToast({
					kind: "error",
					title: "Recorded preview unavailable",
					message: error instanceof Error ? error.message : String(error),
					timeoutMs: 8_000,
				});
			})
			.finally(() => {
				if (active) setLoadingTrace(false);
			});
		return () => {
			active = false;
		};
	}, [selectedLapIndex, selectedSessionId, showToast, source]);

	const durationSeconds = useMemo(
		() =>
			trace?.samples.reduce(
				(duration, sample) => (sample.elapsedSeconds != null && Number.isFinite(sample.elapsedSeconds) ? sample.elapsedSeconds : duration),
				0,
			) ?? 0,
		[trace],
	);

	useEffect(() => {
		if (!playing || (source === "recorded" && durationSeconds <= 0)) return;
		let previous = performance.now();
		const tick = (now: number) => {
			const delta = (now - previous) / 1_000;
			previous = now;
			if (source === "recorded") {
				setPlaybackSeconds((current) => Math.min(durationSeconds, current + delta));
			} else if (source === "demo") {
				setDemoTelemetry(demoPedals(now / 1_000));
			}
			animationFrame.current = requestAnimationFrame(tick);
		};
		animationFrame.current = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(animationFrame.current);
	}, [durationSeconds, playing, source]);

	useEffect(() => {
		if (source === "recorded" && durationSeconds > 0 && playbackSeconds >= durationSeconds) setPlaying(false);
	}, [durationSeconds, playbackSeconds, source]);

	const recordedTelemetry = useMemo(() => {
		if (!trace || !selectedLap) return null;
		const sample = sampleAtTime(trace, playbackSeconds);
		return {
			connection: "replay" as const,
			simulatorName: selectedLap.session.driver?.trim() || "RECORDED LAP",
			session: `${selectedLap.session.track} / ${selectedLap.session.car}`,
			sequence: Math.round(sample?.distanceM ?? 0),
			throttlePercent: sample?.throttlePercent,
			brakePercent: sample?.brakePercent,
			clutchPercent: sample?.clutchPercent,
			steeringDegrees: sample?.steeringDegrees,
		};
	}, [playbackSeconds, selectedLap, trace]);

	const previewTelemetry = source === "live" ? liveTelemetry : source === "recorded" ? recordedTelemetry : demoTelemetry;
	const obsUrl = useMemo(() => pedalObsUrl(settings), [settings]);
	const previewHistory = useMemo(() => {
		if (source === "live") return liveHistory;
		if (source === "recorded") {
			return (trace?.samples ?? [])
				.filter((sample) => {
					const time = sample.elapsedSeconds ?? 0;
					return time <= playbackSeconds && time >= playbackSeconds - settings.historySeconds;
				})
				.map((sample) => ({
					timeSeconds: sample.elapsedSeconds ?? 0,
					throttlePercent: sample.throttlePercent,
					brakePercent: sample.brakePercent,
					clutchPercent: sample.clutchPercent,
					steeringDegrees: sample.steeringDegrees,
				}));
		}
		return demoHistory(demoTelemetry.sequence / 60, settings.historySeconds);
	}, [demoTelemetry.sequence, liveHistory, playbackSeconds, settings.historySeconds, source, trace]);

	async function copyObsUrl() {
		try {
			await navigator.clipboard.writeText(obsUrl);
			showToast({ kind: "success", title: "OBS URL copied", message: "Add it as an OBS Browser Source while TRACE is running." });
		} catch {
			showToast({ kind: "error", title: "Could not copy OBS URL", message: "Select the URL shown below and copy it manually." });
		}
	}

	function selectPreviewSource(nextSource: PreviewSource) {
		setSource(nextSource);
		if (nextSource === "recorded") setPlaybackSeconds(0);
		if (nextSource !== "live") setPlaying(true);
	}

	return (
		<>
			<PageIntro
				index="04"
				eyebrow="OVERLAY WORKSHOP"
				title="BUILD THE VIEW YOU NEED"
				description="Preview overlays at their real window proportions, test them against live or recorded telemetry, then open the configured overlay above your simulator or in OBS."
			/>

			<div
				className={`mt-6 grid min-h-[660px] ${settingsDockOpen ? "grid-cols-[240px_minmax(0,1fr)_320px] max-[1250px]:grid-cols-[210px_minmax(0,1fr)_300px]" : "grid-cols-[240px_minmax(0,1fr)_44px] max-[1050px]:grid-cols-[210px_minmax(0,1fr)_44px]"} border border-trace-divider bg-trace-surface`}
			>
				<aside className="border-r border-trace-divider bg-trace-deep">
					<PanelTitle>OVERLAYS · {OVERLAY_CATALOG.length}</PanelTitle>
					{OVERLAY_CATALOG.map((overlay) => (
						<button
							key={overlay.id}
							type="button"
							onClick={() => setSelectedOverlayId(overlay.id)}
							className={`w-full border-0 border-l-[3px] p-4 text-left ${selectedOverlay.id === overlay.id ? "border-trace-accent bg-trace-accent-wash" : "border-transparent bg-transparent hover:bg-trace-raised"}`}
						>
							<span className="block text-[13px] font-black tracking-[.08em] text-trace-text">{overlay.name}</span>
							<span className="mt-2 block text-[12px] leading-5 text-trace-muted">{overlay.description}</span>
							<span className="mt-3 inline-flex border border-trace-accent-muted px-2 py-1 font-mono text-[10px] text-trace-accent">
								AVAILABLE
							</span>
						</button>
					))}
					<div className="border-t border-trace-divider p-4 text-[12px] leading-5 text-trace-dim">
						New overlay types will appear here without changing the preview and playback workflow.
					</div>
				</aside>

				<section className="min-w-0">
					<div className="flex min-h-16 flex-wrap items-center justify-between gap-3 border-b border-trace-divider px-5 py-3">
						<div>
							<strong className="block text-[13px] tracking-[.08em]">{selectedOverlay.name}</strong>
							<span className="mt-1 block text-[12px] text-trace-dim">
								{selectedOverlay.defaultSize} default · resizable · right-click to close
							</span>
						</div>
						<PedalOverlayLauncher />
					</div>

					<div className="grid border-b border-trace-divider bg-trace-black/50" style={{ height: Math.max(200, settings.overlayHeight + 40) }}>
						<div className="grid min-w-0 place-items-center p-7">
							<div className="w-full" style={{ height: settings.overlayHeight, maxWidth: PEDAL_OVERLAY_WIDTH }}>
								<PedalOverlaySurface telemetry={previewTelemetry} history={previewHistory} settings={settings} className="h-full" />
							</div>
						</div>
					</div>
					<div className="flex min-w-0 items-center gap-3 border-b border-trace-divider bg-trace-deep px-5 py-3">
						<span className="shrink-0 text-[10px] font-black tracking-[.12em] text-trace-dim">OBS BROWSER SOURCE</span>
						<span className="shrink-0 border border-trace-divider bg-trace-black px-2 py-2 font-mono text-[10px] tabular-nums text-trace-soft">
							{PEDAL_OVERLAY_WIDTH} × {settings.overlayHeight}
						</span>
						<input
							readOnly
							value={obsUrl}
							onFocus={(event) => event.currentTarget.select()}
							aria-label="OBS Browser Source URL"
							className="min-w-0 flex-1 border border-trace-divider bg-trace-black px-3 py-2 font-mono text-[11px] text-trace-muted outline-none focus:border-trace-accent-muted"
						/>
						<button
							type="button"
							onClick={() => void copyObsUrl()}
							className="shrink-0 border border-trace-divider bg-trace-raised px-3 py-2 text-[10px] font-black tracking-[.1em] text-trace-soft hover:border-trace-accent-muted hover:text-trace-accent"
						>
							COPY URL
						</button>
					</div>

					<div className="bg-trace-divider">
						<PreviewSourcePanel
							source={source}
							onSource={selectPreviewSource}
							status={status}
							lapOptions={lapOptions}
							selectedLapKey={selectedLap?.key ?? ""}
							onLap={setSelectedLapKey}
							loading={loadingTrace}
							playing={playing}
							onPlaying={setPlaying}
							position={playbackSeconds}
							duration={durationSeconds}
							onSeek={setPlaybackSeconds}
						/>
					</div>
				</section>

				<aside className="min-h-0 min-w-0 overflow-hidden border-l border-trace-divider bg-trace-deep/95">
					<div className={`flex h-16 items-center border-b border-trace-divider ${settingsDockOpen ? "justify-between px-4" : "justify-center"}`}>
						{settingsDockOpen && (
							<span>
								<strong className="block text-[12px] font-black tracking-[.12em] text-trace-soft">OVERLAY SETTINGS</strong>
								<small className="mt-1 block text-[10px] text-trace-dim">Changes apply to preview, window, and OBS.</small>
							</span>
						)}
						<Tooltip content={settingsDockOpen ? "Collapse overlay settings" : "Open overlay settings"}>
							<button
								type="button"
								onClick={() => setSettingsDockOpen((open) => !open)}
								className="grid size-8 shrink-0 place-items-center border border-trace-divider bg-trace-raised text-trace-muted hover:border-trace-accent-muted hover:text-trace-accent"
								aria-label={settingsDockOpen ? "Collapse overlay settings" : "Open overlay settings"}
							>
								<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
									{settingsDockOpen ? <path d="m10 3-5 5 5 5" /> : <path d="m6 3 5 5-5 5" />}
								</svg>
							</button>
						</Tooltip>
					</div>
					{settingsDockOpen && (
						<div className="h-[calc(100%-64px)] overflow-y-auto p-5">
							<PedalOverlaySettings settings={settings} onChange={setSettings} />
						</div>
					)}
				</aside>
			</div>
		</>
	);
}

function PreviewSourcePanel({
	source,
	onSource,
	status,
	lapOptions,
	selectedLapKey,
	onLap,
	loading,
	playing,
	onPlaying,
	position,
	duration,
	onSeek,
}: {
	source: PreviewSource;
	onSource: (source: PreviewSource) => void;
	status: TelemetryStatus | null;
	lapOptions: LapOption[];
	selectedLapKey: string;
	onLap: (key: string) => void;
	loading: boolean;
	playing: boolean;
	onPlaying: (playing: boolean) => void;
	position: number;
	duration: number;
	onSeek: (position: number) => void;
}) {
	return (
		<div className="bg-trace-surface p-4">
			<div className="mb-3 text-[12px] font-extrabold tracking-[.12em] text-trace-soft">PREVIEW TELEMETRY</div>
			<div className="grid grid-cols-3 border border-trace-divider">
				{(["demo", "live", "recorded"] as const).map((value) => (
					<button
						key={value}
						type="button"
						onClick={() => onSource(value)}
						className={`border-0 border-r border-trace-divider px-2 py-2.5 text-[11px] font-bold tracking-[.08em] last:border-r-0 ${source === value ? "bg-trace-accent-wash text-trace-accent" : "bg-trace-deep text-trace-muted hover:text-trace-text"}`}
					>
						{value.toUpperCase()}
					</button>
				))}
			</div>

			{source === "demo" && (
				<p className="mt-4 text-[12px] leading-5 text-trace-muted">A generated input pattern keeps the preview moving while you customise it.</p>
			)}
			{source === "live" && (
				<p className="mt-4 text-[12px] leading-5 text-trace-muted">
					{status?.connection === "recording"
						? `Reading ${status.simulatorName} now.`
						: `Waiting for ${status?.simulatorName ?? "a simulator"}. Start driving or play a replay.`}
				</p>
			)}
			{source === "recorded" && (
				<div className="mt-4 space-y-4">
					<label className="grid gap-2 text-[11px] font-bold tracking-[.08em] text-trace-dim">
						RECORDED LAP
						<select
							className="trace-select min-w-0 border border-trace-divider bg-trace-deep py-3 pl-3 text-[12px] font-normal tracking-normal text-trace-text"
							value={selectedLapKey}
							disabled={lapOptions.length === 0}
							onChange={(event) => onLap(event.target.value)}
						>
							{lapOptions.length === 0 && <option value="">NO RECORDED LAPS</option>}
							{lapOptions.map((option) => (
								<option key={option.key} value={option.key}>
									{lapLabel(option)}
								</option>
							))}
						</select>
					</label>
					<div className="flex items-center gap-3">
						<button
							type="button"
							disabled={loading || duration <= 0}
							onClick={() => {
								if (!playing && position >= duration) onSeek(0);
								onPlaying(!playing);
							}}
							className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-deep text-trace-accent disabled:text-trace-dim"
							aria-label={playing ? "Pause recorded preview" : "Play recorded preview"}
						>
							{playing ? "Ⅱ" : "▶"}
						</button>
						<input
							className="trace-seek min-w-0 flex-1"
							type="range"
							min="0"
							max={Math.max(duration, 0.001)}
							step="0.01"
							value={Math.min(position, duration)}
							disabled={duration <= 0}
							onChange={(event) => {
								onPlaying(false);
								onSeek(Number(event.target.value));
							}}
							aria-label="Recorded preview position"
						/>
						<span className="w-24 text-right font-mono text-[11px] tabular-nums text-trace-muted">
							{loading ? "LOADING" : `${formatClock(position)} / ${formatClock(duration)}`}
						</span>
					</div>
				</div>
			)}
		</div>
	);
}

function demoPedals(seconds: number): LivePedalTelemetry {
	const cycle = seconds % 8;
	const throttle = cycle < 3 ? cycle / 3 : cycle < 5 ? 1 : Math.max(0, 1 - (cycle - 5) / 1.2);
	const brake = cycle < 5.8 ? 0 : cycle < 6.4 ? (cycle - 5.8) / 0.6 : Math.max(0, 1 - (cycle - 6.4) / 1.2);
	const clutch = cycle < 0.7 ? 1 - cycle / 0.7 : 0;
	return {
		connection: "replay",
		simulatorName: "DEMO",
		session: "OVERLAY PREVIEW",
		sequence: Math.round(seconds * 60),
		throttlePercent: throttle * 100,
		brakePercent: brake * 100,
		clutchPercent: clutch * 100,
		steeringDegrees: Math.sin(seconds * 1.3) * 72 + Math.sin(seconds * 3.1) * 18,
	};
}

function demoHistory(seconds: number, historySeconds: number): InputHistorySample[] {
	const samples: InputHistorySample[] = [];
	const start = Math.max(0, seconds - historySeconds);
	for (let time = start; time <= seconds; time += 1 / 30) {
		const telemetry = demoPedals(time);
		samples.push({
			timeSeconds: time,
			throttlePercent: telemetry.throttlePercent,
			brakePercent: telemetry.brakePercent,
			clutchPercent: telemetry.clutchPercent,
			steeringDegrees: telemetry.steeringDegrees,
		});
	}
	return samples;
}

function sampleAtTime(trace: LapTrace, seconds: number) {
	let low = 0;
	let high = trace.samples.length - 1;
	while (low < high) {
		const middle = Math.ceil((low + high) / 2);
		if ((trace.samples[middle]?.elapsedSeconds ?? Number.POSITIVE_INFINITY) <= seconds) low = middle;
		else high = middle - 1;
	}
	return trace.samples[low] ?? null;
}

function lapLabel(option: LapOption) {
	const driver = option.session.driver?.trim() || "Unknown driver";
	return `${driver} · ${option.session.track} · ${option.session.car} · ${option.session.sessionType} · Lap ${option.lapIndex} ${option.lapTime} · ${dateFormatter.format(new Date(option.session.startedAt))}`;
}

function formatClock(seconds: number) {
	if (!Number.isFinite(seconds) || seconds <= 0) return "0:00.0";
	const minutes = Math.floor(seconds / 60);
	return `${minutes}:${(seconds % 60).toFixed(1).padStart(4, "0")}`;
}

function pedalObsUrl(settings: OverlaySettings) {
	const url = new URL("http://127.0.0.1:18081/overlays/pedals");
	url.searchParams.set("graph", settings.showGraph ? "1" : "0");
	url.searchParams.set("clutch", settings.showClutch ? "1" : "0");
	url.searchParams.set("steering", settings.showSteering ? "1" : "0");
	url.searchParams.set("values", settings.showValues ? "1" : "0");
	url.searchParams.set("horizontalGrid", settings.showHorizontalGrid ? "1" : "0");
	url.searchParams.set("verticalGrid", settings.showVerticalGrid ? "1" : "0");
	url.searchParams.set("history", String(settings.historySeconds));
	url.searchParams.set("graphWidth", String(settings.graphWidthPercent));
	url.searchParams.set("height", String(settings.overlayHeight));
	url.searchParams.set("radius", String(settings.borderRadius));
	url.searchParams.set("background", settings.background);
	url.searchParams.set("opacity", String(settings.backgroundOpacity));
	url.searchParams.set("throttle", settings.throttleColor);
	url.searchParams.set("brake", settings.brakeColor);
	url.searchParams.set("clutchColor", settings.clutchColor);
	url.searchParams.set("steeringColor", settings.steeringColor);
	return url.toString();
}
