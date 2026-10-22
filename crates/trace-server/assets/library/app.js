const state = {
	instance: null,
	account: null,
	sessions: [],
	review: null,
	telemetry: null,
};

const elements = {
	loading: document.querySelector("#loading-view"),
	auth: document.querySelector("#auth-view"),
	library: document.querySelector("#library-view"),
	libraryHome: document.querySelector("#library-home"),
	reviewView: document.querySelector("#review-view"),
	instanceName: document.querySelector("#instance-name"),
	instanceVersion: document.querySelector("#instance-version"),
	accountName: document.querySelector("#account-name"),
	loginTab: document.querySelector("#login-tab"),
	registerTab: document.querySelector("#register-tab"),
	loginForm: document.querySelector("#login-form"),
	registerForm: document.querySelector("#register-form"),
	logout: document.querySelector("#logout-button"),
	input: document.querySelector("#package-input"),
	uploadZone: document.querySelector("#upload-zone"),
	uploadLimit: document.querySelector("#upload-limit"),
	search: document.querySelector("#session-search"),
	list: document.querySelector("#session-list"),
	count: document.querySelector("#session-count"),
	empty: document.querySelector("#empty-library"),
	reviewBack: document.querySelector("#review-back"),
	reviewDownload: document.querySelector("#review-download"),
	reviewKicker: document.querySelector("#review-kicker"),
	reviewName: document.querySelector("#review-name"),
	reviewContext: document.querySelector("#review-context"),
	reviewBest: document.querySelector("#review-best"),
	reviewLapCount: document.querySelector("#review-lap-count"),
	reviewLaps: document.querySelector("#review-laps"),
	telemetryEmpty: document.querySelector("#telemetry-empty"),
	telemetryContent: document.querySelector("#telemetry-content"),
	telemetryLapLabel: document.querySelector("#telemetry-lap-label"),
	telemetryLapTime: document.querySelector("#telemetry-lap-time"),
	telemetryStats: document.querySelector("#telemetry-stats"),
	trackCanvas: document.querySelector("#track-canvas"),
	speedCanvas: document.querySelector("#speed-canvas"),
	inputCanvas: document.querySelector("#input-canvas"),
	toasts: document.querySelector("#toast-region"),
};

async function request(path, options = {}) {
	const response = await fetch(path, { credentials: "same-origin", ...options });
	if (response.status === 204) return null;
	const contentType = response.headers.get("content-type") || "";
	const body = contentType.includes("application/json") ? await response.json() : null;
	if (!response.ok) throw new Error(body?.error || `Request failed (${response.status})`);
	return body;
}

function show(view) {
	elements.loading.hidden = view !== "loading";
	elements.auth.hidden = view !== "auth";
	elements.library.hidden = view !== "library";
}

function toast(message, kind = "success") {
	const item = document.createElement("div");
	item.className = `toast ${kind}`;
	item.textContent = message;
	elements.toasts.append(item);
	setTimeout(() => item.remove(), 4500);
}

function setBusy(form, busy) {
	for (const control of form.elements) control.disabled = busy;
}

function switchAuth(mode) {
	const registering = mode === "register";
	elements.loginTab.classList.toggle("active", !registering);
	elements.registerTab.classList.toggle("active", registering);
	elements.loginForm.hidden = registering;
	elements.registerForm.hidden = !registering;
}

async function loadAccount() {
	try {
		state.account = await request("/api/v1/auth/session");
		await loadLibrary();
		show("library");
	} catch {
		state.account = null;
		show("auth");
	}
}

async function loadLibrary() {
	state.sessions = await request("/api/v1/library/sessions");
	elements.accountName.textContent = state.account.display_name;
	renderSessions();
}

function formatBytes(bytes) {
	if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}

function formatDate(value) {
	const date = new Date(value);
	if (Number.isNaN(date.valueOf())) return value || "Unknown date";
	return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

function formatDuration(nanoseconds) {
	if (nanoseconds == null) return "—";
	const milliseconds = Math.round(nanoseconds / 1_000_000);
	const minutes = Math.floor(milliseconds / 60_000);
	const seconds = Math.floor((milliseconds % 60_000) / 1000);
	const remainder = milliseconds % 1000;
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(remainder).padStart(3, "0")}`;
}

function readable(value, fallback) {
	return value && value.trim() ? value : fallback;
}

function sessionSearchText(session) {
	return [session.title, session.driver_name, session.simulator, session.track, session.layout, session.car, session.session_type, ...(session.tags || [])]
		.filter(Boolean)
		.join(" ")
		.toLocaleLowerCase();
}

function renderSessions() {
	const query = elements.search.value.trim().toLocaleLowerCase();
	const sessions = state.sessions.filter((session) => sessionSearchText(session).includes(query));
	elements.list.replaceChildren(...sessions.map(sessionRow));
	elements.count.textContent = query ? `${sessions.length}/${state.sessions.length}` : String(state.sessions.length);
	elements.empty.hidden = state.sessions.length !== 0 || Boolean(query);
}

function sessionRow(session) {
	const row = document.createElement("article");
	row.className = "session-row";
	row.tabIndex = 0;
	row.setAttribute("role", "button");
	row.addEventListener("click", () => openReview(session.session_id));
	row.addEventListener("keydown", (event) => {
		if (event.key === "Enter" || event.key === " ") {
			event.preventDefault();
			openReview(session.session_id);
		}
	});

	const primary = document.createElement("div");
	primary.className = "session-primary";
	const meta = document.createElement("div");
	meta.className = "session-meta";
	meta.textContent = `${readable(session.session_type, "Session")} · ${session.lap_count} ${session.lap_count === 1 ? "lap" : "laps"}`;
	const title = document.createElement("strong");
	title.textContent = readable(session.title, readable(session.driver_name, "Unnamed driver"));
	const date = document.createElement("small");
	date.textContent = formatDate(session.started_at);
	primary.append(meta, title, date);

	const context = document.createElement("div");
	context.className = "session-context";
	const track = document.createElement("strong");
	track.textContent = readable(session.track, "Unknown track");
	const car = document.createElement("small");
	car.textContent = `${readable(session.car, "Unknown car")} · ${readable(session.simulator, "Unknown simulator")} · ${formatBytes(session.size_bytes)}`;
	context.append(track, car);

	const actions = document.createElement("div");
	actions.className = "session-actions";
	const download = document.createElement("a");
	download.href = `/api/v1/library/sessions/${encodeURIComponent(session.session_id)}/download`;
	download.textContent = "Download";
	download.addEventListener("click", (event) => event.stopPropagation());
	const remove = document.createElement("button");
	remove.className = "danger";
	remove.type = "button";
	remove.textContent = "Delete";
	remove.addEventListener("click", (event) => {
		event.stopPropagation();
		deleteSession(session, remove);
	});
	actions.append(download, remove);

	row.append(primary, context, actions);
	return row;
}

async function openReview(sessionId) {
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading session…";
	try {
		state.review = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/review`);
		state.telemetry = null;
		renderReview();
		const preferred = [...state.review.laps].filter((lap) => lap.duration_ns != null).sort((left, right) => left.duration_ns - right.duration_ns)[0];
		if (preferred) await loadLap(preferred.id);
	} catch (error) {
		closeReview();
		toast(error.message, "error");
	}
}

function closeReview() {
	state.review = null;
	state.telemetry = null;
	elements.reviewView.hidden = true;
	elements.libraryHome.hidden = false;
}

function renderReview() {
	const { summary, laps } = state.review;
	elements.reviewDownload.href = `/api/v1/library/sessions/${encodeURIComponent(summary.session_id)}/download`;
	elements.reviewKicker.textContent = `${readable(summary.session_type, "Session")} · ${readable(summary.simulator, "Unknown simulator")}`;
	elements.reviewName.textContent = readable(summary.title, readable(summary.driver_name, "Recorded session"));
	elements.reviewContext.textContent = `${readable(summary.track, "Unknown track")} · ${readable(summary.car, "Unknown car")} · ${formatDate(summary.started_at)}`;
	elements.reviewLapCount.textContent = `${laps.length} ${laps.length === 1 ? "lap" : "laps"}`;
	const timed = laps.filter((lap) => lap.duration_ns != null);
	const best = timed.length ? timed.reduce((fastest, lap) => (lap.duration_ns < fastest.duration_ns ? lap : fastest)) : null;
	elements.reviewBest.hidden = !best;
	if (best) elements.reviewBest.querySelector("strong").textContent = formatDuration(best.duration_ns);
	elements.reviewLaps.replaceChildren(...laps.map((lap) => lapRow(lap, best?.id)));
	elements.telemetryEmpty.textContent = laps.length ? "Select a recorded lap to review its telemetry." : "This package contains no recorded laps.";
}

function lapRow(lap, bestId) {
	const button = document.createElement("button");
	button.className = "lap-row";
	button.type = "button";
	button.dataset.lapId = lap.id;
	const identity = document.createElement("span");
	identity.className = "lap-identity";
	const label = document.createElement("small");
	label.textContent = `LAP ${lap.index}`;
	const time = document.createElement("strong");
	time.textContent = formatDuration(lap.duration_ns);
	identity.append(label, time);
	const sectors = document.createElement("span");
	sectors.className = "sector-list";
	if (lap.sectors.length) {
		for (const sector of lap.sectors) {
			const value = document.createElement("span");
			value.textContent = `S${sector.index} ${formatDuration(sector.duration_ns)}`;
			sectors.append(value);
		}
	} else {
		sectors.textContent = "No sectors";
	}
	if (lap.id === bestId) button.classList.add("best");
	if (lap.validity !== "valid") button.classList.add("invalid");
	button.append(identity, sectors);
	button.addEventListener("click", () => loadLap(lap.id));
	return button;
}

async function loadLap(lapId) {
	if (!state.review) return;
	for (const button of elements.reviewLaps.querySelectorAll(".lap-row")) button.classList.toggle("active", button.dataset.lapId === lapId);
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading lap telemetry…";
	try {
		const sessionId = state.review.summary.session_id;
		state.telemetry = await request(
			`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/laps/${encodeURIComponent(lapId)}/telemetry?max_points=1800`,
		);
		const lap = state.review.laps.find((candidate) => candidate.id === lapId);
		renderTelemetry(lap);
	} catch (error) {
		elements.telemetryEmpty.textContent = error.message;
		toast(error.message, "error");
	}
}

function renderTelemetry(lap) {
	const points = state.telemetry.points;
	elements.telemetryEmpty.hidden = true;
	elements.telemetryContent.hidden = false;
	elements.telemetryLapLabel.textContent = `LAP ${lap.index}`;
	elements.telemetryLapTime.textContent = formatDuration(lap.duration_ns);
	const maximumSpeed = Math.max(0, ...points.map((point) => point.speed_kph || 0));
	const maximumRpm = Math.max(0, ...points.map((point) => point.engine_rpm || 0));
	elements.telemetryStats.replaceChildren(stat(`${Math.round(maximumSpeed)} km/h`, "MAX SPEED"), stat(`${Math.round(maximumRpm)} rpm`, "MAX RPM"));
	requestAnimationFrame(drawTelemetry);
}

function stat(value, label) {
	const item = document.createElement("span");
	const strong = document.createElement("strong");
	strong.textContent = value;
	const small = document.createElement("small");
	small.textContent = label;
	item.append(strong, small);
	return item;
}

function canvasContext(canvas) {
	const rectangle = canvas.getBoundingClientRect();
	const ratio = window.devicePixelRatio || 1;
	canvas.width = Math.max(1, Math.round(rectangle.width * ratio));
	canvas.height = Math.max(1, Math.round(rectangle.height * ratio));
	const context = canvas.getContext("2d");
	context.scale(ratio, ratio);
	context.lineJoin = "round";
	context.lineCap = "round";
	return { context, width: rectangle.width, height: rectangle.height };
}

function drawTelemetry() {
	if (!state.telemetry || elements.telemetryContent.hidden) return;
	drawTrack(elements.trackCanvas, state.telemetry.points);
	drawGraph(elements.speedCanvas, state.telemetry.points, [{ key: "speed_kph", color: "#65a9ff" }]);
	drawGraph(elements.inputCanvas, state.telemetry.points, [
		{ key: "throttle", color: "#68e997", maximum: 1 },
		{ key: "brake", color: "#ef6969", maximum: 1 },
	]);
}

function drawTrack(canvas, points) {
	const { context, width, height } = canvasContext(canvas);
	const positions = points.filter((point) => Number.isFinite(point.position_x_m) && Number.isFinite(point.position_z_m));
	if (positions.length < 2) {
		context.fillStyle = "#777";
		context.font = "12px system-ui";
		context.textAlign = "center";
		context.fillText("TRACK POSITION NOT RECORDED", width / 2, height / 2);
		return;
	}
	const xs = positions.map((point) => point.position_x_m);
	const zs = positions.map((point) => point.position_z_m);
	const minX = Math.min(...xs);
	const maxX = Math.max(...xs);
	const minZ = Math.min(...zs);
	const maxZ = Math.max(...zs);
	const padding = 24;
	const scale = Math.min((width - padding * 2) / Math.max(maxX - minX, 1), (height - padding * 2) / Math.max(maxZ - minZ, 1));
	const offsetX = (width - (maxX - minX) * scale) / 2;
	const offsetZ = (height - (maxZ - minZ) * scale) / 2;
	context.strokeStyle = "#d5d5d5";
	context.lineWidth = 3;
	context.beginPath();
	positions.forEach((point, index) => {
		const x = offsetX + (point.position_x_m - minX) * scale;
		const y = offsetZ + (point.position_z_m - minZ) * scale;
		if (index === 0) context.moveTo(x, y);
		else context.lineTo(x, y);
	});
	context.stroke();
}

function drawGraph(canvas, points, series) {
	const { context, width, height } = canvasContext(canvas);
	const padding = 12;
	context.strokeStyle = "#292929";
	context.lineWidth = 1;
	for (let line = 1; line < 4; line += 1) {
		const y = padding + ((height - padding * 2) * line) / 4;
		context.beginPath();
		context.moveTo(0, y);
		context.lineTo(width, y);
		context.stroke();
	}
	for (const item of series) {
		const maximum = item.maximum || Math.max(1, ...points.map((point) => point[item.key] || 0));
		context.strokeStyle = item.color;
		context.lineWidth = 2;
		context.beginPath();
		let started = false;
		points.forEach((point, index) => {
			const value = point[item.key];
			if (!Number.isFinite(value)) return;
			const x = points.length === 1 ? 0 : (index / (points.length - 1)) * width;
			const y = height - padding - (Math.max(0, value) / maximum) * (height - padding * 2);
			if (started) context.lineTo(x, y);
			else context.moveTo(x, y);
			started = true;
		});
		context.stroke();
	}
}

async function deleteSession(session, button) {
	const previous = button.textContent;
	if (previous !== "Confirm") {
		button.textContent = "Confirm";
		setTimeout(() => {
			if (button.isConnected && button.textContent === "Confirm") button.textContent = previous;
		}, 3000);
		return;
	}
	button.disabled = true;
	try {
		await request(`/api/v1/library/sessions/${encodeURIComponent(session.session_id)}`, { method: "DELETE" });
		state.sessions = state.sessions.filter((item) => item.session_id !== session.session_id);
		renderSessions();
		toast("Session deleted.");
	} catch (error) {
		button.disabled = false;
		button.textContent = "Delete";
		toast(error.message, "error");
	}
}

async function upload(file) {
	if (!file) return;
	if (!file.name.toLocaleLowerCase().endsWith(".trace")) {
		toast("Choose a .trace package.", "error");
		return;
	}
	elements.uploadZone.classList.add("dragging");
	try {
		const session = await request("/api/v1/library/sessions", {
			method: "POST",
			headers: { "content-type": "application/vnd.simtrace.session" },
			body: file,
		});
		state.sessions.unshift(session);
		renderSessions();
		toast("Session uploaded to your private library.");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		elements.uploadZone.classList.remove("dragging");
		elements.input.value = "";
	}
}

elements.loginTab.addEventListener("click", () => switchAuth("login"));
elements.registerTab.addEventListener("click", () => switchAuth("register"));
elements.reviewBack.addEventListener("click", closeReview);
elements.search.addEventListener("input", renderSessions);
elements.input.addEventListener("change", () => upload(elements.input.files[0]));
elements.uploadZone.addEventListener("dragover", (event) => {
	event.preventDefault();
	elements.uploadZone.classList.add("dragging");
});
elements.uploadZone.addEventListener("dragleave", () => elements.uploadZone.classList.remove("dragging"));
elements.uploadZone.addEventListener("drop", (event) => {
	event.preventDefault();
	elements.uploadZone.classList.remove("dragging");
	upload(event.dataTransfer.files[0]);
});

elements.loginForm.addEventListener("submit", async (event) => {
	event.preventDefault();
	setBusy(elements.loginForm, true);
	const values = Object.fromEntries(new FormData(elements.loginForm));
	try {
		state.account = await request("/api/v1/auth/sessions", {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify(values),
		});
		elements.loginForm.reset();
		await loadLibrary();
		show("library");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		setBusy(elements.loginForm, false);
	}
});

elements.registerForm.addEventListener("submit", async (event) => {
	event.preventDefault();
	setBusy(elements.registerForm, true);
	const values = Object.fromEntries(new FormData(elements.registerForm));
	try {
		await request("/api/v1/accounts", {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify(values),
		});
		state.account = await request("/api/v1/auth/sessions", {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ username: values.username, password: values.password }),
		});
		elements.registerForm.reset();
		await loadLibrary();
		show("library");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		setBusy(elements.registerForm, false);
	}
});

elements.logout.addEventListener("click", async () => {
	try {
		await request("/api/v1/auth/session", { method: "DELETE" });
		state.account = null;
		state.sessions = [];
		closeReview();
		show("auth");
	} catch (error) {
		toast(error.message, "error");
	}
});

const telemetryResizeObserver = new ResizeObserver(() => requestAnimationFrame(drawTelemetry));
telemetryResizeObserver.observe(elements.trackCanvas);
telemetryResizeObserver.observe(elements.speedCanvas);
telemetryResizeObserver.observe(elements.inputCanvas);

async function start() {
	show("loading");
	try {
		state.instance = await request("/api/v1/instance");
		elements.instanceName.textContent = state.instance.name;
		elements.instanceVersion.textContent = `v${state.instance.version}`;
		document.title = `${state.instance.name} · TRACE Library`;
		elements.uploadLimit.textContent = `up to ${formatBytes(state.instance.limits.maximum_upload_bytes)}`;
		const registrationOpen = state.instance.registration_policy === "open";
		elements.registerTab.hidden = !registrationOpen;
		await loadAccount();
	} catch (error) {
		elements.loading.querySelector("p").textContent = error.message;
		toast("Could not connect to this TRACE instance.", "error");
	}
}

start();
