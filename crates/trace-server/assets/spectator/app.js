const sessionId = decodeURIComponent(location.pathname.split("/").pop());
const wsUrl = `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/api/v1/live-sessions/${encodeURIComponent(sessionId)}/spectate`;
const connection = document.querySelector("#connection");
const mapCanvas = document.querySelector("#map");
const traceCanvas = document.querySelector("#trace");
const mapEmpty = document.querySelector("#mapEmpty");
const dvrSeek = document.querySelector("#dvrSeek");
const liveEdge = document.querySelector("#liveEdge");
const workspace = document.querySelector("main");
const traceResizer = document.querySelector("#traceResizer");
const path = [];
let trackGeometry = null;
const mapView = { scale: 1, offsetX: 0, offsetY: 0 };
const history = [];
const telemetryFrames = [];
let latestPosition = null;
let currentLapNumber = null;
const completedLapRows = [];
let retry = 0;
let terminal = false;
let followingLive = true;
let playbackAnchor = null;
let playbackFrame = null;
let renderedBufferTime = null;
const LIVE_EDGE_THRESHOLD_SECONDS = 0.15;
const MAP_MIN_SCALE = 1;
const MAP_MAX_SCALE = 8;
const TRACE_MIN_HEIGHT = 96;
const MAP_MIN_HEIGHT = 220;

const set = (id, value) => {
	document.querySelector(id).textContent = value;
};
const valueOf = (channels, names, fallback = null) => {
	for (const name of names) if (channels[name] != null) return Number(channels[name]);
	return fallback;
};
const percent = (value) => Math.max(0, Math.min(100, (Number(value) || 0) * 100));
const setConnection = (label, kind = "") => {
	connection.className = `connection ${kind}`;
	connection.querySelector("span").textContent = label;
};
const formatGear = (value) => (value === -1 ? "R" : value === 0 ? "N" : Number.isFinite(value) ? String(Math.round(value)) : "—");
const formatTime = (seconds) => {
	if (!Number.isFinite(seconds)) return "—";
	const minutes = Math.floor(seconds / 60);
	return `${minutes}:${(seconds - minutes * 60).toFixed(3).padStart(6, "0")}`;
};
const formatDvrTime = (seconds) => {
	const safe = Math.max(0, Number(seconds) || 0);
	const minutes = Math.floor(safe / 60);
	return `${minutes}:${String(Math.floor(safe % 60)).padStart(2, "0")}`;
};
const channelsOf = (data) => {
	const channels = {};
	for (const channel of data.channels || []) channels[channel.id] = Array.isArray(channel.values) ? channel.values.at(-1) : null;
	return channels;
};
const elapsedOf = (data) => Number(data.base_elapsed_ns || 0) / 1e9;
const traceSample = (data) => {
	const channels = channelsOf(data);
	return {
		time: elapsedOf(data),
		throttle: percent(valueOf(channels, ["driver.throttle"], 0)) / 100,
		brake: percent(valueOf(channels, ["driver.brake"], 0)) / 100,
		speed: Math.max(0, (valueOf(channels, ["vehicle.speed"], 0) || 0) * 3.6),
	};
};
const updateDvrBar = (selectedTime) => {
	const first = telemetryFrames.length ? elapsedOf(telemetryFrames[0]) : 0;
	const last = telemetryFrames.length ? elapsedOf(telemetryFrames.at(-1)) : first;
	const selected = followingLive ? last : Math.max(first, Math.min(last, selectedTime ?? last));
	const duration = Math.max(0, last - first);
	const relative = selected - first;
	const progress = duration > 0 ? relative / duration : 1;
	dvrSeek.value = String(followingLive ? 1 : progress);
	dvrSeek.disabled = telemetryFrames.length < 2;
	dvrSeek.style.setProperty("--dvr-progress", `${followingLive ? 100 : progress * 100}%`);
	set("#dvrTime", `${formatDvrTime(selected - first)} / ${formatDvrTime(last - first)}`);
	liveEdge.classList.toggle("active", followingLive);
};
const sectorTone = (lap, sectorIndex) => {
	const value = lap.sectors?.[sectorIndex];
	if (!Number.isFinite(value)) return "grey";
	const all = completedLapRows.map((candidate) => candidate.sectors?.[sectorIndex]).filter(Number.isFinite);
	if (all.length && value === Math.min(...all)) return "purple";
	const prior = completedLapRows
		.filter((candidate) => candidate.number < lap.number)
		.map((candidate) => candidate.sectors?.[sectorIndex])
		.filter(Number.isFinite);
	if (!prior.length) return "grey";
	return value < Math.min(...prior) ? "green" : "yellow";
};
const sectorMarkup = (lap) => {
	if (!lap.sectors?.some(Number.isFinite)) return "";
	return `<div class="lap-sectors">${lap.sectors
		.map(
			(value, index) =>
				`<span class="lap-sector ${sectorTone(lap, index)}"><i></i><small>S${index + 1} ${Number.isFinite(value) ? Number(value).toFixed(3) : "—"}</small></span>`,
		)
		.join("")}</div>`;
};
const renderLaps = (currentTime, pitLabel = "") => {
	const list = document.querySelector("#lapList");
	const previousScrollTop = list.scrollTop;
	const preserveScroll = previousScrollTop > 2;
	const rows = [...completedLapRows]
		.filter((lap) => currentLapNumber == null || lap.number < currentLapNumber)
		.reverse()
		.map(
			(lap) =>
				`<div class="lap-row"><span class="lap-number">LAP ${lap.number}</span><span class="lap-duration">${formatTime(lap.time)}</span><span class="lap-state">COMPLETE</span>${sectorMarkup(lap)}</div>`,
		)
		.join("");
	const current =
		currentLapNumber == null
			? ""
			: `<div class="lap-row current"><span class="lap-number">LAP ${currentLapNumber}</span><span class="lap-duration">${formatTime(currentTime)}</span><span class="lap-state">${pitLabel || "CURRENT"}</span></div>`;
	list.innerHTML = current + rows || `<div class="lap-empty">Completed laps will appear here.</div>`;
	if (preserveScroll) list.scrollTop = previousScrollTop;
	set("#lapCount", `${completedLapRows.length} COMPLETE`);
};
const resizeCanvas = (canvas) => {
	const box = canvas.getBoundingClientRect();
	const ratio = Math.min(devicePixelRatio || 1, 2);
	const width = Math.max(1, Math.round(box.width * ratio));
	const height = Math.max(1, Math.round(box.height * ratio));
	if (canvas.width !== width || canvas.height !== height) {
		canvas.width = width;
		canvas.height = height;
	}
	return { width, height, ratio };
};
const fitPath = (points, width, height, padding) => {
	const xs = points.map((point) => point.x),
		ys = points.map((point) => point.y);
	const minX = Math.min(...xs),
		maxX = Math.max(...xs),
		minY = Math.min(...ys),
		maxY = Math.max(...ys);
	const rangeX = Math.max(1, maxX - minX),
		rangeY = Math.max(1, maxY - minY);
	const scale = Math.min((width - padding * 2) / rangeX, (height - padding * 2) / rangeY);
	// Match the desktop map's X/Z projection. Canvas already grows downward,
	// so negating Z here mirrors the circuit relative to comparison views.
	return (point) => ({ x: (point.x - (minX + maxX) / 2) * scale + width / 2, y: (point.y - (minY + maxY) / 2) * scale + height / 2 });
};
const updateMapZoomLabel = () => set("#mapZoomValue", `${Math.round(mapView.scale * 100)}%`);
function setMapZoom(nextScale, anchorX = mapCanvas.clientWidth / 2, anchorY = mapCanvas.clientHeight / 2) {
	const scale = Math.max(MAP_MIN_SCALE, Math.min(MAP_MAX_SCALE, nextScale));
	if (scale === mapView.scale) return;
	const ratio = scale / mapView.scale;
	const centreX = mapCanvas.clientWidth / 2;
	const centreY = mapCanvas.clientHeight / 2;
	mapView.offsetX = anchorX - centreX - (anchorX - centreX - mapView.offsetX) * ratio;
	mapView.offsetY = anchorY - centreY - (anchorY - centreY - mapView.offsetY) * ratio;
	mapView.scale = scale;
	updateMapZoomLabel();
	drawMap();
}
function resetMapZoom() {
	mapView.scale = 1;
	mapView.offsetX = 0;
	mapView.offsetY = 0;
	updateMapZoomLabel();
	drawMap();
}
function drawMap() {
	const { width, height, ratio } = resizeCanvas(mapCanvas);
	const context = mapCanvas.getContext("2d");
	context.clearRect(0, 0, width, height);
	const reference = trackGeometry?.centre_line?.length > 2 ? trackGeometry.centre_line : path;
	if (reference.length < 2) return;
	const projectBase = fitPath(reference, width, height, 42 * ratio);
	const project = (point) => {
		const projected = projectBase(point);
		return {
			x: (projected.x - width / 2) * mapView.scale + width / 2 + mapView.offsetX * ratio,
			y: (projected.y - height / 2) * mapView.scale + height / 2 + mapView.offsetY * ratio,
		};
	};
	context.lineJoin = "round";
	context.lineCap = "round";
	const strokePath = (points, color, lineWidth, closed = false) => {
		context.strokeStyle = color;
		context.lineWidth = lineWidth * ratio;
		context.beginPath();
		points.forEach((point, index) => {
			const p = project(point);
			index ? context.lineTo(p.x, p.y) : context.moveTo(p.x, p.y);
		});
		if (closed) context.closePath();
		context.stroke();
	};
	if (trackGeometry) {
		strokePath(trackGeometry.left_boundary, "#777", 2, true);
		strokePath(trackGeometry.right_boundary, "#777", 2, true);
		strokePath(trackGeometry.centre_line, "#303030", 1, true);
	} else {
		strokePath(path, "#303030", 14);
		strokePath(path, "#777", 2);
	}
	if (latestPosition) {
		const marker = project(latestPosition);
		context.fillStyle = "#71df8b";
		context.beginPath();
		context.arc(marker.x, marker.y, 7 * ratio, 0, Math.PI * 2);
		context.fill();
		context.strokeStyle = "#101010";
		context.lineWidth = 3 * ratio;
		context.stroke();
	}
}

mapCanvas.addEventListener(
	"wheel",
	(event) => {
		event.preventDefault();
		const bounds = mapCanvas.getBoundingClientRect();
		const factor = Math.exp(-event.deltaY * 0.0015);
		setMapZoom(mapView.scale * factor, event.clientX - bounds.left, event.clientY - bounds.top);
	},
	{ passive: false },
);
mapCanvas.addEventListener("pointerdown", (event) => {
	if (!event.isPrimary || event.button !== 0) return;
	event.preventDefault();
	const startX = event.clientX;
	const startY = event.clientY;
	const startOffsetX = mapView.offsetX;
	const startOffsetY = mapView.offsetY;
	mapCanvas.setPointerCapture(event.pointerId);
	mapCanvas.classList.add("panning");
	const move = (moveEvent) => {
		mapView.offsetX = startOffsetX + moveEvent.clientX - startX;
		mapView.offsetY = startOffsetY + moveEvent.clientY - startY;
		drawMap();
	};
	const finish = () => {
		mapCanvas.classList.remove("panning");
		mapCanvas.removeEventListener("pointermove", move);
		mapCanvas.removeEventListener("pointerup", finish);
		mapCanvas.removeEventListener("pointercancel", finish);
	};
	mapCanvas.addEventListener("pointermove", move);
	mapCanvas.addEventListener("pointerup", finish);
	mapCanvas.addEventListener("pointercancel", finish);
});
document.querySelector("#mapZoomIn").addEventListener("click", () => setMapZoom(mapView.scale * 1.35));
document.querySelector("#mapZoomOut").addEventListener("click", () => setMapZoom(mapView.scale / 1.35));
document.querySelector("#mapZoomReset").addEventListener("click", resetMapZoom);
mapCanvas.addEventListener("dblclick", resetMapZoom);

const maximumTraceHeight = () => Math.max(TRACE_MIN_HEIGHT, workspace.clientHeight - MAP_MIN_HEIGHT - traceResizer.offsetHeight);
const setTraceHeight = (height) => {
	const next = Math.max(TRACE_MIN_HEIGHT, Math.min(maximumTraceHeight(), height));
	workspace.style.setProperty("--trace-height", `${Math.round(next)}px`);
	drawMap();
	drawTrace();
};
traceResizer.addEventListener("pointerdown", (event) => {
	const startY = event.clientY;
	const startHeight = traceCanvas.getBoundingClientRect().height;
	traceResizer.setPointerCapture(event.pointerId);
	document.body.classList.add("resizing-trace");
	const move = (moveEvent) => setTraceHeight(startHeight - (moveEvent.clientY - startY));
	const finish = () => {
		document.body.classList.remove("resizing-trace");
		traceResizer.removeEventListener("pointermove", move);
		traceResizer.removeEventListener("pointerup", finish);
		traceResizer.removeEventListener("pointercancel", finish);
	};
	traceResizer.addEventListener("pointermove", move);
	traceResizer.addEventListener("pointerup", finish);
	traceResizer.addEventListener("pointercancel", finish);
});
traceResizer.addEventListener("keydown", (event) => {
	if (!["ArrowUp", "ArrowDown"].includes(event.key)) return;
	event.preventDefault();
	const height = traceCanvas.getBoundingClientRect().height;
	setTraceHeight(height + (event.key === "ArrowUp" ? 16 : -16));
});
function drawTrace() {
	const { width, height, ratio } = resizeCanvas(traceCanvas);
	const context = traceCanvas.getContext("2d");
	context.clearRect(0, 0, width, height);
	context.strokeStyle = "#242424";
	context.lineWidth = ratio;
	for (let line = 1; line < 4; line++) {
		const y = (height * line) / 4;
		context.beginPath();
		context.moveTo(0, y);
		context.lineTo(width, y);
		context.stroke();
	}
	if (history.length < 2) return;
	const end = history.at(-1).time,
		start = end - 20;
	const draw = (field, color, scale = 1) => {
		context.strokeStyle = color;
		context.lineWidth = 2 * ratio;
		context.beginPath();
		let started = false;
		for (const sample of history) {
			if (sample.time < start) continue;
			const x = ((sample.time - start) / 20) * width;
			const y = height - Math.max(0, Math.min(1, sample[field] / scale)) * height;
			started ? context.lineTo(x, y) : context.moveTo(x, y);
			started = true;
		}
		context.stroke();
	};
	draw("throttle", "#54ce79");
	draw("brake", "#cc5252");
	draw("speed", "#65a7e8", 350);
}
function lapRow(lapIndex) {
	const number = Math.max(0, Math.round(Number(lapIndex))) + 1;
	let lap = completedLapRows.find((candidate) => candidate.number === number);
	if (!lap) {
		lap = { number, time: null, sectors: [] };
		completedLapRows.push(lap);
		completedLapRows.sort((left, right) => left.number - right.number);
		while (completedLapRows.length > 200) completedLapRows.shift();
	}
	return lap;
}
function recordSectorEvent(event) {
	const lap = lapRow(event.lap_index);
	const sectorIndex = Math.max(0, Math.round(Number(event.sector_index)));
	const duration = Number(event.duration_s);
	if (Number.isFinite(duration)) lap.sectors[sectorIndex] = duration;
}
function recordLapEvent(event) {
	const lap = lapRow(event.lap_index);
	const duration = Number(event.duration_s);
	if (Number.isFinite(duration)) lap.time = duration;
}
function accumulateHiddenTelemetry(data) {
	const channels = channelsOf(data);
	const x = valueOf(channels, ["motion.position.x"]);
	const y = valueOf(channels, ["motion.position.z"]);
	if (Number.isFinite(x) && Number.isFinite(y)) {
		const previous = path.at(-1);
		if (!previous || Math.hypot(previous.x - x, previous.y - y) > 0.5) path.push({ x, y });
		if (path.length > 10000) path.shift();
	}
}
function renderTelemetry(data, accumulate = true) {
	const channels = channelsOf(data);
	const throttle = percent(valueOf(channels, ["driver.throttle"], 0));
	const brake = percent(valueOf(channels, ["driver.brake"], 0));
	const speed = Math.max(0, (valueOf(channels, ["vehicle.speed"], 0) || 0) * 3.6);
	const elapsed = Number(data.base_elapsed_ns || 0) / 1e9;
	const x = valueOf(channels, ["motion.position.x"]),
		y = valueOf(channels, ["motion.position.z"]);
	set("#speed", Math.round(speed));
	set("#gear", formatGear(valueOf(channels, ["vehicle.gear"])));
	set("#rpm", Math.round(valueOf(channels, ["vehicle.engine_rpm"], 0)) || "—");
	const fuel = valueOf(channels, ["vehicle.fuel"]);
	set("#fuel", Number.isFinite(fuel) ? fuel.toFixed(1) : "—");
	const steering = valueOf(channels, ["driver.steering_angle"]);
	const steeringDegrees = Number.isFinite(steering) ? Math.round((steering * 180) / Math.PI) : null;
	set("#steering", steeringDegrees ?? "—");
	document.querySelector("#steeringWheel").style.transform = `rotate(${steeringDegrees ?? 0}deg)`;
	const temp = valueOf(channels, ["environment.track_temperature"]);
	set("#trackTemp", Number.isFinite(temp) ? Math.round(temp) : "—");
	const lapTime = valueOf(channels, ["lap.elapsed"]);
	set("#lapTime", formatTime(lapTime));
	const sector = valueOf(channels, ["lap.sector_index"]);
	set("#sector", Number.isFinite(sector) ? `S${Math.round(sector) + 1}` : "—");
	const progress = valueOf(channels, ["lap.normalized_position"]);
	set("#lapProgress", Number.isFinite(progress) ? `${Math.round(progress * 100)}%` : "—");
	const completedLaps = valueOf(channels, ["lap.completed_laps"]);
	const inPit = valueOf(channels, ["session.in_pit"]) > 0;
	const inPitLane = valueOf(channels, ["session.in_pit_lane"]) > 0;
	const pitLabel = inPit ? "IN PIT" : inPitLane ? "PIT LANE" : "";
	const pitStatus = document.querySelector("#pitStatus");
	pitStatus.textContent = pitLabel;
	pitStatus.classList.toggle("visible", Boolean(pitLabel));
	if (Number.isFinite(completedLaps)) {
		const count = Math.max(0, Math.round(completedLaps));
		currentLapNumber = count + 1;
	}
	renderLaps(lapTime, pitLabel);
	set("#throttle", `${Math.round(throttle)}%`);
	set("#brake", `${Math.round(brake)}%`);
	document.querySelector("#throttleBar").style.width = `${throttle}%`;
	document.querySelector("#brakeBar").style.width = `${brake}%`;
	if (accumulate) {
		history.push({ time: elapsed, throttle: throttle / 100, brake: brake / 100, speed });
		while (history.length && history[0].time < elapsed - 21) history.shift();
	}
	if (Number.isFinite(x) && Number.isFinite(y)) {
		latestPosition = { x, y };
		if (accumulate) {
			const previous = path.at(-1);
			if (!previous || Math.hypot(previous.x - x, previous.y - y) > 0.5) path.push(latestPosition);
			if (path.length > 10000) path.shift();
		}
		mapEmpty.hidden = Boolean(trackGeometry) || path.length > 1;
		set("#mapSamples", trackGeometry ? `${trackGeometry.centre_line.length} STATIC POINTS` : `${path.length} DRIVEN POINTS`);
	}
	drawMap();
	drawTrace();
}
function renderBufferedAt(targetTime) {
	if (!telemetryFrames.length) return;
	let selectedIndex = telemetryFrames.length - 1;
	for (let index = telemetryFrames.length - 1; index >= 0; index--) {
		if (elapsedOf(telemetryFrames[index]) <= targetTime) {
			selectedIndex = index;
			break;
		}
	}
	const selectedTime = elapsedOf(telemetryFrames[selectedIndex]);
	if (selectedTime !== renderedBufferTime) {
		history.length = 0;
		for (let index = selectedIndex; index >= 0; index--) {
			const sample = traceSample(telemetryFrames[index]);
			if (sample.time < selectedTime - 20) break;
			history.unshift(sample);
		}
		renderTelemetry(telemetryFrames[selectedIndex], false);
		renderedBufferTime = selectedTime;
	}
	updateDvrBar(targetTime);
}
const dvrStartTime = () => (telemetryFrames.length ? elapsedOf(telemetryFrames[0]) : 0);
const dvrDuration = () => (telemetryFrames.length ? elapsedOf(telemetryFrames.at(-1)) - dvrStartTime() : 0);
const currentPlaybackTime = () =>
	playbackAnchor ? playbackAnchor.mediaTime + (performance.now() - playbackAnchor.wallTime) / 1000 : dvrStartTime() + Number(dvrSeek.value) * dvrDuration();
const playbackTick = () => {
	playbackFrame = null;
	if (followingLive || !playbackAnchor || !telemetryFrames.length) return;
	const first = dvrStartTime();
	const last = elapsedOf(telemetryFrames.at(-1));
	let target = currentPlaybackTime();
	if (target < first) {
		target = first;
		playbackAnchor = { mediaTime: first, wallTime: performance.now() };
	}
	if (!terminal && target >= last - LIVE_EDGE_THRESHOLD_SECONDS) {
		jumpToLive();
		return;
	}
	renderBufferedAt(Math.min(target, last));
	if (!(terminal && target >= last)) playbackFrame = requestAnimationFrame(playbackTick);
};
const playFrom = (targetTime) => {
	followingLive = false;
	playbackAnchor = { mediaTime: targetTime, wallTime: performance.now() };
	renderedBufferTime = null;
	renderBufferedAt(targetTime);
	if (playbackFrame == null) playbackFrame = requestAnimationFrame(playbackTick);
};
dvrSeek.addEventListener("input", () => {
	playFrom(dvrStartTime() + Number(dvrSeek.value) * dvrDuration());
});
function jumpToLive() {
	followingLive = true;
	playbackAnchor = null;
	if (playbackFrame != null) cancelAnimationFrame(playbackFrame);
	playbackFrame = null;
	renderedBufferTime = null;
	if (telemetryFrames.length) renderBufferedAt(elapsedOf(telemetryFrames.at(-1)));
	else updateDvrBar(0);
}
liveEdge.addEventListener("click", jumpToLive);
function connect() {
	const socket = new WebSocket(wsUrl);
	socket.onopen = () => {
		retry = 0;
		setConnection("Live", "live");
	};
	socket.onerror = () => setConnection("Reconnecting", "warn");
	socket.onclose = () => {
		if (terminal) return;
		setConnection("Reconnecting", "warn");
		setTimeout(connect, Math.min(10000, 500 * 2 ** retry++));
	};
	socket.onmessage = (event) => {
		let message;
		try {
			message = JSON.parse(event.data);
		} catch {
			return;
		}
		const payload = message.payload || {};
		if (payload.type === "session_state") {
			const state = payload.data || {};
			set("#driver", state.driver_name || "Unknown driver");
			set("#sim", state.simulator_name || state.simulator || "—");
			set("#simMark", state.simulator_mark || "SIM");
			set("#track", state.track || "Unknown track");
			set("#car", state.car || "—");
			set("#session", formatSessionType(state.session_type));
			if (state.status === "ended") {
				terminal = true;
				setConnection("Ended");
			}
		} else if (payload.type === "track_geometry") {
			const geometry = payload.data || {};
			const convert = (points) =>
				(points || [])
					.map((point) => ({ x: Number(point.x_m), y: Number(point.z_m) }))
					.filter((point) => Number.isFinite(point.x) && Number.isFinite(point.y));
			const candidate = {
				centre_line: convert(geometry.centre_line),
				left_boundary: convert(geometry.left_boundary),
				right_boundary: convert(geometry.right_boundary),
			};
			if (
				candidate.centre_line.length > 2 &&
				candidate.left_boundary.length === candidate.centre_line.length &&
				candidate.right_boundary.length === candidate.centre_line.length
			) {
				trackGeometry = candidate;
				mapEmpty.hidden = true;
				set("#mapSamples", `${candidate.centre_line.length} STATIC POINTS`);
				drawMap();
			}
		} else if (payload.type === "telemetry_batch") {
			const data = payload.data || {};
			telemetryFrames.push(data);
			if (telemetryFrames.length > 2400) telemetryFrames.shift();
			if (followingLive) {
				renderTelemetry(data);
				renderedBufferTime = elapsedOf(data);
				updateDvrBar(elapsedOf(data));
			} else {
				accumulateHiddenTelemetry(data);
				updateDvrBar(Math.min(currentPlaybackTime(), elapsedOf(data)));
			}
		} else if (payload.type === "sector_event") {
			recordSectorEvent(payload.data || {});
		} else if (payload.type === "lap_event") {
			recordLapEvent(payload.data || {});
		} else if (payload.type === "end") {
			terminal = true;
			setConnection("Ended");
		}
	};
}

function formatSessionType(value) {
	const type = String(value || "")
		.trim()
		.toLocaleLowerCase();
	if (!type) return "—";
	if (type === "session") return "Drive";
	if (type === "qualify") return "Qualifying";
	if (type.includes("replay")) return "Replay";
	return type
		.split(/[\s_-]+/)
		.filter(Boolean)
		.map((part) => part.charAt(0).toLocaleUpperCase() + part.slice(1))
		.join(" ");
}
new ResizeObserver(() => {
	drawMap();
	drawTrace();
}).observe(document.querySelector("main"));
connect();
