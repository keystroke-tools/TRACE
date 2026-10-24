// The library coordinator intentionally owns navigation and server state while
// individual React surfaces own rendering and interaction.
// @ts-nocheck

const state = {
	instance: null,
	account: null,
	sessions: [],
	review: null,
	telemetry: null,
	telemetryMessage: "Select a recorded lap to review its telemetry.",
	lapRequestSequence: 0,
	selectedLap: null,
	selectedLapId: null,
	comparison: null,
	comparePickerOpen: false,
	publicShareId: null,
	shareUrl: null,
	sharePanelOpen: false,
};

const elements = {
	loading: document.querySelector("#loading-view"),
	auth: document.querySelector("#auth-view"),
	library: document.querySelector("#library-view"),
	libraryHome: document.querySelector("#library-home"),
	reviewView: document.querySelector("#review-view"),
	instanceName: document.querySelector("#instance-name"),
	instanceVersion: document.querySelector("#instance-version"),
	adminPanel: document.querySelector("#admin-panel"),
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

async function loadAccount() {
	try {
		state.account = await request("/api/v1/auth/session");
		syncAccountShell();
		await loadLibrary();
		show("library");
		await openAuthenticatedRoute();
	} catch {
		state.account = null;
		syncAccountShell();
		show("auth");
	}
}

async function loadLibrary() {
	state.sessions = await request("/api/v1/library/sessions");
	syncSessionLibrary();
}

function syncAccountShell() {
	const snapshot = { instance: state.instance, account: state.account };
	window.traceAccountSnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:account-load", { detail: snapshot }));
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

function syncTelemetryWorkspace(visible = Boolean(state.review)) {
	const snapshot = visible
		? {
				lap: state.selectedLap,
				telemetry: state.telemetry,
				comparison: state.comparison,
				message: state.telemetryMessage,
			}
		: null;
	window.traceTelemetrySnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:telemetry-load", { detail: snapshot }));
}

function syncSharePanel() {
	const snapshot =
		state.review && !state.publicShareId
			? {
					summary: state.review.summary,
					shareUrl: state.shareUrl,
					open: state.sharePanelOpen,
				}
			: null;
	window.traceShareSnapshot = snapshot;
	window.dispatchEvent(new CustomEvent("trace:share-load", { detail: snapshot }));
}

function toggleAdminPanel() {
	elements.adminPanel.hidden = !elements.adminPanel.hidden;
	if (elements.adminPanel.hidden) return;
	window.dispatchEvent(new Event("trace:admin-open"));
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
	state.lapRequestSequence += 1;
	state.publicShareId = null;
	state.sharePanelOpen = false;
	state.shareUrl = null;
	state.review = null;
	state.telemetry = null;
	state.selectedLap = null;
	state.comparison = null;
	state.telemetryMessage = "Loading session…";
	syncReviewHeader();
	syncLapList();
	syncTelemetryWorkspace(true);
	syncSharePanel();
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
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
	state.lapRequestSequence += 1;
	state.publicShareId = shareId;
	state.sharePanelOpen = false;
	state.shareUrl = null;
	state.review = null;
	state.telemetry = null;
	state.selectedLap = null;
	state.comparison = null;
	state.telemetryMessage = "Loading shared session…";
	syncReviewHeader();
	syncLapList();
	syncTelemetryWorkspace(true);
	syncSharePanel();
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
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
		state.telemetryMessage = "This shared session is unavailable or its owner revoked the link.";
		syncTelemetryWorkspace(true);
		show("library");
		toast(error.message, "error");
	}
}

function closeReview(navigate = true) {
	state.lapRequestSequence += 1;
	state.review = null;
	state.telemetry = null;
	state.telemetryMessage = "Select a recorded lap to review its telemetry.";
	state.selectedLap = null;
	state.selectedLapId = null;
	state.publicShareId = null;
	state.shareUrl = null;
	state.sharePanelOpen = false;
	clearComparison(false);
	closeComparePicker();
	syncReviewHeader();
	syncLapList();
	syncTelemetryWorkspace();
	syncSharePanel();
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
	if (!shareId) state.sharePanelOpen = false;
	syncSessionLibrary();
	syncReviewHeader();
	syncSharePanel();
}

async function copyTraceLink() {
	if (!state.publicShareId) return;
	const downloadUrl = new URL(`/api/v1/public-sessions/${encodeURIComponent(state.publicShareId)}/download`, window.location.origin).href;
	await copyText(`trace://shared-session?url=${encodeURIComponent(downloadUrl)}`, "TRACE app link copied.");
}

function renderReview() {
	const { summary, laps } = state.review;
	state.telemetryMessage = laps.length ? "Select a recorded lap to review its telemetry." : "This package contains no recorded laps.";
	syncReviewHeader();
	syncLapList();
	syncTelemetryWorkspace();
	syncSharePanel();
	document.title = `${readable(summary.driver_name, readable(summary.title, "Session"))} at ${readable(summary.track, "Unknown track")} · TRACE`;
}

async function loadLap(lapId) {
	if (!state.review) return;
	const requestSequence = ++state.lapRequestSequence;
	clearComparison(false);
	state.selectedLapId = lapId;
	state.selectedLap = null;
	state.telemetry = null;
	state.telemetryMessage = "Loading lap telemetry…";
	syncLapList();
	syncTelemetryWorkspace();
	try {
		const sessionId = state.review.summary.session_id;
		const endpoint = state.publicShareId
			? `/api/v1/public-sessions/${encodeURIComponent(state.publicShareId)}/laps/${encodeURIComponent(lapId)}/telemetry?max_points=1800`
			: `/api/v1/library/sessions/${encodeURIComponent(sessionId)}/laps/${encodeURIComponent(lapId)}/telemetry?max_points=1800`;
		const telemetry = await request(endpoint);
		if (requestSequence !== state.lapRequestSequence) return;
		state.telemetry = telemetry;
		const lap = state.review.laps.find((candidate) => candidate.id === lapId);
		state.selectedLap = lap;
		state.telemetryMessage = "";
		syncTelemetryWorkspace();
	} catch (error) {
		if (requestSequence !== state.lapRequestSequence) return;
		state.telemetryMessage = error.message;
		syncTelemetryWorkspace();
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
	syncTelemetryWorkspace();
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
	const keys = ["elapsed_seconds", "sector_index", "speed_kph", "throttle", "brake", "position_x_m", "position_z_m"];
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

function clearComparison(redraw = true) {
	state.comparison = null;
	if (redraw) syncTelemetryWorkspace();
}

window.addEventListener("popstate", () => {
	if (state.account) void openAuthenticatedRoute();
});
window.addEventListener("trace:toast", (event) => toast(event.detail.message, event.detail.kind));
window.addEventListener("trace:library-changed", () => {
	if (state.account) void loadLibrary();
});
window.addEventListener("trace:authenticated", async (event) => {
	state.account = event.detail.account;
	syncAccountShell();
	await loadLibrary();
	show("library");
	await openAuthenticatedRoute();
});
window.addEventListener("trace:signed-out", () => {
	state.account = null;
	state.sessions = [];
	closeReview(false);
	syncAccountShell();
	setLocation("/app", true);
	show("auth");
});
window.addEventListener("trace:admin-toggle", toggleAdminPanel);
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
window.addEventListener("trace:comparison-clear", () => clearComparison());
window.addEventListener("trace:share-updated", (event) => {
	state.sharePanelOpen = event.detail.open;
	updateOwnedShare(event.detail.shareId, event.detail.shareUrl);
});
window.addEventListener("trace:share-visibility", (event) => {
	state.sharePanelOpen = event.detail.open;
	syncSharePanel();
});
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

async function start() {
	show("loading");
	try {
		state.instance = await request("/api/v1/instance");
		syncAccountShell();
		elements.instanceName.textContent = state.instance.name;
		elements.instanceVersion.textContent = `v${state.instance.version}`;
		document.title = `${state.instance.name} · TRACE Library`;
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
