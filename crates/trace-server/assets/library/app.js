const state = {
	instance: null,
	account: null,
	sessions: [],
	review: null,
	telemetry: null,
	selectedLap: null,
	comparison: null,
	pickerReview: null,
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
	registrationEnabled: document.querySelector("#registration-enabled"),
	tokenPanel: document.querySelector("#token-panel"),
	tokenLabel: document.querySelector("#token-label"),
	tokenCreate: document.querySelector("#token-create"),
	tokenSecret: document.querySelector("#token-secret"),
	tokenValue: document.querySelector("#token-value"),
	tokenCopy: document.querySelector("#token-copy"),
	tokenList: document.querySelector("#token-list"),
	input: document.querySelector("#package-input"),
	uploadZone: document.querySelector("#upload-zone"),
	uploadLimit: document.querySelector("#upload-limit"),
	search: document.querySelector("#session-search"),
	list: document.querySelector("#session-list"),
	count: document.querySelector("#session-count"),
	empty: document.querySelector("#empty-library"),
	reviewBack: document.querySelector("#review-back"),
	reviewDownload: document.querySelector("#review-download"),
	reviewOpenTrace: document.querySelector("#review-open-trace"),
	reviewCopyTrace: document.querySelector("#review-copy-trace"),
	metadataOpen: document.querySelector("#metadata-open"),
	metadataPanel: document.querySelector("#metadata-panel"),
	metadataCancel: document.querySelector("#metadata-cancel"),
	metadataTitle: document.querySelector("#metadata-title"),
	metadataDriver: document.querySelector("#metadata-driver"),
	metadataTags: document.querySelector("#metadata-tags"),
	shareOpen: document.querySelector("#share-open"),
	sharePanel: document.querySelector("#share-panel"),
	shareUrl: document.querySelector("#share-url"),
	shareCopy: document.querySelector("#share-copy"),
	shareRevoke: document.querySelector("#share-revoke"),
	compareOpen: document.querySelector("#compare-open"),
	compareClose: document.querySelector("#compare-close"),
	comparePicker: document.querySelector("#compare-picker"),
	compareSearch: document.querySelector("#compare-search"),
	compareSessions: document.querySelector("#compare-sessions"),
	compareLaps: document.querySelector("#compare-laps"),
	comparisonSummary: document.querySelector("#comparison-summary"),
	comparisonClear: document.querySelector("#comparison-clear"),
	alignmentLabel: document.querySelector("#alignment-label"),
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
	renderSessions();
}

async function toggleAdminPanel() {
	elements.adminPanel.hidden = !elements.adminPanel.hidden;
	if (elements.adminPanel.hidden) return;
	try {
		const settings = await request("/api/v1/admin/settings");
		elements.registrationEnabled.checked = settings.registration_enabled;
	} catch (error) {
		toast(error.message, "error");
	}
}

async function updateRegistration() {
	elements.registrationEnabled.disabled = true;
	try {
		const settings = await request("/api/v1/admin/settings", {
			method: "PUT",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ registration_enabled: elements.registrationEnabled.checked }),
		});
		elements.registrationEnabled.checked = settings.registration_enabled;
		toast(settings.registration_enabled ? "Account registration opened." : "Account registration closed.");
	} catch (error) {
		elements.registrationEnabled.checked = !elements.registrationEnabled.checked;
		toast(error.message, "error");
	} finally {
		elements.registrationEnabled.disabled = false;
	}
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
	meta.textContent = `${session.share_id ? "UNLISTED · " : ""}${readable(session.session_type, "Session")} · ${session.lap_count} ${session.lap_count === 1 ? "lap" : "laps"}`;
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

async function openReview(sessionId, navigate = true) {
	state.publicShareId = null;
	elements.metadataOpen.hidden = false;
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading session…";
	try {
		state.review = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/review`);
		state.telemetry = null;
		state.selectedLap = null;
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
	elements.libraryHome.hidden = true;
	elements.reviewView.hidden = false;
	elements.reviewBack.hidden = true;
	elements.compareOpen.hidden = true;
	elements.metadataOpen.hidden = true;
	elements.metadataPanel.hidden = true;
	elements.shareOpen.hidden = true;
	elements.sharePanel.hidden = true;
	elements.telemetryContent.hidden = true;
	elements.telemetryEmpty.hidden = false;
	elements.telemetryEmpty.textContent = "Loading shared session…";
	try {
		state.review = await request(`/api/v1/public-sessions/${encodeURIComponent(shareId)}`);
		state.telemetry = null;
		state.selectedLap = null;
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
	state.publicShareId = null;
	state.shareUrl = null;
	clearComparison(false);
	closeComparePicker();
	elements.reviewBack.hidden = false;
	elements.compareOpen.hidden = false;
	elements.shareOpen.hidden = false;
	elements.reviewOpenTrace.hidden = true;
	elements.reviewCopyTrace.hidden = true;
	elements.metadataPanel.hidden = true;
	elements.sharePanel.hidden = true;
	elements.reviewView.hidden = true;
	elements.libraryHome.hidden = false;
	document.title = `${state.instance?.name || "TRACE"} · TRACE Library`;
	if (navigate) setLocation("/app");
}

function toggleMetadataPanel() {
	if (!state.review || state.publicShareId) return;
	const opening = elements.metadataPanel.hidden;
	elements.metadataPanel.hidden = !opening;
	if (!opening) return;
	const summary = state.review.summary;
	elements.metadataTitle.value = summary.title || "";
	elements.metadataDriver.value = summary.driver_name || "";
	elements.metadataTags.value = (summary.tags || []).join(", ");
	elements.metadataTitle.focus();
}

async function updateMetadata(event) {
	event.preventDefault();
	if (!state.review || state.publicShareId) return;
	const tags = elements.metadataTags.value
		.split(",")
		.map((tag) => tag.trim())
		.filter(Boolean);
	setBusy(elements.metadataPanel, true);
	try {
		const sessionId = state.review.summary.session_id;
		const summary = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}`, {
			method: "PUT",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({
				title: elements.metadataTitle.value,
				driver_name: elements.metadataDriver.value,
				tags,
			}),
		});
		state.review.summary = summary;
		const index = state.sessions.findIndex((session) => session.session_id === sessionId);
		if (index >= 0) state.sessions[index] = summary;
		elements.metadataPanel.hidden = true;
		renderSessions();
		renderReview();
		setLocation(sessionPath(summary), true);
		toast("Session details updated.");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		setBusy(elements.metadataPanel, false);
	}
}

function updateOwnedShare(shareId, shareUrl = null) {
	if (!state.review) return;
	state.review.summary.share_id = shareId;
	state.shareUrl = shareUrl;
	const summary = state.sessions.find((session) => session.session_id === state.review.summary.session_id);
	if (summary) summary.share_id = shareId;
	renderSessions();
	renderShareState();
}

function renderShareState() {
	const shareId = state.review?.summary.share_id;
	elements.shareOpen.textContent = shareId ? "Shared" : "Share";
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
	elements.shareOpen.disabled = true;
	try {
		const sessionId = state.review.summary.session_id;
		const share = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/share`, { method: "POST" });
		updateOwnedShare(share.share_id, share.share_url);
		elements.sharePanel.hidden = false;
		toast("Unlisted session link created.");
	} catch (error) {
		toast(error.message, "error");
	} finally {
		elements.shareOpen.disabled = false;
	}
}

async function copyShareLink() {
	await copyText(elements.shareUrl.value, "Share link copied.", elements.shareUrl);
}

async function copyTraceLink() {
	await copyText(elements.reviewOpenTrace.href, "TRACE app link copied.");
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
	elements.reviewDownload.href = state.publicShareId
		? `/api/v1/public-sessions/${encodeURIComponent(state.publicShareId)}/download`
		: `/api/v1/library/sessions/${encodeURIComponent(summary.session_id)}/download`;
	elements.reviewOpenTrace.hidden = !state.publicShareId;
	elements.reviewCopyTrace.hidden = !state.publicShareId;
	if (state.publicShareId) {
		const downloadUrl = new URL(`/api/v1/public-sessions/${encodeURIComponent(state.publicShareId)}/download`, window.location.origin).href;
		elements.reviewOpenTrace.href = `trace://shared-session?url=${encodeURIComponent(downloadUrl)}`;
	}
	elements.reviewKicker.textContent = `${state.publicShareId ? "Shared session · " : ""}${readable(summary.session_type, "Session")} · ${readable(summary.simulator, "Unknown simulator")}`;
	elements.reviewName.textContent = readable(summary.title, readable(summary.driver_name, "Recorded session"));
	elements.reviewContext.textContent = `${readable(summary.track, "Unknown track")} · ${readable(summary.car, "Unknown car")} · ${formatDate(summary.started_at)}`;
	document.title = `${readable(summary.driver_name, readable(summary.title, "Session"))} at ${readable(summary.track, "Unknown track")} · TRACE`;
	elements.reviewLapCount.textContent = `${laps.length} ${laps.length === 1 ? "lap" : "laps"}`;
	const timed = laps.filter((lap) => lap.duration_ns != null);
	const best = timed.length ? timed.reduce((fastest, lap) => (lap.duration_ns < fastest.duration_ns ? lap : fastest)) : null;
	elements.reviewBest.hidden = !best;
	if (best) elements.reviewBest.querySelector("strong").textContent = formatDuration(best.duration_ns);
	elements.reviewLaps.replaceChildren(...laps.map((lap) => lapRow(lap, best?.id)));
	elements.telemetryEmpty.textContent = laps.length ? "Select a recorded lap to review its telemetry." : "This package contains no recorded laps.";
	if (!state.publicShareId) renderShareState();
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
	clearComparison(false);
	for (const button of elements.reviewLaps.querySelectorAll(".lap-row")) button.classList.toggle("active", button.dataset.lapId === lapId);
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
	if (!state.selectedLap || !state.telemetry) {
		toast("Select a timed lap first.", "error");
		return;
	}
	elements.comparePicker.hidden = false;
	elements.compareSearch.value = "";
	state.pickerReview = null;
	renderCompareSessions();
	elements.compareLaps.replaceChildren(message("Select a session to see its laps."));
	elements.compareSearch.focus();
}

function closeComparePicker() {
	elements.comparePicker.hidden = true;
	state.pickerReview = null;
}

function message(text) {
	const paragraph = document.createElement("p");
	paragraph.textContent = text;
	return paragraph;
}

function summaryCompatible(summary) {
	const current = state.review?.summary;
	if (!current) return false;
	return (
		summary.simulator === current.simulator &&
		readable(summary.track, "").toLocaleLowerCase() === readable(current.track, "").toLocaleLowerCase() &&
		readable(summary.car, "").toLocaleLowerCase() === readable(current.car, "").toLocaleLowerCase()
	);
}

function renderCompareSessions() {
	const query = elements.compareSearch.value.trim().toLocaleLowerCase();
	const sessions = [...state.sessions]
		.filter((session) => sessionSearchText(session).includes(query))
		.sort((left, right) => Number(summaryCompatible(right)) - Number(summaryCompatible(left)));
	elements.compareSessions.replaceChildren(...sessions.map(compareSessionRow));
	if (!sessions.length) elements.compareSessions.append(message("No matching sessions."));
}

function compareSessionRow(session) {
	const button = document.createElement("button");
	button.type = "button";
	button.className = "picker-session";
	if (state.pickerReview?.summary.session_id === session.session_id) button.classList.add("active");
	const heading = document.createElement("span");
	const driver = document.createElement("strong");
	driver.textContent = readable(session.driver_name, readable(session.title, "Recorded session"));
	const badge = document.createElement("small");
	badge.textContent = summaryCompatible(session) ? "MATCH" : readable(session.simulator, "OTHER");
	heading.append(driver, badge);
	const context = document.createElement("span");
	context.textContent = `${readable(session.track, "Unknown track")} · ${readable(session.car, "Unknown car")}`;
	const date = document.createElement("time");
	date.textContent = formatDate(session.started_at);
	button.append(heading, context, date);
	button.addEventListener("click", () => loadPickerSession(session.session_id));
	return button;
}

async function loadPickerSession(sessionId) {
	elements.compareLaps.replaceChildren(message("Loading laps…"));
	try {
		state.pickerReview = await request(`/api/v1/library/sessions/${encodeURIComponent(sessionId)}/review`);
		renderCompareSessions();
		renderPickerLaps();
	} catch (error) {
		elements.compareLaps.replaceChildren(message(error.message));
	}
}

function detailsCompatible(left, right) {
	const sameIdentity = (sourceLeft, sourceRight, labelLeft, labelRight) =>
		sourceLeft && sourceRight ? sourceLeft === sourceRight : readable(labelLeft, "").toLocaleLowerCase() === readable(labelRight, "").toLocaleLowerCase();
	return (
		left.summary.simulator === right.summary.simulator &&
		sameIdentity(left.source_track_id, right.source_track_id, left.summary.track, right.summary.track) &&
		sameIdentity(left.source_car_id, right.source_car_id, left.summary.car, right.summary.car) &&
		readable(left.summary.layout, "") === readable(right.summary.layout, "")
	);
}

function renderPickerLaps() {
	const compatible = detailsCompatible(state.review, state.pickerReview);
	const heading = document.createElement("div");
	heading.className = `picker-compatibility ${compatible ? "compatible" : "warning"}`;
	heading.textContent = compatible ? "Compatible car and track" : "Different car, track, layout, or simulator";
	const laps = state.pickerReview.laps.map((lap) => pickerLapRow(lap, compatible));
	elements.compareLaps.replaceChildren(heading, ...laps);
	if (!laps.length) elements.compareLaps.append(message("This session has no laps."));
}

function pickerLapRow(lap, compatible) {
	const sameLap = state.pickerReview.summary.session_id === state.review.summary.session_id && lap.id === state.selectedLap.id;
	const unavailable = lap.duration_ns == null || lap.validity === "invalid" || (lap.max_tyres_out ?? 0) >= 3;
	const button = document.createElement("button");
	button.type = "button";
	button.className = "picker-lap";
	button.disabled = sameLap || unavailable || !compatible;
	const label = document.createElement("span");
	label.textContent = `Lap ${lap.index}`;
	const duration = document.createElement("strong");
	duration.textContent = formatDuration(lap.duration_ns);
	const status = document.createElement("small");
	status.textContent = sameLap ? "CURRENT" : unavailable ? "UNAVAILABLE" : `${lap.sectors.length} SECTORS`;
	button.append(label, duration, status);
	button.addEventListener("click", () => createComparison(lap));
	return button;
}

async function createComparison(candidateLap) {
	elements.compareLaps.classList.add("loading");
	try {
		const candidateSession = state.pickerReview;
		const candidateTelemetry = await request(
			`/api/v1/library/sessions/${encodeURIComponent(candidateSession.summary.session_id)}/laps/${encodeURIComponent(candidateLap.id)}/telemetry?max_points=1800`,
		);
		const current = { detail: state.review, lap: state.selectedLap, telemetry: state.telemetry };
		const candidate = { detail: candidateSession, lap: candidateLap, telemetry: candidateTelemetry };
		const reference = candidateLap.duration_ns < state.selectedLap.duration_ns ? candidate : current;
		const analysed = reference === candidate ? current : candidate;
		state.comparison = { reference, analysed, ...alignLaps(reference.telemetry.points, analysed.telemetry.points) };
		closeComparePicker();
		renderComparison();
		requestAnimationFrame(drawTelemetry);
	} catch (error) {
		toast(error.message, "error");
	} finally {
		elements.compareLaps.classList.remove("loading");
	}
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
elements.reviewBack.addEventListener("click", () => closeReview());
elements.metadataOpen.addEventListener("click", toggleMetadataPanel);
elements.metadataCancel.addEventListener("click", () => {
	elements.metadataPanel.hidden = true;
});
elements.metadataPanel.addEventListener("submit", updateMetadata);
elements.shareOpen.addEventListener("click", toggleSharePanel);
elements.shareCopy.addEventListener("click", copyShareLink);
elements.reviewCopyTrace.addEventListener("click", copyTraceLink);
elements.shareRevoke.addEventListener("click", revokeShare);
elements.compareOpen.addEventListener("click", openComparePicker);
elements.compareClose.addEventListener("click", closeComparePicker);
elements.comparisonClear.addEventListener("click", () => clearComparison());
elements.compareSearch.addEventListener("input", renderCompareSessions);
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
elements.registrationEnabled.addEventListener("change", updateRegistration);
elements.tokenCreate.addEventListener("click", createToken);
elements.tokenCopy.addEventListener("click", copyToken);
for (const field of [elements.tokenValue, elements.shareUrl]) {
	field.addEventListener("click", () => field.select());
}
window.addEventListener("popstate", () => {
	if (state.account) void openAuthenticatedRoute();
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
		elements.uploadLimit.textContent = `up to ${formatBytes(state.instance.limits.maximum_upload_bytes)}`;
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
