const state = {
	instance: null,
	account: null,
	sessions: [],
	review: null,
	telemetry: null,
	selectedLap: null,
	selectedLapId: null,
	comparison: null,
	comparePickerOpen: false,
	publicShareId: null,
	shareUrl: null,
	tokens: [],
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
	connectDesktop: document.querySelector("#connect-desktop"),
	instanceSettings: document.querySelector("#instance-settings"),
	adminPanel: document.querySelector("#admin-panel"),
	tokenPanel: document.querySelector("#token-panel"),
	tokenLabel: document.querySelector("#token-label"),
	tokenCreate: document.querySelector("#token-create"),
	tokenSecret: document.querySelector("#token-secret"),
	tokenValue: document.querySelector("#token-value"),
	tokenCopy: document.querySelector("#token-copy"),
	tokenList: document.querySelector("#token-list"),
	sharePanel: document.querySelector("#share-panel"),
	shareUrl: document.querySelector("#share-url"),
	shareCopy: document.querySelector("#share-copy"),
	shareRevoke: document.querySelector("#share-revoke"),
	comparisonSummary: document.querySelector("#comparison-summary"),
	comparisonClear: document.querySelector("#comparison-clear"),
	alignmentLabel: document.querySelector("#alignment-label"),
	telemetryEmpty: document.querySelector("#telemetry-empty"),
	telemetryContent: document.querySelector("#telemetry-content"),
	telemetryLapLabel: document.querySelector("#telemetry-lap-label"),
	telemetryLapTime: document.querySelector("#telemetry-lap-time"),
	telemetryStats: document.querySelector("#telemetry-stats"),
	trackCanvas: document.querySelector("#track-canvas"),
	speedCanvas: document.querySelector("#speed-canvas"),
	throttleCanvas: document.querySelector("#throttle-canvas"),
	brakeCanvas: document.querySelector("#brake-canvas"),
	deltaCanvas: document.querySelector("#delta-canvas"),
	deltaCard: document.querySelector("#delta-card"),
	toasts: document.querySelector("#toast-region"),
};

async function request(path, options = {}) {
	const response = await fetch(path, { credentials: "same-origin", ...options });
	if (response.status === 204) return null;
	const contentType = response.headers.get("content-type") || "";
	const body = contentType.includes("application/json") ? await response.json() : await response.text();
	if (!response.ok) {
		const detail = typeof body === "string" ? body.trim() : body?.error;
		throw new Error(detail || `Request failed (${response.status})`);
	}
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

async function copyText(value, successMessage, source = null) {
	try {
		if (!navigator.clipboard?.writeText) throw new Error("Clipboard API unavailable");
		await navigator.clipboard.writeText(value);
		toast(successMessage);
		return;
	} catch {
		const field = source || document.createElement("textarea");
		if (!source) {
			field.value = value;
			field.className = "clipboard-fallback";
			document.body.append(field);
		}
		field.focus();
		field.select();
		field.setSelectionRange?.(0, field.value.length);
		const copied = document.execCommand("copy");
		if (!source) field.remove();
		if (!copied && !source) window.prompt("Copy this link", value);
		toast(
			copied ? successMessage : source ? "The value is selected. Press Ctrl+C to copy it." : "Copy the link from the dialog.",
			copied ? "success" : "error",
		);
	}
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
		await openAuthenticatedRoute();
	} catch {
		state.account = null;
		show("auth");
	}
}

async function loadLibrary() {
	state.sessions = await request("/api/v1/library/sessions");
	elements.accountName.textContent = state.account.display_name;
	elements.instanceSettings.hidden = !state.account.administrator;
	syncSessionLibrary();
}

function syncSessionLibrary() {
	const snapshot = {
		sessions: state.sessions,
		maximumUploadBytes: state.instance?.limits.maximum_upload_bytes || 0,
	};
	window.traceLibrarySnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:sessions-load", { detail: snapshot }));
	if (state.comparePickerOpen) syncComparePicker();
}

function syncReviewHeader() {
	const snapshot = state.review
		? {
				summary: state.review.summary,
				laps: state.review.laps,
				publicShareId: state.publicShareId,
			}
		: null;
	window.traceReviewSnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:review-load", { detail: snapshot }));
}

function syncLapList() {
	const snapshot = state.review
		? {
				laps: state.review.laps,
				selectedLapId: state.selectedLapId,
			}
		: null;
	window.traceLapListSnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:laps-load", { detail: snapshot }));
}

function syncComparePicker() {
	const snapshot =
		state.comparePickerOpen && state.review && state.selectedLap
			? {
					sessions: state.sessions,
					currentReview: state.review,
					selectedLap: state.selectedLap,
				}
			: null;
	window.traceCompareSnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:compare-load", { detail: snapshot }));
}

function toggleAdminPanel() {
	elements.adminPanel.hidden = !elements.adminPanel.hidden;
	if (elements.adminPanel.hidden) return;
	window.dispatchEvent(new Event("trace:admin-open"));
}

async function toggleTokenPanel() {
	elements.tokenPanel.hidden = !elements.tokenPanel.hidden;
	if (elements.tokenPanel.hidden) return;
	try {
		state.tokens = await request("/api/v1/auth/tokens");
		renderTokens();
	} catch (error) {
		toast(error.message, "error");
	}
}

function renderTokens() {
	const rows = state.tokens.map((token) => {
		const row = document.createElement("div");
		row.className = "token-row";
		const details = document.createElement("span");
		const label = document.createElement("strong");
		label.textContent = token.label;
		const meta = document.createElement("small");
		meta.textContent = `${token.token_prefix}… · Created ${formatDate(token.created_at_unix_ms)}`;
		details.append(label, meta);
		const revoke = document.createElement("button");
		revoke.type = "button";
		revoke.className = "danger";
		revoke.textContent = "Revoke";
		revoke.addEventListener("click", () => revokeToken(token, revoke));
		row.append(details, revoke);
		return row;
	});
	elements.tokenList.replaceChildren(...rows);
}

async function createToken() {
	const label = elements.tokenLabel.value.trim();
	if (!label) return;
	elements.tokenCreate.disabled = true;
	try {
		const created = await request("/api/v1/auth/tokens", {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ label }),
		});
		state.tokens.unshift(created);
		elements.tokenValue.value = created.token;
		elements.tokenSecret.hidden = false;
		renderTokens();
		toast("Desktop token created.");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		elements.tokenCreate.disabled = false;
	}
}

async function revokeToken(token, button) {
	button.disabled = true;
	try {
		await request(`/api/v1/auth/tokens/${encodeURIComponent(token.token_id)}`, { method: "DELETE" });
		state.tokens = state.tokens.filter((candidate) => candidate.token_id !== token.token_id);
		renderTokens();
		toast("Desktop token revoked.");
	} catch (error) {
		button.disabled = false;
		toast(error.message, "error");
	}
}

async function copyToken() {
	await copyText(elements.tokenValue.value, "Desktop token copied.", elements.tokenValue);
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

function urlSlug(...parts) {
	const slug = parts
		.filter(Boolean)
		.join(" ")
		.normalize("NFKD")
		.toLocaleLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "")
		.slice(0, 80)
		.replace(/-$/, "");
	return slug || "session";
}

function sessionPath(summary) {
	const identity = readable(summary.driver_name, readable(summary.title, "session"));
	return `/app/sessions/${urlSlug(identity, summary.track)}--${summary.session_id}`;
}

function sharedSessionPath(summary, shareId) {
	const identity = readable(summary.driver_name, readable(summary.title, "session"));
	return `/shared/${urlSlug(identity, summary.track)}--${shareId}`;
}

function routeIdentifier(pattern) {
	return window.location.pathname.match(pattern)?.[1] || null;
}

function privateSessionId() {
	return routeIdentifier(/^\/app\/sessions\/.+--([0-9a-f]{32})\/?$/i);
}

function publicShareId() {
	return routeIdentifier(/^\/p\/([0-9a-f]{32})\/?$/i) || routeIdentifier(/^\/shared\/.+--([0-9a-f]{32})\/?$/i);
}

function setLocation(path, replace = false) {
	if (window.location.pathname === path) return;
	history[replace ? "replaceState" : "pushState"]({}, "", path);
}

async function openAuthenticatedRoute() {
	const sessionId = privateSessionId();
	if (sessionId) {
		await openReview(sessionId, false);
		return;
	}
	if (window.location.pathname === "/app") {
		if (state.review) closeReview(false);
		return;
	}
	if (window.location.pathname !== "/app") setLocation("/app", true);
}

async function openReview(sessionId, navigate = true) {
	state.publicShareId = null;
	state.review = null;
	syncReviewHeader();
	syncLapList();
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading session…";
	try {
		state.review = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/review`);
		state.telemetry = null;
		state.selectedLap = null;
		state.selectedLapId = null;
		clearComparison(false);
		renderReview();
		if (navigate) setLocation(sessionPath(state.review.summary));
		const preferred = [...state.review.laps].filter((lap) => lap.duration_ns != null).sort((left, right) => left.duration_ns - right.duration_ns)[0];
		if (preferred) await loadLap(preferred.id);
	} catch (error) {
		closeReview(false);
		setLocation("/app", true);
		toast(error.message, "error");
	}
}

async function openPublicReview(shareId) {
	state.publicShareId = shareId;
	state.review = null;
	syncReviewHeader();
	syncLapList();
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
	elements.sharePanel.hidden = true;
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading shared session…";
	try {
		state.review = await request(`/api/v1/public-sessions/${encodeURIComponent(shareId)}`);
		state.telemetry = null;
		state.selectedLap = null;
		state.selectedLapId = null;
		clearComparison(false);
		renderReview();
		setLocation(sharedSessionPath(state.review.summary, shareId), true);
		const preferred = [...state.review.laps].filter((lap) => lap.duration_ns != null).sort((left, right) => left.duration_ns - right.duration_ns)[0];
		if (preferred) await loadLap(preferred.id);
		show("library");
	} catch (error) {
		elements.telemetryEmpty.textContent = "This shared session is unavailable or its owner revoked the link.";
		show("library");
		toast(error.message, "error");
	}
}

function closeReview(navigate = true) {
	state.review = null;
	state.telemetry = null;
	state.selectedLap = null;
	state.selectedLapId = null;
	state.publicShareId = null;
	state.shareUrl = null;
	clearComparison(false);
	closeComparePicker();
	syncReviewHeader();
	syncLapList();
	elements.sharePanel.hidden = true;
	elements.reviewView.hidden = true;
	elements.libraryHome.hidden = false;
	document.title = `${state.instance?.name || "TRACE"} · TRACE Library`;
	if (navigate) setLocation("/app");
}

function updateOwnedShare(shareId, shareUrl = null) {
	if (!state.review) return;
	state.review.summary.share_id = shareId;
	state.shareUrl = shareUrl;
	const summary = state.sessions.find((session) => session.session_id === state.review.summary.session_id);
	if (summary) summary.share_id = shareId;
	syncSessionLibrary();
	syncReviewHeader();
	renderShareState();
}

function renderShareState() {
	const shareId = state.review?.summary.share_id;
	if (!shareId) {
		elements.sharePanel.hidden = true;
		elements.shareUrl.value = "";
		return;
	}
	elements.shareUrl.value = state.shareUrl || new URL(sharedSessionPath(state.review.summary, shareId), window.location.origin).href;
}

async function toggleSharePanel() {
	if (!state.review || state.publicShareId) return;
	if (state.review.summary.share_id) {
		elements.sharePanel.hidden = !elements.sharePanel.hidden;
		return;
	}
	const button = document.querySelector("#share-open");
	if (button) button.disabled = true;
	try {
		const sessionId = state.review.summary.session_id;
		const share = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/share`, { method: "POST" });
		updateOwnedShare(share.share_id, share.share_url);
		elements.sharePanel.hidden = false;
		toast("Unlisted session link created.");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		if (button) button.disabled = false;
	}
}

async function copyShareLink() {
	await copyText(elements.shareUrl.value, "Share link copied.", elements.shareUrl);
}

async function copyTraceLink() {
	if (!state.publicShareId) return;
	const downloadUrl = new URL(`/api/v1/public-sessions/${encodeURIComponent(state.publicShareId)}/download`, window.location.origin).href;
	await copyText(`trace://shared-session?url=${encodeURIComponent(downloadUrl)}`, "TRACE app link copied.");
}

async function revokeShare() {
	if (elements.shareRevoke.textContent !== "Confirm revoke") {
		elements.shareRevoke.textContent = "Confirm revoke";
		setTimeout(() => {
			if (elements.shareRevoke.textContent === "Confirm revoke") elements.shareRevoke.textContent = "Revoke";
		}, 3000);
		return;
	}
	elements.shareRevoke.disabled = true;
	try {
		const sessionId = state.review.summary.session_id;
		await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/share`, { method: "DELETE" });
		updateOwnedShare(null);
		toast("Public access revoked.");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		elements.shareRevoke.disabled = false;
		elements.shareRevoke.textContent = "Revoke";
	}
}

function renderReview() {
	const { summary, laps } = state.review;
	syncReviewHeader();
	syncLapList();
	document.title = `${readable(summary.driver_name, readable(summary.title, "Session"))} at ${readable(summary.track, "Unknown track")} · TRACE`;
	elements.telemetryEmpty.textContent = laps.length ? "Select a recorded lap to review its telemetry." : "This package contains no recorded laps.";
	if (!state.publicShareId) renderShareState();
}

async function loadLap(lapId) {
	if (!state.review) return;
	clearComparison(false);
	state.selectedLapId = lapId;
	syncLapList();
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading lap telemetry…";
	try {
		const sessionId = state.review.summary.session_id;
		const endpoint = state.publicShareId
			? `/api/v1/public-sessions/${encodeURIComponent(state.publicShareId)}/laps/${encodeURIComponent(lapId)}/telemetry?max_points=1800`
			: `/api/v1/library/sessions/${encodeURIComponent(sessionId)}/laps/${encodeURIComponent(lapId)}/telemetry?max_points=1800`;
		state.telemetry = await request(endpoint);
		const lap = state.review.laps.find((candidate) => candidate.id === lapId);
		state.selectedLap = lap;
		renderTelemetry(lap);
	} catch (error) {
		elements.telemetryEmpty.textContent = error.message;
		toast(error.message, "error");
	}
}

function openComparePicker() {
	if (!state.selectedLap || state.selectedLap.duration_ns == null || !state.telemetry) {
		toast("Select a timed lap first.", "error");
		return;
	}
	state.comparePickerOpen = true;
	syncComparePicker();
}

function closeComparePicker() {
	state.comparePickerOpen = false;
	syncComparePicker();
}

function createComparison(candidateSession, candidateLap, candidateTelemetry) {
	if (!state.review || !state.selectedLap || !state.telemetry) return;
	const current = { detail: state.review, lap: state.selectedLap, telemetry: state.telemetry };
	const candidate = { detail: candidateSession, lap: candidateLap, telemetry: candidateTelemetry };
	const reference = candidateLap.duration_ns < state.selectedLap.duration_ns ? candidate : current;
	const analysed = reference === candidate ? current : candidate;
	state.comparison = { reference, analysed, ...alignLaps(reference.telemetry.points, analysed.telemetry.points) };
	closeComparePicker();
	renderComparison();
	requestAnimationFrame(drawTelemetry);
}

function progressSeries(points) {
	const positioned = points.filter((point) => Number.isFinite(point.lap_position));
	const minimum = Math.min(...positioned.map((point) => point.lap_position));
	const maximum = Math.max(...positioned.map((point) => point.lap_position));
	if (positioned.length >= points.length * 0.65 && maximum - minimum > 0.5) {
		return {
			mode: "distance",
			points: positioned
				.map((point) => ({ ...point, progress: (point.lap_position - minimum) / (maximum - minimum) }))
				.sort((left, right) => left.progress - right.progress),
		};
	}
	const start = points[0]?.elapsed_seconds ?? 0;
	const end = points.at(-1)?.elapsed_seconds ?? start + 1;
	const duration = Math.max(end - start, 0.001);
	return { mode: "time", points: points.map((point) => ({ ...point, progress: (point.elapsed_seconds - start) / duration })) };
}

function interpolate(points, progress, key) {
	if (!points.length) return null;
	let upper = points.findIndex((point) => point.progress >= progress);
	if (upper < 0) upper = points.length - 1;
	const lower = Math.max(0, upper - 1);
	const before = points[lower];
	const after = points[upper];
	const left = before[key];
	const right = after[key];
	if (!Number.isFinite(left)) return Number.isFinite(right) ? right : null;
	if (!Number.isFinite(right) || after.progress === before.progress) return left;
	const fraction = (progress - before.progress) / (after.progress - before.progress);
	return left + (right - left) * Math.max(0, Math.min(1, fraction));
}

function alignLaps(referencePoints, analysedPoints) {
	const reference = progressSeries(referencePoints);
	const analysed = progressSeries(analysedPoints);
	const keys = ["elapsed_seconds", "speed_kph", "throttle", "brake", "position_x_m", "position_z_m"];
	const samples = Array.from({ length: 600 }, (_, index) => {
		const progress = index / 599;
		const sample = { progress };
		for (const key of keys) {
			sample[`reference_${key}`] = interpolate(reference.points, progress, key);
			sample[`analysed_${key}`] = interpolate(analysed.points, progress, key);
		}
		sample.delta_seconds =
			Number.isFinite(sample.reference_elapsed_seconds) && Number.isFinite(sample.analysed_elapsed_seconds)
				? sample.analysed_elapsed_seconds - sample.reference_elapsed_seconds
				: null;
		return sample;
	});
	return { samples, alignment: reference.mode === "distance" && analysed.mode === "distance" ? "distance" : "time" };
}

function comparisonName(entry) {
	return readable(entry.detail.summary.driver_name, readable(entry.detail.summary.title, `Lap ${entry.lap.index}`));
}

function renderComparison() {
	const { reference, analysed, alignment } = state.comparison;
	const referenceElement = elements.comparisonSummary.querySelector(".reference");
	const analysedElement = elements.comparisonSummary.querySelector(".analysed");
	referenceElement.querySelector("strong").textContent = comparisonName(reference);
	referenceElement.querySelector("span").textContent = `Lap ${reference.lap.index} · ${formatDuration(reference.lap.duration_ns)}`;
	analysedElement.querySelector("strong").textContent = comparisonName(analysed);
	analysedElement.querySelector("span").textContent = `Lap ${analysed.lap.index} · ${formatDuration(analysed.lap.duration_ns)}`;
	const deltaSeconds = (analysed.lap.duration_ns - reference.lap.duration_ns) / 1_000_000_000;
	elements.comparisonSummary.querySelector(".comparison-delta strong").textContent = `+${deltaSeconds.toFixed(3)}`;
	elements.alignmentLabel.textContent = alignment === "distance" ? "DISTANCE ALIGNED" : "NORMALISED TIME · DISTANCE UNAVAILABLE";
	elements.comparisonSummary.hidden = false;
	elements.deltaCard.hidden = false;
}

function clearComparison(redraw = true) {
	state.comparison = null;
	elements.comparisonSummary.hidden = true;
	elements.deltaCard.hidden = true;
	if (redraw) requestAnimationFrame(drawTelemetry);
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
	if (state.comparison) {
		const { reference, analysed, samples } = state.comparison;
		drawTrack(elements.trackCanvas, [
			{ points: analysed.telemetry.points, color: "#c9ff00", width: 2 },
			{ points: reference.telemetry.points, color: "#b49cff", width: 3 },
		]);
		drawSeriesGraph(elements.speedCanvas, [
			{ values: samples.map((sample) => sample.analysed_speed_kph), color: "#65a9ff" },
			{ values: samples.map((sample) => sample.reference_speed_kph), color: "#b49cff" },
		]);
		drawSeriesGraph(elements.throttleCanvas, [
			{ values: samples.map((sample) => sample.analysed_throttle), color: "#c9ff00", maximum: 1 },
			{ values: samples.map((sample) => sample.reference_throttle), color: "#b49cff", maximum: 1 },
		]);
		drawSeriesGraph(elements.brakeCanvas, [
			{ values: samples.map((sample) => sample.analysed_brake), color: "#ef6969", maximum: 1 },
			{ values: samples.map((sample) => sample.reference_brake), color: "#b49cff", maximum: 1 },
		]);
		drawSeriesGraph(elements.deltaCanvas, [{ values: samples.map((sample) => sample.delta_seconds), color: "#e8e8e8" }], true);
		return;
	}
	drawTrack(elements.trackCanvas, [{ points: state.telemetry.points, color: "#d5d5d5", width: 3 }]);
	drawSeriesGraph(elements.speedCanvas, [{ values: state.telemetry.points.map((point) => point.speed_kph), color: "#65a9ff" }]);
	drawSeriesGraph(elements.throttleCanvas, [{ values: state.telemetry.points.map((point) => point.throttle), color: "#c9ff00", maximum: 1 }]);
	drawSeriesGraph(elements.brakeCanvas, [{ values: state.telemetry.points.map((point) => point.brake), color: "#ef6969", maximum: 1 }]);
}

function drawTrack(canvas, traces) {
	const { context, width, height } = canvasContext(canvas);
	const positionedTraces = traces.map((trace) => ({
		...trace,
		points: trace.points.filter((point) => Number.isFinite(point.position_x_m) && Number.isFinite(point.position_z_m)),
	}));
	const positions = positionedTraces.flatMap((trace) => trace.points);
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
	for (const trace of positionedTraces) {
		context.strokeStyle = trace.color;
		context.lineWidth = trace.width;
		context.beginPath();
		trace.points.forEach((point, index) => {
			const x = offsetX + (point.position_x_m - minX) * scale;
			const y = offsetZ + (point.position_z_m - minZ) * scale;
			if (index === 0) context.moveTo(x, y);
			else context.lineTo(x, y);
		});
		context.stroke();
	}
}

function drawSeriesGraph(canvas, series, zeroCentered = false) {
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
	const allValues = series.flatMap((item) => item.values.filter(Number.isFinite));
	const absoluteMaximum = Math.max(0.01, ...allValues.map(Math.abs));
	const sharedMaximum = zeroCentered ? absoluteMaximum : Math.max(1, ...allValues);
	if (zeroCentered) {
		context.strokeStyle = "#555";
		context.beginPath();
		context.moveTo(0, height / 2);
		context.lineTo(width, height / 2);
		context.stroke();
	}
	for (const item of series) {
		const maximum = item.maximum || sharedMaximum;
		context.strokeStyle = item.color;
		context.lineWidth = 2;
		context.beginPath();
		let started = false;
		item.values.forEach((value, index) => {
			if (!Number.isFinite(value)) return;
			const x = item.values.length === 1 ? 0 : (index / (item.values.length - 1)) * width;
			const normalized = zeroCentered ? (value + maximum) / (maximum * 2) : Math.max(0, value) / maximum;
			const y = height - padding - normalized * (height - padding * 2);
			if (started) context.lineTo(x, y);
			else context.moveTo(x, y);
			started = true;
		});
		context.stroke();
	}
}

elements.loginTab.addEventListener("click", () => switchAuth("login"));
elements.registerTab.addEventListener("click", () => switchAuth("register"));
elements.shareCopy.addEventListener("click", copyShareLink);
elements.shareRevoke.addEventListener("click", revokeShare);
elements.comparisonClear.addEventListener("click", () => clearComparison());

elements.loginForm.addEventListener("submit", async (event) => {
	event.preventDefault();
	const values = Object.fromEntries(new FormData(elements.loginForm));
	setBusy(elements.loginForm, true);
	try {
		state.account = await request("/api/v1/auth/sessions", {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify(values),
		});
		elements.loginForm.reset();
		await loadLibrary();
		show("library");
		await openAuthenticatedRoute();
	} catch (error) {
		toast(error.message, "error");
	} finally {
		setBusy(elements.loginForm, false);
	}
});

elements.registerForm.addEventListener("submit", async (event) => {
	event.preventDefault();
	const values = Object.fromEntries(new FormData(elements.registerForm));
	setBusy(elements.registerForm, true);
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
		await openAuthenticatedRoute();
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
		closeReview(false);
		setLocation("/app", true);
		show("auth");
	} catch (error) {
		toast(error.message, "error");
	}
});
elements.connectDesktop.addEventListener("click", toggleTokenPanel);
elements.instanceSettings.addEventListener("click", toggleAdminPanel);
elements.tokenCreate.addEventListener("click", createToken);
elements.tokenCopy.addEventListener("click", copyToken);
for (const field of [elements.tokenValue, elements.shareUrl]) {
	field.addEventListener("click", () => field.select());
}
window.addEventListener("popstate", () => {
	if (state.account) void openAuthenticatedRoute();
});
window.addEventListener("trace:toast", (event) => toast(event.detail.message, event.detail.kind));
window.addEventListener("trace:library-changed", () => {
	if (state.account) void loadLibrary();
});
window.addEventListener("trace:sessions-updated", (event) => {
	state.sessions = event.detail.sessions;
});
window.addEventListener("trace:open-session", (event) => void openReview(event.detail.sessionId));
window.addEventListener("trace:lap-open", (event) => void loadLap(event.detail.lapId));
window.addEventListener("trace:review-back", () => closeReview());
window.addEventListener("trace:compare-open", openComparePicker);
window.addEventListener("trace:compare-close", closeComparePicker);
window.addEventListener("trace:comparison-create", (event) =>
	createComparison(event.detail.candidateSession, event.detail.candidateLap, event.detail.candidateTelemetry),
);
window.addEventListener("trace:share-open", toggleSharePanel);
window.addEventListener("trace:copy-app-link", copyTraceLink);
window.addEventListener("trace:review-summary-updated", (event) => {
	if (!state.review) return;
	const summary = event.detail.summary;
	state.review.summary = summary;
	const index = state.sessions.findIndex((session) => session.session_id === summary.session_id);
	if (index >= 0) state.sessions[index] = summary;
	syncSessionLibrary();
	setLocation(sessionPath(summary), true);
	document.title = `${readable(summary.driver_name, readable(summary.title, "Session"))} at ${readable(summary.track, "Unknown track")} · TRACE`;
});

const telemetryResizeObserver = new ResizeObserver(() => requestAnimationFrame(drawTelemetry));
telemetryResizeObserver.observe(elements.trackCanvas);
telemetryResizeObserver.observe(elements.speedCanvas);
telemetryResizeObserver.observe(elements.throttleCanvas);
telemetryResizeObserver.observe(elements.brakeCanvas);
telemetryResizeObserver.observe(elements.deltaCanvas);

async function start() {
	show("loading");
	try {
		state.instance = await request("/api/v1/instance");
		elements.instanceName.textContent = state.instance.name;
		elements.instanceVersion.textContent = `v${state.instance.version}`;
		document.title = `${state.instance.name} · TRACE Library`;
		const registrationOpen = state.instance.registration_enabled || state.instance.initial_admin_required;
		elements.registerTab.hidden = !registrationOpen;
		if (state.instance.initial_admin_required) {
			elements.registerTab.textContent = "Create administrator";
			switchAuth("register");
		}
		const shareId = publicShareId();
		if (shareId) {
			document.title = `Shared session · ${state.instance.name}`;
			await openPublicReview(shareId);
		} else {
			await loadAccount();
		}
	} catch (error) {
		elements.loading.querySelector("p").textContent = error.message;
		toast("Could not connect to this TRACE instance.", "error");
	}
}

start();
