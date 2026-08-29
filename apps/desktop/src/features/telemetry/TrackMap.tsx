import { useEffect, useId, useRef, useState, type KeyboardEventHandler, type PointerEvent as ReactPointerEvent, type PointerEventHandler } from "react";
import type { CornerAnalysis, LapComparisonSample, TrackMapAsset } from "../../data-source";
import { channelColours } from "./ComparisonChart";

export function useTrackMapPip(active: boolean) {
	const anchor = useRef<HTMLDivElement>(null);
	const [visible, setVisible] = useState(false);
	const [dismissed, setDismissed] = useState(false);

	useEffect(() => {
		const element = anchor.current;
		if (!active || !element) {
			setVisible(false);
			return;
		}
		const observer = new IntersectionObserver(([entry]) => setVisible(!entry.isIntersecting), { rootMargin: "-56px 0px 0px", threshold: 0.08 });
		observer.observe(element);
		return () => observer.disconnect();
	}, [active]);

	return { anchor, visible: visible && !dismissed, dismiss: () => setDismissed(true) };
}

type FloatingMapFrame = { x: number; y: number; width: number; height: number };
type FloatingMapInteraction = { kind: "move" | "resize"; pointerId: number; x: number; y: number; frame: FloatingMapFrame };

const floatingMapFrameKey = "trace.floating-track-map.frame";
const floatingMapMargin = 16;
const floatingMapTop = 64;
const floatingMapMinimumWidth = 340;
const floatingMapMinimumHeight = 260;

function constrainFloatingMapFrame(frame: FloatingMapFrame): FloatingMapFrame {
	const maximumWidth = Math.max(240, window.innerWidth - floatingMapMargin * 2);
	const maximumHeight = Math.max(220, window.innerHeight - floatingMapTop - floatingMapMargin);
	const minimumWidth = Math.min(floatingMapMinimumWidth, maximumWidth);
	const minimumHeight = Math.min(floatingMapMinimumHeight, maximumHeight);
	const width = Math.min(maximumWidth, Math.max(minimumWidth, frame.width));
	const height = Math.min(maximumHeight, Math.max(minimumHeight, frame.height));
	return {
		x: Math.min(Math.max(floatingMapMargin, frame.x), Math.max(floatingMapMargin, window.innerWidth - width - floatingMapMargin)),
		y: Math.min(Math.max(floatingMapTop, frame.y), Math.max(floatingMapTop, window.innerHeight - height - floatingMapMargin)),
		width,
		height,
	};
}

function initialFloatingMapFrame(): FloatingMapFrame {
	const width = Math.min(500, window.innerWidth - floatingMapMargin * 2);
	const fallback = { x: window.innerWidth - width - 24, y: floatingMapTop, width, height: 300 };
	try {
		const stored = JSON.parse(localStorage.getItem(floatingMapFrameKey) ?? "null") as Partial<FloatingMapFrame> | null;
		if (stored && [stored.x, stored.y, stored.width, stored.height].every((value) => typeof value === "number" && Number.isFinite(value))) {
			return constrainFloatingMapFrame(stored as FloatingMapFrame);
		}
	} catch {
		// Ignore stale or malformed display preferences.
	}
	return constrainFloatingMapFrame(fallback);
}

export function FloatingTrackMap({
	samples,
	cursorIndex,
	comparison = false,
	comparisonIsFaster = false,
	trackMap,
	focusSelection = false,
	rangeLabel,
	corners = [],
	selectedCornerIndex = null,
	onRangeZoom,
	rangeZoomLinked = false,
	onRangeZoomLinked,
	onDismiss,
}: {
	samples: LapComparisonSample[];
	cursorIndex: number | null;
	comparison?: boolean;
	comparisonIsFaster?: boolean;
	trackMap?: TrackMapAsset | null;
	focusSelection?: boolean;
	rangeLabel?: string;
	corners?: CornerAnalysis[];
	selectedCornerIndex?: number | null;
	onRangeZoom?: (anchorM: number, direction: "in" | "out") => void;
	rangeZoomLinked?: boolean;
	onRangeZoomLinked?: (linked: boolean) => void;
	onDismiss: () => void;
}) {
	const [frame, setFrame] = useState(initialFloatingMapFrame);
	const interaction = useRef<FloatingMapInteraction | null>(null);

	useEffect(() => {
		const handlePointerMove = (event: PointerEvent) => {
			const active = interaction.current;
			if (!active || active.pointerId !== event.pointerId) return;
			const deltaX = event.clientX - active.x;
			const deltaY = event.clientY - active.y;
			setFrame(
				constrainFloatingMapFrame(
					active.kind === "move"
						? { ...active.frame, x: active.frame.x + deltaX, y: active.frame.y + deltaY }
						: { ...active.frame, width: active.frame.width + deltaX, height: active.frame.height + deltaY },
				),
			);
		};
		const stopInteraction = (event: PointerEvent) => {
			if (interaction.current?.pointerId === event.pointerId) interaction.current = null;
		};
		const keepOnScreen = () => setFrame((current) => constrainFloatingMapFrame(current));
		window.addEventListener("pointermove", handlePointerMove);
		window.addEventListener("pointerup", stopInteraction);
		window.addEventListener("pointercancel", stopInteraction);
		window.addEventListener("resize", keepOnScreen);
		return () => {
			window.removeEventListener("pointermove", handlePointerMove);
			window.removeEventListener("pointerup", stopInteraction);
			window.removeEventListener("pointercancel", stopInteraction);
			window.removeEventListener("resize", keepOnScreen);
		};
	}, []);

	useEffect(() => {
		try {
			localStorage.setItem(floatingMapFrameKey, JSON.stringify(frame));
		} catch {
			// The map remains usable when WebView storage is unavailable.
		}
	}, [frame]);

	const startInteraction = (kind: FloatingMapInteraction["kind"], event: ReactPointerEvent) => {
		if (event.button !== 0) return;
		event.preventDefault();
		interaction.current = { kind, pointerId: event.pointerId, x: event.clientX, y: event.clientY, frame };
	};
	const moveWithKeyboard: KeyboardEventHandler<HTMLButtonElement> = (event) => {
		const step = event.shiftKey ? 40 : 12;
		const movement =
			event.key === "ArrowLeft"
				? [-step, 0]
				: event.key === "ArrowRight"
					? [step, 0]
					: event.key === "ArrowUp"
						? [0, -step]
						: event.key === "ArrowDown"
							? [0, step]
							: null;
		if (!movement) return;
		event.preventDefault();
		setFrame((current) => constrainFloatingMapFrame({ ...current, x: current.x + movement[0], y: current.y + movement[1] }));
	};
	const resizeWithKeyboard: KeyboardEventHandler<HTMLButtonElement> = (event) => {
		const step = event.shiftKey ? 40 : 12;
		const change =
			event.key === "ArrowLeft"
				? [-step, 0]
				: event.key === "ArrowRight"
					? [step, 0]
					: event.key === "ArrowUp"
						? [0, -step]
						: event.key === "ArrowDown"
							? [0, step]
							: null;
		if (!change) return;
		event.preventDefault();
		setFrame((current) => constrainFloatingMapFrame({ ...current, width: current.width + change[0], height: current.height + change[1] }));
	};

	return (
		<aside
			className="fixed z-40 overflow-hidden border border-trace-accent/35 bg-trace-black shadow-[0_18px_55px_rgba(0,0,0,.65)]"
			style={{ left: frame.x, top: frame.y, width: frame.width, height: frame.height }}
			aria-label="Floating track map"
		>
			<TrackMap
				samples={samples}
				cursorIndex={cursorIndex}
				comparison={comparison}
				comparisonIsFaster={comparisonIsFaster}
				height={frame.height - 40}
				trackMap={trackMap}
				focusSelection={focusSelection}
				rangeLabel={rangeLabel}
				corners={corners}
				selectedCornerIndex={selectedCornerIndex}
				onRangeZoom={onRangeZoom}
				rangeZoomLinked={rangeZoomLinked}
				onRangeZoomLinked={onRangeZoomLinked}
				onDismiss={onDismiss}
				onFloatingDragStart={(event) => startInteraction("move", event)}
				onFloatingDragKeyDown={moveWithKeyboard}
			/>
			<button
				type="button"
				className="absolute bottom-0 right-0 z-10 size-5 cursor-se-resize border-0 bg-transparent p-0 text-trace-soft hover:text-trace-accent"
				onPointerDown={(event) => startInteraction("resize", event)}
				onKeyDown={resizeWithKeyboard}
				aria-label="Resize floating track map"
			>
				<svg className="size-full stroke-current" viewBox="0 0 20 20" aria-hidden="true">
					<path d="M19 7 7 19M19 12l-7 7M19 17l-2 2" fill="none" />
				</svg>
			</button>
		</aside>
	);
}

export function closestSampleAtElapsedTime(samples: LapComparisonSample[], key: "referenceElapsedSeconds" | "comparisonElapsedSeconds", targetSeconds: number) {
	return samples.reduce<LapComparisonSample | null>((closest, sample) => {
		const value = sample[key];
		if (value == null || !Number.isFinite(value)) return closest;
		const closestValue = closest?.[key];
		return closestValue == null || Math.abs(value - targetSeconds) < Math.abs(closestValue - targetSeconds) ? sample : closest;
	}, null);
}

export function TrackMap({
	samples,
	cursorIndex,
	comparison = false,
	comparisonIsFaster = false,
	height: requestedHeight,
	trackMap,
	focusSelection = false,
	rangeLabel,
	corners = [],
	selectedCornerIndex = null,
	onRangeZoom,
	rangeZoomLinked = false,
	onRangeZoomLinked,
	onDismiss,
	onFloatingDragStart,
	onFloatingDragKeyDown,
}: {
	samples: LapComparisonSample[];
	cursorIndex: number | null;
	comparison?: boolean;
	comparisonIsFaster?: boolean;
	height?: number;
	trackMap?: TrackMapAsset | null;
	focusSelection?: boolean;
	rangeLabel?: string;
	corners?: CornerAnalysis[];
	selectedCornerIndex?: number | null;
	onRangeZoom?: (anchorM: number, direction: "in" | "out") => void;
	rangeZoomLinked?: boolean;
	onRangeZoomLinked?: (linked: boolean) => void;
	onDismiss?: () => void;
	onFloatingDragStart?: PointerEventHandler<HTMLButtonElement>;
	onFloatingDragKeyDown?: KeyboardEventHandler<HTMLButtonElement>;
}) {
	const [zoom, setZoom] = useState(1);
	const [pan, setPan] = useState({ x: 0, y: 0 });
	const gradientScope = useId().replaceAll(":", "");
	const drag = useRef<{ x: number; y: number; panX: number; panY: number } | null>(null);
	const mapViewport = useRef<HTMLDivElement>(null);
	const spatialZoomAt = useRef<(direction: "in" | "out", anchor: readonly [number, number]) => void>(() => undefined);
	const rangeDistanceAt = useRef<(anchor: readonly [number, number]) => number | null>(() => null);
	const displayHeight = requestedHeight ?? (comparison ? 720 : 600);
	const width = 1_000;
	const height = 700;
	useEffect(() => {
		const element = mapViewport.current;
		if (!element) return;
		const handleWheel = (event: WheelEvent) => {
			event.preventDefault();
			event.stopPropagation();
			event.stopImmediatePropagation();
			const svg = element.querySelector("svg");
			const bounds = svg?.getBoundingClientRect();
			const insideMap =
				bounds != null &&
				event.clientX >= bounds.left &&
				event.clientX <= bounds.right &&
				event.clientY >= bounds.top &&
				event.clientY <= bounds.bottom;
			const anchor =
				insideMap && bounds
					? ([
							((event.clientX - bounds.left) / Math.max(bounds.width, 1)) * width,
							((event.clientY - bounds.top) / Math.max(bounds.height, 1)) * height,
						] as const)
					: ([width / 2, height / 2] as const);
			const direction = event.deltaY < 0 ? "in" : "out";
			if (onRangeZoom && rangeZoomLinked) {
				const cursorDistance = cursorIndex == null ? null : samples[cursorIndex]?.distanceM;
				const anchorDistance = insideMap ? rangeDistanceAt.current(anchor) : null;
				onRangeZoom(anchorDistance ?? cursorDistance ?? ((samples[0]?.distanceM ?? 0) + (samples.at(-1)?.distanceM ?? 0)) / 2, direction);
				return;
			}
			spatialZoomAt.current(direction, anchor);
		};
		element.addEventListener("wheel", handleWheel, { passive: false, capture: true });
		return () => element.removeEventListener("wheel", handleWheel, { capture: true });
	}, [cursorIndex, height, onRangeZoom, rangeZoomLinked, samples, width]);
	useEffect(() => {
		setZoom(1);
		setPan({ x: 0, y: 0 });
	}, [focusSelection]);
	useEffect(() => {
		if (zoom > 1) setPan((current) => (current.x === 0 && current.y === 0 ? current : { x: 0, y: 0 }));
		// A cursor move starts a fresh follow position. Retaining pan calculated
		// around the previous target can place the new target outside the view.
	}, [cursorIndex]);
	const padding = focusSelection ? 90 : 42;
	const drivenPoints = samples.flatMap((sample) =>
		[
			sample.referencePositionXM != null && sample.referencePositionZM != null
				? ([sample.referencePositionXM, sample.referencePositionZM] as const)
				: null,
			comparison && sample.comparisonPositionXM != null && sample.comparisonPositionZM != null
				? ([sample.comparisonPositionXM, sample.comparisonPositionZM] as const)
				: null,
		].filter((point): point is readonly [number, number] => point != null && point.every(Number.isFinite)),
	);
	if (drivenPoints.length < 2)
		return (
			<div className="grid min-h-[340px] place-items-center border border-trace-divider bg-trace-surface p-8 text-center text-[12px] leading-5 text-trace-dim">
				TRACK POSITION WAS NOT RECORDED
				<br />
				FOR THIS LAP
			</div>
		);
	const geometryPoints = trackMap ? [...trackMap.leftBoundary, ...trackMap.rightBoundary].map((point) => [point.xM, point.zM] as const) : [];
	const points = focusSelection && drivenPoints.length > 1 ? drivenPoints : geometryPoints.length > 3 ? geometryPoints : drivenPoints;
	const xs = points.map(([x]) => x);
	const zs = points.map(([, z]) => z);
	const minX = Math.min(...xs);
	const maxX = Math.max(...xs);
	const minZ = Math.min(...zs);
	const maxZ = Math.max(...zs);
	const scale = Math.min((width - padding * 2) / Math.max(maxX - minX, 1), (height - padding * 2) / Math.max(maxZ - minZ, 1));
	const offsetX = (width - (maxX - minX) * scale) / 2;
	const offsetZ = (height - (maxZ - minZ) * scale) / 2;
	// AC's map projection uses world Z directly as downward screen Y (see map.ini).
	const project = (x: number, z: number) => [offsetX + (x - minX) * scale, offsetZ + (z - minZ) * scale] as const;
	const path = (xKey: "referencePositionXM" | "comparisonPositionXM", zKey: "referencePositionZM" | "comparisonPositionZM") =>
		samples.reduce((result, sample) => {
			const x = sample[xKey];
			const z = sample[zKey];
			if (x == null || z == null || !Number.isFinite(x) || !Number.isFinite(z)) return result;
			const [px, py] = project(x, z);
			return `${result}${result ? "L" : "M"}${px.toFixed(2)},${py.toFixed(2)}`;
		}, "");
	const geometryPath = (geometry: TrackMapAsset["centreLine"], close = false) => {
		const result = geometry.reduce((value, point) => {
			const [px, py] = project(point.xM, point.zM);
			return `${value}${value ? "L" : "M"}${px.toFixed(2)},${py.toFixed(2)}`;
		}, "");
		return close && result ? `${result}Z` : result;
	};
	const brakeSegments = (
		prefix: string,
		xKey: "referencePositionXM" | "comparisonPositionXM",
		zKey: "referencePositionZM" | "comparisonPositionZM",
		brakeKey: "referenceBrakePercent" | "comparisonBrakePercent",
		strokeWidth: number,
	) => {
		const intensities: number[] = [];
		samples.forEach((sample, index) => {
			const rawIntensity = Math.min(1, Math.max(0, (sample[brakeKey] ?? 0) / 100));
			if (index === 0) {
				intensities.push(rawIntensity);
				return;
			}
			const distanceM = Math.max(0, sample.distanceM - samples[index - 1].distanceM);
			const previousTrail = index === 1 ? Math.min(1, Math.max(0, (samples[0][brakeKey] ?? 0) / 100)) : intensities[index - 1];
			const fadingTrail = previousTrail * Math.exp(-distanceM / 24);
			intensities.push(Math.max(rawIntensity, fadingTrail));
		});
		const opacity = (intensity: number) => (intensity < 0.01 ? 0 : 0.12 + Math.pow(intensity, 0.62) * 0.88);

		return samples.slice(1).flatMap((sample, offset) => {
			const previous = samples[offset];
			const startIntensity = intensities[offset];
			const endIntensity = intensities[offset + 1];
			const x1 = previous[xKey];
			const z1 = previous[zKey];
			const x2 = sample[xKey];
			const z2 = sample[zKey];
			if (
				Math.max(startIntensity, endIntensity) < 0.01 ||
				x1 == null ||
				z1 == null ||
				x2 == null ||
				z2 == null ||
				![x1, z1, x2, z2].every(Number.isFinite)
			)
				return [];
			const start = project(x1, z1);
			const end = project(x2, z2);
			if (Math.hypot(end[0] - start[0], end[1] - start[1]) < 0.001) return [];
			const gradientId = `${gradientScope}-${prefix}-${offset}`;
			return [
				<g key={`${prefix}-${offset}`}>
					<defs>
						<linearGradient id={gradientId} gradientUnits="userSpaceOnUse" x1={start[0]} y1={start[1]} x2={end[0]} y2={end[1]}>
							<stop offset="0%" stopColor={channelColours.mapBrake} stopOpacity={opacity(startIntensity)} />
							<stop offset="100%" stopColor={channelColours.mapBrake} stopOpacity={opacity(endIntensity)} />
						</linearGradient>
					</defs>
					<line
						x1={start[0]}
						y1={start[1]}
						x2={end[0]}
						y2={end[1]}
						stroke={`url(#${gradientId})`}
						strokeWidth={strokeWidth}
						strokeLinecap="round"
						vectorEffect="non-scaling-stroke"
					/>
				</g>,
			];
		});
	};
	const mapCentre = project((minX + maxX) / 2, (minZ + maxZ) / 2);
	const lineSegments = (line: readonly (readonly [number, number])[]) => line.slice(1).map((point, index) => [line[index], point] as const);
	const projectedGeometry = (line: TrackMapAsset["centreLine"]) => line.map((point) => project(point.xM, point.zM));
	const projectedSamples = (xKey: "referencePositionXM" | "comparisonPositionXM", zKey: "referencePositionZM" | "comparisonPositionZM") =>
		samples.flatMap((sample) => {
			const x = sample[xKey];
			const z = sample[zKey];
			return x == null || z == null || !Number.isFinite(x) || !Number.isFinite(z) ? [] : [project(x, z)];
		});
	const obstructionSegments = trackMap
		? [
				...lineSegments(projectedGeometry(trackMap.leftBoundary)),
				...lineSegments(projectedGeometry(trackMap.rightBoundary)),
				...lineSegments(projectedGeometry(trackMap.centreLine)),
			]
		: [
				...lineSegments(projectedSamples("referencePositionXM", "referencePositionZM")),
				...(comparison ? lineSegments(projectedSamples("comparisonPositionXM", "comparisonPositionZM")) : []),
			];
	const distanceToSegment = (point: readonly [number, number], segment: readonly [readonly [number, number], readonly [number, number]]) => {
		const [start, end] = segment;
		const dx = end[0] - start[0];
		const dy = end[1] - start[1];
		const lengthSquared = dx * dx + dy * dy;
		const position = lengthSquared === 0 ? 0 : Math.min(1, Math.max(0, ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / lengthSquared));
		return Math.hypot(point[0] - (start[0] + dx * position), point[1] - (start[1] + dy * position));
	};
	const labelClearance = (point: readonly [number, number]) =>
		obstructionSegments.length === 0 ? Number.POSITIVE_INFINITY : Math.min(...obstructionSegments.map((segment) => distanceToSegment(point, segment)));
	const labelIsSafe = (point: readonly [number, number]) =>
		point[0] >= 20 && point[0] <= width - 20 && point[1] >= 20 && point[1] <= height - 20 && labelClearance(point) >= 20;
	const cornerLabelPoint = (x: number, z: number) => {
		if (trackMap && trackMap.centreLine.length > 0) {
			const nearestIndex = trackMap.centreLine.reduce((nearest, point, index) => {
				const nearestPoint = trackMap.centreLine[nearest];
				const distance = (point.xM - x) ** 2 + (point.zM - z) ** 2;
				const nearestDistance = (nearestPoint.xM - x) ** 2 + (nearestPoint.zM - z) ** 2;
				return distance < nearestDistance ? index : nearest;
			}, 0);
			const centrePoint = trackMap.centreLine[nearestIndex];
			const nearestBoundary = (boundary: TrackMapAsset["centreLine"]) =>
				boundary.reduce(
					(nearest, point) => ((point.xM - x) ** 2 + (point.zM - z) ** 2 < (nearest.xM - x) ** 2 + (nearest.zM - z) ** 2 ? point : nearest),
					boundary[0],
				);
			const boundaries = [trackMap.leftBoundary, trackMap.rightBoundary].filter((boundary) => boundary.length > 0).map(nearestBoundary);
			if (centrePoint && boundaries.length > 0) {
				const centre = project(centrePoint.xM, centrePoint.zM);
				const directions = boundaries.map((boundary) => {
					const edge = project(boundary.xM, boundary.zM);
					const dx = edge[0] - centre[0];
					const dy = edge[1] - centre[1];
					const magnitude = Math.max(Math.hypot(dx, dy), 0.001);
					return { edge, x: dx / magnitude, y: dy / magnitude };
				});
				for (const offset of [24, 32, 42, 54, 70, 90]) {
					const safe = directions
						.map((direction) => [direction.edge[0] + direction.x * offset, direction.edge[1] + direction.y * offset] as const)
						.filter(labelIsSafe)
						.sort((left, right) => labelClearance(right) - labelClearance(left));
					if (safe[0]) return safe[0];
				}
				return null;
			}
		}
		const apex = project(x, z);
		const dx = apex[0] - mapCentre[0];
		const dy = apex[1] - mapCentre[1];
		const magnitude = Math.max(Math.hypot(dx, dy), 0.001);
		for (const offset of [28, 38, 50, 66, 84, 104]) {
			const candidate = [apex[0] + (dx / magnitude) * offset, apex[1] + (dy / magnitude) * offset] as const;
			if (labelIsSafe(candidate)) return candidate;
		}
		return null;
	};
	const visibleCornerLabels = corners.flatMap((corner) => {
		if (corner.apexDistanceM < (samples[0]?.distanceM ?? 0) || corner.apexDistanceM > (samples.at(-1)?.distanceM ?? 0)) return [];
		const sample = samples.reduce(
			(closest, candidate) =>
				Math.abs(candidate.distanceM - corner.apexDistanceM) < Math.abs(closest.distanceM - corner.apexDistanceM) ? candidate : closest,
			samples[0],
		);
		if (!sample || sample.referencePositionXM == null || sample.referencePositionZM == null) return [];
		const point = cornerLabelPoint(sample.referencePositionXM, sample.referencePositionZM);
		return point ? [{ corner, point }] : [];
	});
	const visibleBrakeMarkers = corners.flatMap((corner) => {
		const values = [
			{ driver: "reference" as const, distanceM: corner.metrics.referenceBrakingPointM },
			...(comparison ? [{ driver: "comparison" as const, distanceM: corner.metrics.comparisonBrakingPointM }] : []),
		];
		return values.flatMap(({ driver, distanceM }) => {
			if (distanceM == null || distanceM < (samples[0]?.distanceM ?? 0) || distanceM > (samples.at(-1)?.distanceM ?? 0)) return [];
			const sample = samples.reduce(
				(closest, candidate) => (Math.abs(candidate.distanceM - distanceM) < Math.abs(closest.distanceM - distanceM) ? candidate : closest),
				samples[0],
			);
			const x = driver === "reference" ? sample?.referencePositionXM : sample?.comparisonPositionXM;
			const z = driver === "reference" ? sample?.referencePositionZM : sample?.comparisonPositionZM;
			return x == null || z == null ? [] : [{ corner, driver, point: project(x, z) }];
		});
	});
	const road = trackMap ? geometryPath([...trackMap.leftBoundary, ...[...trackMap.rightBoundary].reverse()], true) : "";
	const cursor = cursorIndex == null ? null : (samples[cursorIndex] ?? null);
	const comparisonCursorSample =
		comparison && cursor?.referenceElapsedSeconds != null
			? (closestSampleAtElapsedTime(samples, "comparisonElapsedSeconds", cursor.referenceElapsedSeconds) ?? cursor)
			: cursor;
	const referenceCursor =
		cursor?.referencePositionXM != null && cursor.referencePositionZM != null ? project(cursor.referencePositionXM, cursor.referencePositionZM) : null;
	const comparisonCursor =
		comparisonCursorSample?.comparisonPositionXM != null && comparisonCursorSample.comparisonPositionZM != null
			? project(comparisonCursorSample.comparisonPositionXM, comparisonCursorSample.comparisonPositionZM)
			: null;
	const cursorTargets = [referenceCursor, comparisonCursor].filter((point): point is readonly [number, number] => point != null);
	const followedTarget =
		cursorTargets.length === 0
			? null
			: cursorTargets
					.reduce((total, point) => [total[0] + point[0], total[1] + point[1]] as const, [0, 0] as const)
					.map((value) => value / cursorTargets.length);
	const followPan = zoom > 1 && followedTarget ? { x: zoom * (width / 2 - followedTarget[0]), y: zoom * (height / 2 - followedTarget[1]) } : { x: 0, y: 0 };
	const renderedPan = { x: pan.x + followPan.x, y: pan.y + followPan.y };
	const mapPointUnderViewportPoint = (anchor: readonly [number, number]) =>
		[width / 2 + (anchor[0] - renderedPan.x - width / 2) / zoom, height / 2 + (anchor[1] - renderedPan.y - height / 2) / zoom] as const;
	rangeDistanceAt.current = (anchor) => {
		const mapPoint = mapPointUnderViewportPoint(anchor);
		return (
			samples.reduce<{ distanceM: number; squaredDistance: number } | null>((nearest, sample) => {
				const positions = [
					sample.referencePositionXM != null && sample.referencePositionZM != null
						? project(sample.referencePositionXM, sample.referencePositionZM)
						: null,
					comparison && sample.comparisonPositionXM != null && sample.comparisonPositionZM != null
						? project(sample.comparisonPositionXM, sample.comparisonPositionZM)
						: null,
				].filter((point): point is readonly [number, number] => point != null);
				const squaredDistance = Math.min(...positions.map((point) => (point[0] - mapPoint[0]) ** 2 + (point[1] - mapPoint[1]) ** 2));
				return positions.length > 0 && (nearest == null || squaredDistance < nearest.squaredDistance)
					? { distanceM: sample.distanceM, squaredDistance }
					: nearest;
			}, null)?.distanceM ?? null
		);
	};
	spatialZoomAt.current = (direction, anchor) => {
		const nextZoom = Math.min(8, Math.max(1, zoom * (direction === "in" ? 1.15 : 0.87)));
		if (Math.abs(nextZoom - zoom) < 0.000_001) return;
		const ratio = nextZoom / zoom;
		const nextRenderedPan = {
			x: ratio * renderedPan.x + (1 - ratio) * (anchor[0] - width / 2),
			y: ratio * renderedPan.y + (1 - ratio) * (anchor[1] - height / 2),
		};
		const nextFollowPan =
			nextZoom > 1 && followedTarget ? { x: nextZoom * (width / 2 - followedTarget[0]), y: nextZoom * (height / 2 - followedTarget[1]) } : { x: 0, y: 0 };
		setZoom(nextZoom);
		setPan({ x: nextRenderedPan.x - nextFollowPan.x, y: nextRenderedPan.y - nextFollowPan.y });
	};
	const start = samples.find((sample) => sample.referencePositionXM != null && sample.referencePositionZM != null);
	const startPoint =
		start?.referencePositionXM != null && start.referencePositionZM != null ? project(start.referencePositionXM, start.referencePositionZM) : null;
	const resetView = () => {
		setZoom(1);
		setPan({ x: 0, y: 0 });
	};
	const referenceColour = !comparison || comparisonIsFaster ? "var(--color-trace-accent)" : channelColours.faster;
	const comparisonColour = comparisonIsFaster ? channelColours.faster : "var(--color-trace-accent)";
	const mapRangeLabel = focusSelection ? (rangeLabel ?? "SELECTED RANGE") : null;
	const adjustMapZoom = (direction: "in" | "out") => {
		if (onRangeZoom && rangeZoomLinked) {
			const anchor =
				cursor?.distanceM ?? rangeDistanceAt.current([width / 2, height / 2]) ?? ((samples[0]?.distanceM ?? 0) + (samples.at(-1)?.distanceM ?? 0)) / 2;
			onRangeZoom(anchor, direction);
		} else {
			spatialZoomAt.current(direction, [width / 2, height / 2]);
		}
	};
	const mapControls = (
		<div className="ml-auto flex shrink-0 items-center gap-1">
			{onRangeZoom && onRangeZoomLinked && (
				<button
					type="button"
					onClick={() => onRangeZoomLinked(!rangeZoomLinked)}
					className={`inline-flex h-8 items-center justify-center border px-2 font-mono text-[9px] font-bold leading-none ${rangeZoomLinked ? "border-trace-accent/50 bg-trace-accent-wash text-trace-accent" : "border-trace-divider bg-trace-deep text-trace-muted hover:text-trace-text"}`}
					aria-label={rangeZoomLinked ? "Unlink map zoom from telemetry graphs" : "Link map zoom to telemetry graphs"}
				>
					{rangeZoomLinked ? "LINKED" : "MAP"}
				</button>
			)}
			<button
				type="button"
				onClick={() => adjustMapZoom("in")}
				className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-base text-trace-muted hover:text-trace-text"
				aria-label={rangeZoomLinked ? "Zoom all telemetry in" : "Zoom map in"}
			>
				+
			</button>
			<button
				type="button"
				onClick={() => adjustMapZoom("out")}
				className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-base text-trace-muted hover:text-trace-text"
				aria-label={rangeZoomLinked ? "Zoom all telemetry out" : "Zoom map out"}
			>
				−
			</button>
			<button
				type="button"
				onClick={resetView}
				className="inline-flex h-8 items-center justify-center border border-trace-divider bg-trace-deep px-2 font-mono text-[10px] leading-none text-trace-muted hover:text-trace-text"
				aria-label="Reset map pan and spatial zoom"
			>
				RESET
			</button>
			{onDismiss && (
				<button
					type="button"
					onClick={onDismiss}
					className="grid size-8 place-items-center border border-trace-divider bg-trace-deep text-lg leading-none text-trace-muted hover:border-trace-accent/50 hover:text-trace-text"
					aria-label="Dismiss floating track map"
				>
					×
				</button>
			)}
		</div>
	);
	return (
		<div ref={mapViewport} className="min-w-0 overflow-hidden overscroll-contain border border-trace-divider bg-trace-surface">
			{onDismiss ? (
				<div className="flex h-10 min-w-0 items-center overflow-hidden border-b border-trace-divider px-2">
					<button
						type="button"
						onPointerDown={onFloatingDragStart}
						onKeyDown={onFloatingDragKeyDown}
						className="flex h-8 min-w-0 flex-1 cursor-move touch-none items-center gap-2 px-2 text-left font-mono text-[9px] font-bold tracking-[.12em] text-trace-dim hover:text-trace-text active:cursor-grabbing"
						aria-label="Move floating track map"
					>
						<svg className="size-3 shrink-0 fill-current" viewBox="0 0 12 12" aria-hidden="true">
							<circle cx="3" cy="3" r="1" />
							<circle cx="9" cy="3" r="1" />
							<circle cx="3" cy="9" r="1" />
							<circle cx="9" cy="9" r="1" />
						</svg>
						MOVE
					</button>
					{mapControls}
				</div>
			) : (
				<div className="flex min-h-12 min-w-0 flex-wrap items-center gap-x-3 gap-y-2 border-b border-trace-divider px-3 py-2">
					<div className="min-w-0 shrink">
						{mapRangeLabel && <span className="block truncate font-mono text-[10px] font-black text-trace-accent">{mapRangeLabel}</span>}
					</div>
					<div className="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-x-3 gap-y-1 font-mono text-[10px] font-bold text-trace-muted">
						{comparison && (
							<>
								<span className="flex items-center gap-2 whitespace-nowrap">
									<span
										className={`block w-6 border-t-2 ${comparisonIsFaster ? "" : "border-dashed"}`}
										style={{ borderColor: referenceColour }}
									/>
									REFERENCE
								</span>
								<span className="flex items-center gap-2 whitespace-nowrap">
									<span
										className={`block w-6 border-t-2 ${comparisonIsFaster ? "border-dashed" : ""}`}
										style={{ borderColor: comparisonColour }}
									/>
									ANALYSED LAP
								</span>
							</>
						)}
						<span className="flex items-center gap-2 whitespace-nowrap">
							<span className="block h-1.5 w-6" style={{ backgroundColor: channelColours.mapBrake }} />
							BRAKE
						</span>
					</div>
					{mapControls}
				</div>
			)}
			<svg
				className="block w-full cursor-grab touch-none active:cursor-grabbing"
				style={{ height: displayHeight }}
				viewBox={`0 0 ${width} ${height}`}
				role="img"
				aria-label="Recorded path around the track"
				onPointerDown={(event) => {
					event.currentTarget.setPointerCapture(event.pointerId);
					drag.current = { x: event.clientX, y: event.clientY, panX: pan.x, panY: pan.y };
				}}
				onPointerMove={(event) => {
					if (!drag.current) return;
					const bounds = event.currentTarget.getBoundingClientRect();
					setPan({
						x: drag.current.panX + ((event.clientX - drag.current.x) * width) / bounds.width,
						y: drag.current.panY + ((event.clientY - drag.current.y) * height) / bounds.height,
					});
				}}
				onPointerUp={() => {
					drag.current = null;
				}}
				onPointerCancel={() => {
					drag.current = null;
				}}
			>
				<g
					transform={`translate(${renderedPan.x} ${renderedPan.y}) translate(${width / 2} ${height / 2}) scale(${zoom}) translate(${-width / 2} ${-height / 2})`}
				>
					{trackMap && <path d={road} fill="var(--color-trace-deep)" stroke="none" />}
					{trackMap && (
						<path
							d={geometryPath(trackMap.leftBoundary)}
							fill="none"
							stroke="var(--color-trace-soft)"
							strokeWidth="1.5"
							vectorEffect="non-scaling-stroke"
						/>
					)}
					{trackMap && (
						<path
							d={geometryPath(trackMap.rightBoundary)}
							fill="none"
							stroke="var(--color-trace-soft)"
							strokeWidth="1.5"
							vectorEffect="non-scaling-stroke"
						/>
					)}
					{trackMap && (
						<path
							d={geometryPath(trackMap.centreLine)}
							fill="none"
							stroke="var(--color-trace-divider)"
							strokeWidth="1"
							strokeDasharray="5 8"
							vectorEffect="non-scaling-stroke"
						/>
					)}
					<path
						d={path("referencePositionXM", "referencePositionZM")}
						fill="none"
						stroke={referenceColour}
						strokeWidth="3"
						strokeDasharray={comparison && !comparisonIsFaster ? "9 7" : undefined}
						strokeLinecap="round"
						vectorEffect="non-scaling-stroke"
					/>
					{comparison && (
						<path
							d={path("comparisonPositionXM", "comparisonPositionZM")}
							fill="none"
							stroke={comparisonColour}
							strokeWidth="3"
							strokeDasharray={comparisonIsFaster ? "9 7" : undefined}
							strokeLinecap="round"
							vectorEffect="non-scaling-stroke"
						/>
					)}
					{brakeSegments("reference-brake", "referencePositionXM", "referencePositionZM", "referenceBrakePercent", 6)}
					{comparison && brakeSegments("comparison-brake", "comparisonPositionXM", "comparisonPositionZM", "comparisonBrakePercent", 3.5)}
					{visibleBrakeMarkers.map(({ corner, driver, point }) => {
						const selected = corner.index === selectedCornerIndex;
						const colour = driver === "reference" ? referenceColour : comparisonColour;
						return (
							<g transform={`translate(${point[0]} ${point[1]})`} pointerEvents="none" key={`${corner.index}-${driver}-brake`}>
								<circle
									r={selected ? (driver === "reference" ? 7 : 5.5) : driver === "reference" ? 5 : 4}
									fill="var(--color-trace-black)"
									stroke={colour}
									strokeWidth={selected ? 2.5 : 1.75}
									vectorEffect="non-scaling-stroke"
								/>
								{selected && (
									<text y="3" textAnchor="middle" fill={colour} fontFamily="monospace" fontSize="8" fontWeight="900">
										{driver === "reference" ? "R" : "A"}
									</text>
								)}
							</g>
						);
					})}
					{visibleCornerLabels.map(({ corner, point }) => (
						<g transform={`translate(${point[0]} ${point[1]})`} pointerEvents="none" key={corner.index}>
							<circle
								r="15"
								fill="var(--color-trace-black)"
								stroke={corner.index === selectedCornerIndex ? "var(--color-trace-text)" : "var(--color-trace-soft)"}
								strokeWidth={corner.index === selectedCornerIndex ? 2.25 : 1.75}
								vectorEffect="non-scaling-stroke"
							/>
							<text y="4" textAnchor="middle" fill="var(--color-trace-text)" fontFamily="monospace" fontSize="11" fontWeight="900">
								{corner.label}
							</text>
						</g>
					))}
					{startPoint && (
						<g transform={`translate(${startPoint[0]} ${startPoint[1]})`}>
							<line x1="-7" y1="-7" x2="7" y2="7" stroke="#fff" strokeWidth="2" vectorEffect="non-scaling-stroke" />
							<line x1="7" y1="-7" x2="-7" y2="7" stroke="#fff" strokeWidth="2" vectorEffect="non-scaling-stroke" />
						</g>
					)}
					{referenceCursor && (
						<circle
							cx={referenceCursor[0]}
							cy={referenceCursor[1]}
							r="6"
							fill={referenceColour}
							stroke="#101010"
							strokeWidth="2"
							vectorEffect="non-scaling-stroke"
						/>
					)}
					{comparisonCursor && (
						<circle
							cx={comparisonCursor[0]}
							cy={comparisonCursor[1]}
							r="4.5"
							fill={comparisonColour}
							stroke="#101010"
							strokeWidth="2"
							vectorEffect="non-scaling-stroke"
						/>
					)}
				</g>
			</svg>
		</div>
	);
}
