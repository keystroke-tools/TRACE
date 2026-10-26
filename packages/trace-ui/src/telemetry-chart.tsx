import { useEffect, useLayoutEffect, useRef, useState } from "react";

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
	referenceGear?: number | null;
	comparisonGear?: number | null;
	sectorIndex?: number | null;
	referencePositionXM?: number | null;
	referencePositionZM?: number | null;
	comparisonPositionXM?: number | null;
	comparisonPositionZM?: number | null;
	referenceAirTemperatureC?: number | null;
	referenceTrackTemperatureC?: number | null;
	comparisonAirTemperatureC?: number | null;
	comparisonTrackTemperatureC?: number | null;
	referenceTyreLoadFrontLeftN?: number | null;
	referenceTyreLoadFrontRightN?: number | null;
	referenceTyreLoadRearLeftN?: number | null;
	referenceTyreLoadRearRightN?: number | null;
	referenceTyreSlipFrontLeft?: number | null;
	referenceTyreSlipFrontRight?: number | null;
	referenceTyreSlipRearLeft?: number | null;
	referenceTyreSlipRearRight?: number | null;
	comparisonTyreLoadFrontLeftN?: number | null;
	comparisonTyreLoadFrontRightN?: number | null;
	comparisonTyreLoadRearLeftN?: number | null;
	comparisonTyreLoadRearRightN?: number | null;
	comparisonTyreSlipFrontLeft?: number | null;
	comparisonTyreSlipFrontRight?: number | null;
	comparisonTyreSlipRearLeft?: number | null;
	comparisonTyreSlipRearRight?: number | null;
}

type ComparisonValueKey = keyof Pick<
	LapComparisonSample,
	| "deltaSeconds"
	| "referenceSpeedKmh"
	| "comparisonSpeedKmh"
	| "referenceThrottlePercent"
	| "comparisonThrottlePercent"
	| "referenceBrakePercent"
	| "comparisonBrakePercent"
	| "referenceSteeringDegrees"
	| "comparisonSteeringDegrees"
	| "referenceRpm"
	| "comparisonRpm"
	| "referenceGear"
	| "comparisonGear"
>;
export type ComparisonChartSeries = { label: string; colour: string; value: (sample: LapComparisonSample) => number | null | undefined };

export const channelColours = {
	speed: "#45d6e8",
	throttle: "#42db76",
	brake: "#ff5263",
	gear: "#5394ff",
	steering: "#f2f3f5",
	rpm: "#ffb84d",
	faster: "var(--color-trace-purple)",
	delta: "#e8eaed",
	mapBrake: "#c13b4a",
};

export function comparisonSeries(
	reference: ComparisonValueKey,
	comparison: ComparisonValueKey,
	colour: string,
	comparisonIsFaster: boolean,
): ComparisonChartSeries[] {
	return [
		{ label: "REFERENCE", colour: comparisonIsFaster ? colour : channelColours.faster, value: (sample) => sample[reference] },
		{ label: "ANALYSED LAP", colour: comparisonIsFaster ? channelColours.faster : colour, value: (sample) => sample[comparison] },
	];
}

export function singleSeries(key: ComparisonValueKey, colour: string): ComparisonChartSeries[] {
	return [{ label: "LAP", colour, value: (sample) => sample[key] }];
}

export function formatGear(gear?: number | null) {
	if (gear == null) return "—";
	if (gear === -1) return "R";
	if (gear === 0) return "N";
	if (gear < -1) return `? (${gear})`;
	return String(gear);
}

export function deltaRange(samples: LapComparisonSample[]): [number, number] {
	const maximum = Math.max(0.1, ...samples.flatMap((sample) => (sample.deltaSeconds == null ? [] : [Math.abs(sample.deltaSeconds)])));
	return [-maximum, maximum];
}

export function steeringAngleRange(samples: LapComparisonSample[]): [number, number] {
	const maximum = Math.max(
		10,
		...samples.flatMap((sample) =>
			[sample.referenceSteeringDegrees, sample.comparisonSteeringDegrees].flatMap((value) =>
				value == null || !Number.isFinite(value) ? [] : [Math.abs(value)],
			),
		),
	);
	const bound = Math.min(720, Math.ceil(maximum / 10) * 10);
	return [-bound, bound];
}

export function ComparisonChart({
	label,
	unit,
	samples,
	series,
	cursorIndex,
	onCursor,
	onZoom,
	onPan,
	fixedRange,
	zeroLine = false,
	compact = false,
	distanceUnit = "M",
	formatValue = formatChartValue,
}: {
	label: string;
	unit: string;
	samples: LapComparisonSample[];
	series: ComparisonChartSeries[];
	cursorIndex: number | null;
	onCursor: (index: number | null) => void;
	onZoom?: (anchorM: number, direction: "in" | "out") => void;
	onPan?: (deltaM: number) => void;
	fixedRange?: [number, number];
	zeroLine?: boolean;
	compact?: boolean;
	distanceUnit?: string;
	formatValue?: (value: number, unit: string) => string;
}) {
	const chart = useRef<HTMLDivElement>(null);
	const plotElement = useRef<SVGSVGElement>(null);
	const dragClientX = useRef<number | null>(null);
	const [width, setWidth] = useState(1_000);
	const [pointerInside, setPointerInside] = useState(false);
	const [dragging, setDragging] = useState(false);
	const height = compact ? 82 : 220;
	const plot = { left: 58, right: 18, top: compact ? 10 : 24, bottom: compact ? 20 : 30 };
	const values = series.flatMap((item) =>
		samples.flatMap((sample) => {
			const value = item.value(sample);
			return value == null || !Number.isFinite(value) ? [] : [value];
		}),
	);
	const automaticMin = values.length ? Math.min(...values) : 0;
	const automaticMax = values.length ? Math.max(...values) : 1;
	const padding = Math.max((automaticMax - automaticMin) * 0.08, 0.01);
	const [minimum, maximum] = fixedRange ?? [automaticMin - padding, automaticMax + padding];
	const range = Math.max(maximum - minimum, 0.000_001);
	const firstDistance = samples[0]?.distanceM ?? 0;
	const lastDistance = samples.at(-1)?.distanceM ?? firstDistance + 1;
	const distanceRange = Math.max(lastDistance - firstDistance, 1);
	const x = (distance: number) => plot.left + ((distance - firstDistance) / distanceRange) * (width - plot.left - plot.right);
	const y = (value: number) => plot.top + ((maximum - value) / range) * (height - plot.top - plot.bottom);
	const cursorSample = cursorIndex == null ? null : (samples[cursorIndex] ?? null);
	const zoomAnchorM = cursorSample?.distanceM ?? firstDistance + distanceRange / 2;
	const cursorX = cursorSample ? x(cursorSample.distanceM) : null;
	const tooltipTransform = cursorX == null ? undefined : cursorX < 100 ? "translateX(5px)" : "translateX(calc(-100% - 5px))";
	const tooltipValues = cursorSample
		? series.flatMap((item) => {
				const value = item.value(cursorSample);
				return value == null || !Number.isFinite(value) ? [] : [{ item, value, chartY: y(value) }];
			})
		: [];
	const chartPixelHeight = compact ? 82 : 224;
	const headerPixelHeight = compact ? 36 : 48;
	const tooltipTops = tooltipValues.map(({ chartY }) => headerPixelHeight + (chartY / height) * chartPixelHeight);
	if (tooltipTops.length > 1) {
		const minimumGap = 26;
		const ordered = tooltipTops.map((top, index) => ({ top, index })).sort((left, right) => left.top - right.top);
		for (let index = 1; index < ordered.length; index += 1) {
			ordered[index].top = Math.max(ordered[index].top, ordered[index - 1].top + minimumGap);
		}
		const minimumTop = headerPixelHeight + 12;
		const maximumTop = headerPixelHeight + chartPixelHeight - 12;
		const overflow = ordered.at(-1)!.top - maximumTop;
		if (overflow > 0) ordered.forEach((tooltip) => (tooltip.top -= overflow));
		const underflow = minimumTop - ordered[0].top;
		if (underflow > 0) ordered.forEach((tooltip) => (tooltip.top += underflow));
		ordered.forEach((tooltip) => (tooltipTops[tooltip.index] = tooltip.top));
	}
	useLayoutEffect(() => {
		const element = chart.current;
		if (!element) return;
		const updateWidth = () => setWidth(Math.max(320, Math.round(element.clientWidth)));
		updateWidth();
		const observer = new ResizeObserver(updateWidth);
		observer.observe(element);
		return () => observer.disconnect();
	}, []);
	useEffect(() => {
		if (!pointerInside || !onZoom) return;
		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.ctrlKey || event.metaKey || event.altKey) return;
			const target = event.target;
			if (target instanceof HTMLInputElement || target instanceof HTMLSelectElement || target instanceof HTMLTextAreaElement) return;
			const direction =
				event.key === "+" || event.key === "=" || event.code === "NumpadAdd"
					? "in"
					: event.key === "-" || event.code === "NumpadSubtract"
						? "out"
						: null;
			if (!direction) return;
			event.preventDefault();
			onZoom(zoomAnchorM, direction);
		};
		window.addEventListener("keydown", handleKeyDown);
		return () => window.removeEventListener("keydown", handleKeyDown);
	}, [onZoom, pointerInside, zoomAnchorM]);
	useEffect(() => {
		const element = plotElement.current;
		if (!element || !onZoom) return;
		const handleWheel = (event: WheelEvent) => {
			if ((!event.ctrlKey && !event.metaKey) || event.deltaY === 0) return;
			event.preventDefault();
			event.stopPropagation();
			const bounds = element.getBoundingClientRect();
			let anchorM = zoomAnchorM;
			if (bounds.width > 0) {
				const pointerX = ((event.clientX - bounds.left) / bounds.width) * width;
				const ratio = Math.min(1, Math.max(0, (pointerX - plot.left) / (width - plot.left - plot.right)));
				anchorM = firstDistance + ratio * distanceRange;
			}
			onZoom(anchorM, event.deltaY < 0 ? "in" : "out");
		};
		element.addEventListener("wheel", handleWheel, { passive: false });
		return () => element.removeEventListener("wheel", handleWheel);
	}, [distanceRange, firstDistance, onZoom, width, zoomAnchorM]);
	const updateCursor = (element: SVGSVGElement, clientX: number) => {
		const bounds = element.getBoundingClientRect();
		const pointerX = ((clientX - bounds.left) / bounds.width) * width;
		const ratio = Math.min(1, Math.max(0, (pointerX - plot.left) / (width - plot.left - plot.right)));
		onCursor(Math.round(ratio * (samples.length - 1)));
	};
	const finishDrag = (element: SVGSVGElement, pointerId: number) => {
		dragClientX.current = null;
		setDragging(false);
		if (element.hasPointerCapture(pointerId)) element.releasePointerCapture(pointerId);
	};
	return (
		<div
			ref={chart}
			className={`trace-chart ${compact ? "trace-chart--compact" : ""} relative min-w-0 overflow-hidden border border-trace-divider bg-trace-surface`}
		>
			<div
				className={`trace-chart__header flex items-center justify-between gap-3 overflow-hidden border-b border-trace-divider px-4 ${compact ? "h-9" : "h-12"}`}
			>
				<span className="trace-chart__label font-mono text-[12px] font-bold tracking-[.1em] text-trace-soft">{label}</span>
				<div className="trace-chart__legend flex min-w-0 items-center gap-4 overflow-hidden font-mono text-[11px] font-bold">
					{series.map((item) => (
						<span className="trace-chart__legend-item flex shrink-0 items-center gap-1.5" key={item.label}>
							<span className="trace-chart__legend-dot size-1.5 rounded-full" style={{ backgroundColor: item.colour }} />
							<span style={{ color: item.colour }}>{item.label}</span>
						</span>
					))}
					{onZoom && (
						<span className="trace-chart__zoom ml-auto flex shrink-0 items-center">
							<button
								type="button"
								onClick={() => onZoom(zoomAnchorM, "out")}
								className="trace-chart__zoom-button grid size-7 place-items-center border border-trace-divider bg-trace-deep text-sm text-trace-muted hover:border-trace-soft hover:text-trace-text"
								aria-label={`Zoom all telemetry out from ${Math.round(zoomAnchorM)} ${distanceUnit}`}
							>
								−
							</button>
							<button
								type="button"
								onClick={() => onZoom(zoomAnchorM, "in")}
								className="trace-chart__zoom-button grid size-7 place-items-center border border-l-0 border-trace-divider bg-trace-deep text-sm text-trace-muted hover:border-trace-soft hover:text-trace-text"
								aria-label={`Zoom all telemetry in around ${Math.round(zoomAnchorM)} ${distanceUnit}`}
							>
								+
							</button>
						</span>
					)}
				</div>
			</div>
			<svg
				ref={plotElement}
				className={`trace-chart__plot block w-full touch-none ${onPan ? (dragging ? "cursor-grabbing" : "cursor-grab") : ""} ${compact ? "h-[82px]" : "h-56"}`}
				viewBox={`0 0 ${width} ${height}`}
				preserveAspectRatio="none"
				role="img"
				aria-label={`${label} telemetry by lap distance`}
				onPointerEnter={() => setPointerInside(true)}
				onPointerLeave={() => setPointerInside(false)}
				onPointerDown={(event) => {
					updateCursor(event.currentTarget, event.clientX);
					if (!onPan || event.button !== 0) return;
					dragClientX.current = event.clientX;
					setDragging(true);
					event.currentTarget.setPointerCapture(event.pointerId);
				}}
				onPointerMove={(event) => {
					updateCursor(event.currentTarget, event.clientX);
					if (!onPan || dragClientX.current == null) return;
					const bounds = event.currentTarget.getBoundingClientRect();
					const plotWidth = Math.max(1, bounds.width * ((width - plot.left - plot.right) / width));
					const deltaPixels = event.clientX - dragClientX.current;
					dragClientX.current = event.clientX;
					onPan((-deltaPixels / plotWidth) * distanceRange);
				}}
				onPointerUp={(event) => finishDrag(event.currentTarget, event.pointerId)}
				onPointerCancel={(event) => finishDrag(event.currentTarget, event.pointerId)}
			>
				{[0, 0.5, 1].map((ratio) => (
					<line
						x1={plot.left}
						x2={width - plot.right}
						y1={plot.top + ratio * (height - plot.top - plot.bottom)}
						y2={plot.top + ratio * (height - plot.top - plot.bottom)}
						className="stroke-trace-divider"
						strokeWidth="1"
						vectorEffect="non-scaling-stroke"
						key={ratio}
					/>
				))}
				{zeroLine && minimum < 0 && maximum > 0 && (
					<line
						x1={plot.left}
						x2={width - plot.right}
						y1={y(0)}
						y2={y(0)}
						className="stroke-trace-dim"
						strokeDasharray="4 4"
						vectorEffect="non-scaling-stroke"
					/>
				)}
				{series.map((item) => (
					<path
						d={comparisonPath(samples, item.value, x, y)}
						fill="none"
						stroke={item.colour}
						strokeWidth="2"
						vectorEffect="non-scaling-stroke"
						key={item.label}
					/>
				))}
				{cursorSample && (
					<line
						x1={x(cursorSample.distanceM)}
						x2={x(cursorSample.distanceM)}
						y1={plot.top}
						y2={height - plot.bottom}
						className="stroke-trace-text"
						strokeWidth="1"
						vectorEffect="non-scaling-stroke"
					/>
				)}
			</svg>
			<span
				className="trace-chart__axis trace-chart__axis--maximum pointer-events-none absolute left-2 font-mono text-[12px] leading-none text-trace-dim"
				style={{ top: `${headerPixelHeight + (plot.top / height) * chartPixelHeight}px` }}
				aria-hidden="true"
			>
				{formatValue(maximum, unit)}
			</span>
			<span
				className="trace-chart__axis trace-chart__axis--minimum pointer-events-none absolute left-2 -translate-y-full font-mono text-[12px] leading-none text-trace-dim"
				style={{ top: `${headerPixelHeight + ((height - plot.bottom) / height) * chartPixelHeight}px` }}
				aria-hidden="true"
			>
				{formatValue(minimum, unit)}
			</span>
			<span
				className="trace-chart__axis trace-chart__axis--start pointer-events-none absolute -translate-y-full font-mono text-[12px] leading-none text-trace-dim"
				style={{ left: `${(plot.left / width) * 100}%`, top: `${headerPixelHeight + ((height - 8) / height) * chartPixelHeight}px` }}
				aria-hidden="true"
			>
				{Math.round(firstDistance)} {distanceUnit}
			</span>
			<span
				className="trace-chart__axis trace-chart__axis--end pointer-events-none absolute -translate-x-full -translate-y-full font-mono text-[12px] leading-none text-trace-dim"
				style={{ left: `${((width - plot.right) / width) * 100}%`, top: `${headerPixelHeight + ((height - 8) / height) * chartPixelHeight}px` }}
				aria-hidden="true"
			>
				{Math.round(lastDistance)} {distanceUnit}
			</span>
			{cursorX != null &&
				tooltipValues.map(({ item, value }, index) => (
					<span
						className="trace-chart__tooltip pointer-events-none absolute z-20 whitespace-nowrap rounded-sm px-2 py-1 font-mono text-[11px] font-black tabular-nums shadow-[0_5px_14px_rgba(0,0,0,.5)]"
						style={{
							left: `${(cursorX / width) * 100}%`,
							top: `${tooltipTops[index]}px`,
							transform: `${tooltipTransform} translateY(-50%)`,
							backgroundColor: item.colour,
							color: chartTooltipTextColour(item.colour),
						}}
						role="status"
						aria-label={`${item.label}: ${formatValue(value, unit)}${unit ? ` ${unit}` : ""}`}
						key={item.label}
					>
						{formatValue(value, unit)}
						{unit ? ` ${unit}` : ""}
					</span>
				))}
		</div>
	);
}

export function comparisonPath(
	samples: LapComparisonSample[],
	value: (sample: LapComparisonSample) => number | null | undefined,
	x: (distance: number) => number,
	y: (value: number) => number,
) {
	let drawing = false;
	return samples.reduce((path, sample) => {
		const current = value(sample);
		if (current == null || !Number.isFinite(current)) {
			drawing = false;
			return path;
		}
		const command = drawing ? "L" : "M";
		drawing = true;
		return `${path}${command}${x(sample.distanceM).toFixed(2)},${y(current).toFixed(2)}`;
	}, "");
}

export function formatChartValue(value: number, unit: string) {
	if (unit === "%" || unit === "" || unit === "rpm" || unit === "km/h" || unit === "°") return String(Math.round(value));
	if (unit === "s") return value.toFixed(3);
	const magnitude = Math.abs(value);
	return magnitude >= 100 ? value.toFixed(0) : magnitude >= 10 ? value.toFixed(1) : value.toFixed(3);
}

export function chartTooltipTextColour(colour: string) {
	if (!colour.startsWith("#") || colour.length !== 7) return "#fff";
	const red = Number.parseInt(colour.slice(1, 3), 16);
	const green = Number.parseInt(colour.slice(3, 5), 16);
	const blue = Number.parseInt(colour.slice(5, 7), 16);
	return red * 0.299 + green * 0.587 + blue * 0.114 > 150 ? "#090b0d" : "#fff";
}

export function formatDelta(value: number) {
	return `${value >= 0 ? "+" : "−"}${Math.abs(value).toFixed(3)} S`;
}
