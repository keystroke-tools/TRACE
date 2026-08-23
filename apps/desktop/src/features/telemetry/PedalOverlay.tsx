import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { telemetryDataSource, type LivePedalTelemetry } from "../../data-source";

export const PEDAL_OVERLAY_SETTINGS_KEY = "trace.pedal-overlay.settings.v7";
export const PEDAL_OVERLAY_WIDTH = 540;

export interface OverlaySettings {
	showGraph: boolean;
	showClutch: boolean;
	showSteering: boolean;
	showValues: boolean;
	showHorizontalGrid: boolean;
	showVerticalGrid: boolean;
	historySeconds: number;
	graphWidthPercent: number;
	overlayHeight: number;
	borderRadius: number;
	background: string;
	backgroundOpacity: number;
	throttleColor: string;
	brakeColor: string;
	clutchColor: string;
	steeringColor: string;
	alwaysOnTop: boolean;
}

export interface InputHistorySample {
	timeSeconds: number;
	throttlePercent?: number | null;
	brakePercent?: number | null;
	clutchPercent?: number | null;
	steeringDegrees?: number | null;
}

const DEFAULT_SETTINGS: OverlaySettings = {
	showGraph: true,
	showClutch: true,
	showSteering: true,
	showValues: false,
	showHorizontalGrid: true,
	showVerticalGrid: true,
	historySeconds: 6,
	graphWidthPercent: 62,
	overlayHeight: 180,
	borderRadius: 8,
	background: "#151515",
	backgroundOpacity: 100,
	throttleColor: "#31d576",
	brakeColor: "#db4b4b",
	clutchColor: "#4b9fe8",
	steeringColor: "#f2f4f3",
	alwaysOnTop: true,
};

export function loadOverlaySettings(): OverlaySettings {
	if (typeof window === "undefined") return DEFAULT_SETTINGS;
	try {
		const stored = JSON.parse(window.localStorage.getItem(PEDAL_OVERLAY_SETTINGS_KEY) ?? "{}") as Partial<OverlaySettings>;
		return { ...DEFAULT_SETTINGS, ...stored };
	} catch {
		return DEFAULT_SETTINGS;
	}
}

export function saveOverlaySettings(settings: OverlaySettings) {
	window.localStorage.setItem(PEDAL_OVERLAY_SETTINGS_KEY, JSON.stringify(settings));
}

export function PedalOverlay() {
	const [telemetry, setTelemetry] = useState<LivePedalTelemetry | null>(null);
	const [history, setHistory] = useState<InputHistorySample[]>([]);
	const [settings, setSettings] = useState<OverlaySettings>(loadOverlaySettings);
	const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
	const lastSequence = useRef<number | null>(null);

	useEffect(() => {
		saveOverlaySettings(settings);
		if (isTauri()) {
			const currentWindow = getCurrentWindow();
			void currentWindow.setAlwaysOnTop(settings.alwaysOnTop);
			void currentWindow.setSize(new LogicalSize(PEDAL_OVERLAY_WIDTH, settings.overlayHeight));
		}
	}, [settings]);

	useEffect(() => {
		const syncSettings = (event: StorageEvent) => {
			if (event.key === PEDAL_OVERLAY_SETTINGS_KEY) setSettings(loadOverlaySettings());
		};
		window.addEventListener("storage", syncSettings);
		return () => window.removeEventListener("storage", syncSettings);
	}, []);

	useEffect(() => {
		let active = true;
		let timeout = 0;
		const poll = async () => {
			try {
				const next = await telemetryDataSource.getLivePedalTelemetry();
				if (!active) return;
				setTelemetry(next);
				if (next.sequence !== lastSequence.current) {
					const now = performance.now() / 1_000;
					setHistory((current) => {
						const base = lastSequence.current !== null && next.sequence < lastSequence.current ? [] : current;
						return appendHistory(base, toHistorySample(next, now), settings.historySeconds);
					});
					lastSequence.current = next.sequence;
				}
			} catch {
				if (active) setTelemetry(null);
			}
			if (active) timeout = window.setTimeout(poll, 33);
		};
		void poll();
		return () => {
			active = false;
			window.clearTimeout(timeout);
		};
	}, [settings.historySeconds]);

	return (
		<div
			className="relative h-dvh w-dvw overflow-hidden p-1.5"
			onClick={() => setContextMenu(null)}
			onContextMenu={(event) => {
				event.preventDefault();
				setContextMenu({ x: event.clientX, y: event.clientY });
			}}
		>
			<PedalOverlaySurface telemetry={telemetry} history={history} settings={settings} className="h-full" draggable />
			{contextMenu && (
				<div
					className="absolute z-20 min-w-40 border border-white/15 bg-[#151515] p-1"
					style={{ left: Math.min(contextMenu.x, window.innerWidth - 170), top: Math.min(contextMenu.y, window.innerHeight - 48) }}
					onClick={(event) => event.stopPropagation()}
				>
					<button
						type="button"
						onClick={() => (isTauri() ? void getCurrentWindow().close() : window.close())}
						className="flex w-full items-center justify-between border-0 bg-transparent px-3 py-2 text-left text-[11px] font-bold tracking-[.08em] text-white/75 hover:bg-white/10 hover:text-white"
					>
						CLOSE OVERLAY
					</button>
				</div>
			)}
		</div>
	);
}

export function PedalOverlaySurface({
	telemetry,
	history,
	settings,
	children,
	className = "",
	draggable = false,
}: {
	telemetry: LivePedalTelemetry | null;
	history: InputHistorySample[];
	settings: OverlaySettings;
	children?: ReactNode;
	className?: string;
	draggable?: boolean;
}) {
	const background = colorWithOpacity(settings.background, settings.backgroundOpacity / 100);
	const visibleModules = Number(settings.showGraph) + Number(settings.showSteering) + 1;

	return (
		<div
			className={`flex min-h-0 select-none flex-col overflow-hidden border border-[#343434] bg-[#151515] text-white ${className}`}
			style={{ background, borderRadius: settings.borderRadius }}
			data-tauri-drag-region={draggable ? "" : undefined}
		>
			<div
				className={`grid min-h-0 flex-1 gap-[7px] p-1.5 ${draggable ? "pointer-events-none" : ""}`}
				style={{ gridTemplateColumns: overlayColumns(settings, visibleModules) }}
			>
				{settings.showGraph && <InputHistoryGraph history={history} settings={settings} />}
				<PedalBars telemetry={telemetry} settings={settings} />
				{settings.showSteering && (
					<SteeringDisplay degrees={telemetry?.steeringDegrees} color={settings.steeringColor} showValue={settings.showValues} />
				)}
			</div>
			{children}
		</div>
	);
}

function InputHistoryGraph({ history, settings }: { history: InputHistorySample[]; settings: OverlaySettings }) {
	const paths = useMemo(() => {
		const end = history.at(-1)?.timeSeconds ?? settings.historySeconds;
		const start = end - settings.historySeconds;
		return {
			throttle: graphPath(history, start, end, (sample) => sample.throttlePercent),
			brake: graphPath(history, start, end, (sample) => sample.brakePercent),
			clutch: graphPath(history, start, end, (sample) => sample.clutchPercent),
		};
	}, [history, settings.historySeconds]);

	return (
		<section className="relative min-h-0 min-w-0 overflow-hidden bg-[#0f0f0f]" style={{ borderRadius: Math.max(0, settings.borderRadius - 3) }}>
			<svg className="h-full min-h-[86px] w-full" viewBox="0 0 360 112" preserveAspectRatio="none" role="img" aria-label="Recent pedal input history">
				{settings.showHorizontalGrid && (
					<path className="stroke-white/10" strokeWidth="1" vectorEffect="non-scaling-stroke" d="M0 28H360M0 56H360M0 84H360" />
				)}
				{settings.showVerticalGrid && (
					<path className="stroke-white/[.06]" strokeWidth="1" vectorEffect="non-scaling-stroke" d="M90 0V112M180 0V112M270 0V112" />
				)}
				<path
					d={paths.throttle}
					fill="none"
					stroke={settings.throttleColor}
					strokeWidth="2"
					strokeLinecap="round"
					strokeLinejoin="round"
					vectorEffect="non-scaling-stroke"
				/>
				<path
					d={paths.brake}
					fill="none"
					stroke={settings.brakeColor}
					strokeWidth="2"
					strokeLinecap="round"
					strokeLinejoin="round"
					vectorEffect="non-scaling-stroke"
				/>
				{settings.showClutch && (
					<path
						d={paths.clutch}
						fill="none"
						stroke={settings.clutchColor}
						strokeWidth="2"
						strokeLinecap="round"
						strokeLinejoin="round"
						vectorEffect="non-scaling-stroke"
					/>
				)}
			</svg>
		</section>
	);
}

function PedalBars({ telemetry, settings }: { telemetry: LivePedalTelemetry | null; settings: OverlaySettings }) {
	const pedals = [
		{ key: "T", name: "Throttle", value: telemetry?.throttlePercent, color: settings.throttleColor },
		{ key: "B", name: "Brake", value: telemetry?.brakePercent, color: settings.brakeColor },
		...(settings.showClutch ? [{ key: "C", name: "Clutch", value: telemetry?.clutchPercent, color: settings.clutchColor }] : []),
	];
	return (
		<section className="flex h-full min-h-0 min-w-[54px] items-stretch justify-center gap-[3px]">
			{pedals.map((pedal) => {
				const value = clampPercent(pedal.value);
				return (
					<div key={pedal.key} className="flex h-full w-4 flex-none flex-col items-center gap-1" title={undefined}>
						<div className="relative min-h-[52px] w-full max-w-4 flex-1 overflow-hidden bg-[#0b0b0b]">
							<div
								className="absolute inset-x-0 bottom-0 opacity-90 transition-[height] duration-75"
								style={{ height: `${value}%`, backgroundColor: pedal.color, color: pedal.color }}
							/>
							<div className="absolute inset-x-0 top-1/2 border-t border-white/10" />
						</div>
						{settings.showValues && (
							<div className="text-center">
								<strong className="block text-[9px] font-black tracking-[.08em] text-white/60" aria-label={pedal.name}>
									{pedal.key}
								</strong>
								<span className="block font-mono text-[10px] font-bold tabular-nums text-white">{Math.round(value)}</span>
							</div>
						)}
					</div>
				);
			})}
		</section>
	);
}

function SteeringDisplay({ degrees, color, showValue }: { degrees?: number | null; color: string; showValue: boolean }) {
	const value = Number.isFinite(degrees) ? Number(degrees) : 0;
	const visualDegrees = Math.max(-180, Math.min(180, -value));
	return (
		<section className="flex min-h-0 min-w-[90px] flex-col items-center justify-center">
			<svg className="min-h-0 w-full max-w-[124px] flex-1" viewBox="0 0 100 100" role="img" aria-label={`Steering ${signedDegrees(value)}`}>
				<circle cx="50" cy="50" r="38" fill="#000" fillOpacity=".32" stroke="#000" strokeOpacity=".42" strokeWidth="7" />
				<g style={{ transform: `rotate(${visualDegrees}deg)`, transformOrigin: "50px 50px" }}>
					<path d="M46 10h8v9h-8z" fill={color} />
				</g>
			</svg>
			{showValue && (
				<span className="font-mono text-[9px] font-bold tabular-nums" style={{ color }}>
					{signedDegrees(value)}
				</span>
			)}
		</section>
	);
}

export function PedalOverlaySettings({ settings, onChange }: { settings: OverlaySettings; onChange: (settings: OverlaySettings) => void }) {
	const update = <Key extends keyof OverlaySettings>(key: Key, value: OverlaySettings[Key]) => onChange({ ...settings, [key]: value });
	return (
		<div className="grid gap-4">
			<div className="grid grid-cols-2 gap-2">
				<SettingToggle label="GRAPH" checked={settings.showGraph} onChange={(value) => update("showGraph", value)} />
				<SettingToggle label="STEERING" checked={settings.showSteering} onChange={(value) => update("showSteering", value)} />
				<SettingToggle label="CLUTCH" checked={settings.showClutch} onChange={(value) => update("showClutch", value)} />
				<SettingToggle label="LABELS + VALUES" checked={settings.showValues} onChange={(value) => update("showValues", value)} />
				<SettingToggle label="HORIZONTAL GRID" checked={settings.showHorizontalGrid} onChange={(value) => update("showHorizontalGrid", value)} />
				<SettingToggle label="VERTICAL GRID" checked={settings.showVerticalGrid} onChange={(value) => update("showVerticalGrid", value)} />
				<SettingToggle label="ALWAYS ON TOP" checked={settings.alwaysOnTop} onChange={(value) => update("alwaysOnTop", value)} />
			</div>
			<label className="grid grid-cols-[1fr_auto] items-center gap-3 text-[11px] font-bold tracking-[.08em] text-current opacity-75">
				<span>GRAPH HISTORY</span>
				<span className="font-mono">{settings.historySeconds}S</span>
				<input
					className="trace-seek col-span-2 w-full"
					type="range"
					min="2"
					max="10"
					step="1"
					value={settings.historySeconds}
					onChange={(event) => update("historySeconds", Number(event.target.value))}
				/>
			</label>
			<label className="grid grid-cols-[1fr_auto] items-center gap-3 text-[11px] font-bold tracking-[.08em] text-current opacity-75">
				<span>GRAPH WIDTH</span>
				<span className="font-mono">{settings.graphWidthPercent}%</span>
				<input
					className="trace-seek col-span-2 w-full"
					type="range"
					min="30"
					max="82"
					step="1"
					value={settings.graphWidthPercent}
					onChange={(event) => update("graphWidthPercent", Number(event.target.value))}
				/>
			</label>
			<label className="grid grid-cols-[1fr_auto] items-center gap-3 text-[11px] font-bold tracking-[.08em] text-current opacity-75">
				<span>OVERLAY HEIGHT</span>
				<span className="font-mono">{settings.overlayHeight}px</span>
				<input
					className="trace-seek col-span-2 w-full"
					type="range"
					min="100"
					max="280"
					step="5"
					value={settings.overlayHeight}
					onChange={(event) => update("overlayHeight", Number(event.target.value))}
				/>
			</label>
			<label className="grid grid-cols-[1fr_auto] items-center gap-3 text-[11px] font-bold tracking-[.08em] text-current opacity-75">
				<span>CORNER RADIUS</span>
				<span className="font-mono">{settings.borderRadius}px</span>
				<input
					className="trace-seek col-span-2 w-full"
					type="range"
					min="0"
					max="24"
					step="1"
					value={settings.borderRadius}
					onChange={(event) => update("borderRadius", Number(event.target.value))}
				/>
			</label>
			<div className="grid grid-cols-2 gap-2">
				<ColorSetting label="THROTTLE" value={settings.throttleColor} onChange={(value) => update("throttleColor", value)} />
				<ColorSetting label="BRAKE" value={settings.brakeColor} onChange={(value) => update("brakeColor", value)} />
				<ColorSetting label="CLUTCH" value={settings.clutchColor} onChange={(value) => update("clutchColor", value)} />
				<ColorSetting label="STEERING" value={settings.steeringColor} onChange={(value) => update("steeringColor", value)} />
				<ColorSetting label="BACKGROUND" value={settings.background} onChange={(value) => update("background", value)} />
			</div>
			<label className="grid grid-cols-[1fr_auto] items-center gap-3 text-[11px] font-bold tracking-[.08em] text-current opacity-75">
				<span>BACKGROUND</span>
				<span className="font-mono">{settings.backgroundOpacity}%</span>
				<input
					className="trace-seek col-span-2 w-full"
					type="range"
					min="20"
					max="100"
					value={settings.backgroundOpacity}
					onChange={(event) => update("backgroundOpacity", Number(event.target.value))}
				/>
			</label>
		</div>
	);
}

function SettingToggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
	return (
		<label
			className={`flex cursor-pointer items-center justify-between gap-2 border px-3 py-2.5 text-[10px] font-black tracking-[.07em] ${checked ? "border-[#31d576]/50 bg-[#31d576]/10 text-[#7eeba9]" : "border-white/15 bg-white/[.03] text-current opacity-55"}`}
		>
			{label}
			<input type="checkbox" className="sr-only" checked={checked} onChange={(event) => onChange(event.target.checked)} />
			<span className={`size-1.5 rounded-full ${checked ? "bg-[#31d576]" : "bg-current opacity-50"}`} />
		</label>
	);
}

function ColorSetting({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
	return (
		<label className="flex items-center gap-2 border border-white/10 px-3 py-2.5 text-[10px] font-bold tracking-[.07em] text-current opacity-70">
			<input
				className="size-5 cursor-pointer border-0 bg-transparent p-0"
				type="color"
				value={value}
				onChange={(event) => onChange(event.target.value)}
			/>
			{label}
		</label>
	);
}

function graphPath(history: InputHistorySample[], start: number, end: number, read: (sample: InputHistorySample) => number | null | undefined) {
	const points = history
		.filter((sample) => sample.timeSeconds >= start && sample.timeSeconds <= end && Number.isFinite(read(sample)))
		.map((sample) => {
			const x = ((sample.timeSeconds - start) / Math.max(0.001, end - start)) * 360;
			const y = 108 - clampPercent(read(sample)) * 1.04;
			return `${x.toFixed(2)} ${y.toFixed(2)}`;
		});
	return points.length > 1 ? `M${points.join(" L")}` : "";
}

function overlayColumns(settings: OverlaySettings, visibleModules: number) {
	if (visibleModules === 1) return "minmax(118px, 1fr)";
	if (!settings.showGraph) return "54px minmax(90px, 1fr)";
	if (!settings.showSteering) return "minmax(120px, 1fr) 54px";
	const steeringWidth = Math.max(10, 88 - settings.graphWidthPercent);
	return `minmax(120px, ${settings.graphWidthPercent}fr) minmax(54px, 12fr) minmax(90px, ${steeringWidth}fr)`;
}

function toHistorySample(telemetry: LivePedalTelemetry, timeSeconds: number): InputHistorySample {
	return {
		timeSeconds,
		throttlePercent: telemetry.throttlePercent,
		brakePercent: telemetry.brakePercent,
		clutchPercent: telemetry.clutchPercent,
		steeringDegrees: telemetry.steeringDegrees,
	};
}

export function appendHistory(history: InputHistorySample[], sample: InputHistorySample, historySeconds: number) {
	const cutoff = sample.timeSeconds - historySeconds - 0.25;
	return [...history.filter((entry) => entry.timeSeconds >= cutoff), sample];
}

function clampPercent(value?: number | null) {
	return Math.max(0, Math.min(100, Number.isFinite(value) ? Number(value) : 0));
}

function signedDegrees(value: number) {
	if (!Number.isFinite(value)) return "—";
	const rounded = Math.round(value);
	return `${rounded > 0 ? "+" : ""}${rounded}°`;
}

function colorWithOpacity(hex: string, opacity: number) {
	const value = hex.replace("#", "");
	if (!/^[0-9a-f]{6}$/i.test(value)) return `rgba(21, 21, 21, ${opacity})`;
	return `rgba(${Number.parseInt(value.slice(0, 2), 16)}, ${Number.parseInt(value.slice(2, 4), 16)}, ${Number.parseInt(value.slice(4, 6), 16)}, ${opacity})`;
}
