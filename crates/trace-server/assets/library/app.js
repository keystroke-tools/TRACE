const state = {
	instance: null,
	account: null,
	sessions: [],
};

const elements = {
	loading: document.querySelector("#loading-view"),
	auth: document.querySelector("#auth-view"),
	library: document.querySelector("#library-view"),
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
	const remove = document.createElement("button");
	remove.className = "danger";
	remove.type = "button";
	remove.textContent = "Delete";
	remove.addEventListener("click", () => deleteSession(session, remove));
	actions.append(download, remove);

	row.append(primary, context, actions);
	return row;
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
		show("auth");
	} catch (error) {
		toast(error.message, "error");
	}
});

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
