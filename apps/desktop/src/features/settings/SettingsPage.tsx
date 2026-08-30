import { useEffect, useState, type KeyboardEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { telemetryDataSource, type GameInstallDirectory, type LiveBroadcastOptions, type LiveSettings } from "../../data-source";
import { PageIntro } from "../../components/layout";
import { useToast } from "../../Toast";
import { useUpdater } from "../update/UpdateContext";
import { autoIndexSetupsEnabled, indexDetectedSetupLibraries, setAutoIndexSetupsEnabled } from "../setups/setup-preferences";

const DEFAULT_LIVE_SERVICE_ENDPOINT = "https://live.simtrace.run";
const AC_SESSION_TYPES = [
	{ id: "practice", label: "Practice" },
	{ id: "qualifying", label: "Qualifying" },
	{ id: "race", label: "Race" },
	{ id: "hotlap", label: "Hotlap" },
	{ id: "time attack", label: "Time Attack" },
	{ id: "drift", label: "Drift" },
	{ id: "drag", label: "Drag" },
] as const;

const settingsTabs = [
	{ id: "general", label: "GENERAL", description: "Driver identity and app preferences" },
	{ id: "games", label: "GAMES", description: "Simulator installations and adapters" },
	{ id: "connectivity", label: "CONNECTIVITY", description: "Live services and integrations" },
	{ id: "updates", label: "UPDATES & ABOUT", description: "Version and release channel" },
] as const;

type SettingsTab = (typeof settingsTabs)[number]["id"];

function normalizeServiceEndpoint(value: string) {
	try {
		const endpoint = new URL(value.trim());
		if ((endpoint.protocol !== "https:" && endpoint.protocol !== "http:") || !endpoint.hostname) return null;
		return endpoint.toString().replace(/\/$/, "");
	} catch {
		return null;
	}
}

export function SettingsPage() {
	const showToast = useToast();
	const updater = useUpdater();
	const [activeTab, setActiveTab] = useState<SettingsTab>("general");
	const [directories, setDirectories] = useState<GameInstallDirectory[]>([]);
	const [drafts, setDrafts] = useState<Record<string, string>>({});
	const [loading, setLoading] = useState(true);
	const [saving, setSaving] = useState<string | null>(null);
	const [profileName, setProfileName] = useState("");
	const [savedProfileName, setSavedProfileName] = useState("");
	const [savingProfile, setSavingProfile] = useState(false);
	const [liveEndpoint, setLiveEndpoint] = useState(DEFAULT_LIVE_SERVICE_ENDPOINT);
	const [savedLiveSettings, setSavedLiveSettings] = useState<LiveSettings | null>(null);
	const [savingLiveSettings, setSavingLiveSettings] = useState(false);
	const [localSpectatorPort, setLocalSpectatorPort] = useState("0");
	const [autoStreamEnabled, setAutoStreamEnabled] = useState(false);
	const [discordActivityEnabled, setDiscordActivityEnabled] = useState(false);
	const [liveMode, setLiveMode] = useState<LiveBroadcastOptions["mode"]>("hosted");
	const [acSessionTypes, setAcSessionTypes] = useState<string[]>(AC_SESSION_TYPES.map((value) => value.id));
	const [autoIndexSetups, setAutoIndexSetups] = useState(autoIndexSetupsEnabled);
	const [indexingSetups, setIndexingSetups] = useState(false);

	async function changeAutoIndexSetups(enabled: boolean) {
		setAutoIndexSetups(enabled);
		setAutoIndexSetupsEnabled(enabled);
		if (!enabled) return;
		setIndexingSetups(true);
		try {
			const result = await indexDetectedSetupLibraries();
			showToast({
				kind: result.errors > 0 ? "error" : "success",
				title: "Setup library indexing enabled",
				message: `${result.indexed} existing setup files indexed${result.errors > 0 ? ` · ${result.errors} locations could not be read` : ""}.`,
				timeoutMs: result.errors > 0 ? 8_000 : 5_000,
			});
		} finally {
			setIndexingSetups(false);
		}
	}

	useEffect(() => {
		let active = true;
		void Promise.all([telemetryDataSource.getGameInstallDirectories(), telemetryDataSource.getDriverProfile(), telemetryDataSource.getLiveSettings()])
			.then(([values, profile, liveSettings]) => {
				if (!active) return;
				setDirectories(values);
				setDrafts(Object.fromEntries(values.map((value) => [value.simulatorId, value.path ?? ""])));
				setProfileName(profile.name ?? "");
				setSavedProfileName(profile.name ?? "");
				setLiveEndpoint(liveSettings.endpoint);
				setSavedLiveSettings(liveSettings);
				setLocalSpectatorPort(String(liveSettings.autoStream.localPort ?? 0));
				setAutoStreamEnabled(liveSettings.autoStream.enabled);
				setDiscordActivityEnabled(liveSettings.discordActivityEnabled);
				setLiveMode(liveSettings.autoStream.mode);
				setAcSessionTypes(liveSettings.autoStream.simulatorSessionTypes["assetto-corsa"] ?? []);
				setLoading(false);
			})
			.catch((error) => {
				if (!active) return;
				setLoading(false);
				showToast({ kind: "error", title: "Settings unavailable", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
			});
		return () => {
			active = false;
		};
	}, [showToast]);

	async function saveProfile() {
		setSavingProfile(true);
		try {
			const profile = await telemetryDataSource.setDriverProfile(profileName.trim() || null);
			setProfileName(profile.name ?? "");
			setSavedProfileName(profile.name ?? "");
			showToast({
				kind: "success",
				title: profile.name ? "Driver profile saved" : "Driver profile cleared",
				message: profile.name
					? `${profile.name} will identify your new captures and shared TRACE sessions.`
					: "Future captures will not receive a driver name.",
				timeoutMs: 5_000,
			});
		} catch (error) {
			showToast({
				kind: "error",
				title: "Could not save driver profile",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 8_000,
			});
		} finally {
			setSavingProfile(false);
		}
	}

	async function saveDirectory(simulatorId: string, customPath: string | null) {
		setSaving(simulatorId);
		try {
			const updated = await telemetryDataSource.setGameInstallDirectory(simulatorId, customPath);
			setDirectories((current) => current.map((value) => (value.simulatorId === simulatorId ? updated : value)));
			setDrafts((current) => ({ ...current, [simulatorId]: updated.path ?? "" }));
			showToast({
				kind: "success",
				title: customPath ? "Game folder saved" : "Automatic detection restored",
				message: updated.path ?? `${updated.simulatorName} was not detected.`,
				timeoutMs: 4_500,
			});
		} catch (error) {
			showToast({
				kind: "error",
				title: "Could not save game folder",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 8_000,
			});
		} finally {
			setSaving(null);
		}
	}

	async function saveLiveSettings() {
		const normalizedLiveEndpoint = normalizeServiceEndpoint(liveEndpoint);
		if (!normalizedLiveEndpoint) {
			showToast({
				kind: "error",
				title: "Invalid service endpoint",
				message: "Enter a complete HTTP or HTTPS URL for the Live service.",
				timeoutMs: 7_000,
			});
			return;
		}
		setSavingLiveSettings(true);
		try {
			const port = Number.parseInt(localSpectatorPort, 10);
			if (!Number.isInteger(port) || (port !== 0 && port < 1024) || port > 65535) throw new Error("Local port must be 0 or between 1024 and 65535.");
			if (autoStreamEnabled && acSessionTypes.length === 0) throw new Error("Choose at least one Assetto Corsa session type for automatic streaming.");
			const settings = await telemetryDataSource.setLiveSettings({
				endpoint: normalizedLiveEndpoint,
				discordActivityEnabled,
				autoStream: {
					enabled: autoStreamEnabled,
					mode: liveMode,
					localPort: port === 0 ? null : port,
					simulatorSessionTypes: { "assetto-corsa": acSessionTypes },
				},
			});
			setLiveEndpoint(settings.endpoint);
			setSavedLiveSettings(settings);
			setLocalSpectatorPort(String(settings.autoStream.localPort ?? 0));
			setAutoStreamEnabled(settings.autoStream.enabled);
			setDiscordActivityEnabled(settings.discordActivityEnabled);
			setLiveMode(settings.autoStream.mode);
			setAcSessionTypes(settings.autoStream.simulatorSessionTypes["assetto-corsa"] ?? []);
			window.dispatchEvent(new CustomEvent<LiveSettings>("trace:live-settings", { detail: settings }));
			showToast({
				kind: "success",
				title: "Connectivity settings saved",
				message: settings.autoStream.enabled
					? "Eligible simulator sessions will now start streaming automatically."
					: "Manual Go Live settings were updated.",
				timeoutMs: 5_000,
			});
		} catch (error) {
			showToast({
				kind: "error",
				title: "Could not save connectivity settings",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 8_000,
			});
		} finally {
			setSavingLiveSettings(false);
		}
	}

	async function chooseDirectory(directory: GameInstallDirectory) {
		try {
			const selected = await open({
				directory: true,
				multiple: false,
				defaultPath: drafts[directory.simulatorId]?.trim() || directory.path || undefined,
				title: `Choose ${directory.simulatorName} folder`,
			});
			if (typeof selected === "string") {
				setDrafts((current) => ({ ...current, [directory.simulatorId]: selected }));
				await saveDirectory(directory.simulatorId, selected);
			}
		} catch (error) {
			showToast({ kind: "error", title: "Folder picker unavailable", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		}
	}

	const updateBusy = updater.phase === "checking" || updater.phase === "downloading" || updater.phase === "installing" || updater.phase === "restarting";
	const updateStatus = !updater.supported
		? "Update checks are available in an installed TRACE desktop build."
		: updater.phase === "checking"
			? "Checking the release channel…"
			: updater.phase === "upToDate"
				? "TRACE is up to date."
				: updater.phase === "available"
					? `${updater.availableVersion} is available to download and install.`
					: updater.phase === "downloading"
						? `Downloading ${updater.availableVersion ?? "the update"}${updater.downloadProgress == null ? "…" : ` · ${updater.downloadProgress}%`}`
						: updater.phase === "installing"
							? `Installing ${updater.availableVersion ?? "the update"}…`
							: updater.phase === "restarting"
								? "The update is installed. Restarting TRACE…"
								: updater.phase === "failed"
									? updater.error || "TRACE could not complete the update request."
									: "TRACE checks for updates shortly after launch. You can also check manually.";
	const parsedLocalPort = Number.parseInt(localSpectatorPort, 10);
	const normalizedLocalPort = Number.isInteger(parsedLocalPort) && parsedLocalPort !== 0 ? parsedLocalPort : null;
	const savedAcSessionTypes = savedLiveSettings?.autoStream.simulatorSessionTypes["assetto-corsa"] ?? [];
	const liveSettingsChanged =
		savedLiveSettings == null ||
		liveEndpoint.trim() !== savedLiveSettings.endpoint ||
		autoStreamEnabled !== savedLiveSettings.autoStream.enabled ||
		discordActivityEnabled !== savedLiveSettings.discordActivityEnabled ||
		liveMode !== savedLiveSettings.autoStream.mode ||
		normalizedLocalPort !== (savedLiveSettings.autoStream.localPort ?? null) ||
		[...acSessionTypes].sort().join("|") !== [...savedAcSessionTypes].sort().join("|");

	function runUpdateAction() {
		if (updater.availableVersion) void updater.installUpdate();
		else void updater.checkForUpdates(true);
	}

	function handleTabKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
		let nextIndex: number | null = null;
		if (event.key === "ArrowRight") nextIndex = (index + 1) % settingsTabs.length;
		else if (event.key === "ArrowLeft") nextIndex = (index - 1 + settingsTabs.length) % settingsTabs.length;
		else if (event.key === "Home") nextIndex = 0;
		else if (event.key === "End") nextIndex = settingsTabs.length - 1;
		if (nextIndex == null) return;
		event.preventDefault();
		const nextTab = settingsTabs[nextIndex];
		setActiveTab(nextTab.id);
		window.requestAnimationFrame(() => document.getElementById(`settings-tab-${nextTab.id}`)?.focus());
	}

	return (
		<>
			<PageIntro
				index="06"
				eyebrow="PREFERENCES"
				title="SETTINGS"
				description="Control how TRACE connects to your simulators and works with their data. Recording, storage, analysis, and appearance preferences will also live here as those features become configurable."
			/>
			<div
				className="mt-7 grid grid-cols-4 border border-trace-divider bg-trace-surface max-[1050px]:grid-cols-2"
				role="tablist"
				aria-label="Settings categories"
			>
				{settingsTabs.map((tab, index) => {
					const selected = activeTab === tab.id;
					return (
						<button
							id={`settings-tab-${tab.id}`}
							key={tab.id}
							type="button"
							role="tab"
							aria-selected={selected}
							aria-controls={`settings-panel-${tab.id}`}
							tabIndex={selected ? 0 : -1}
							onClick={() => setActiveTab(tab.id)}
							onKeyDown={(event) => handleTabKeyDown(event, index)}
							className={`relative min-h-[72px] min-w-0 border-0 border-r border-trace-divider px-4 py-3 text-left last:border-r-0 max-[1050px]:border-b max-[1050px]:nth-[2]:border-r-0 max-[1050px]:nth-[3]:border-b-0 max-[1050px]:nth-[4]:border-b-0 ${
								selected ? "bg-trace-deep text-trace-text" : "bg-trace-surface text-trace-muted hover:bg-trace-raised hover:text-trace-text"
							}`}
						>
							<span className="block truncate font-mono text-[11px] font-black tracking-[.09em]">{tab.label}</span>
							<span className="mt-1 block truncate text-[11px] leading-4 text-trace-dim">{tab.description}</span>
							{selected && <span className="absolute inset-x-0 bottom-0 h-0.5 bg-trace-accent" aria-hidden="true" />}
						</button>
					);
				})}
			</div>
			<div id="settings-panel-general" role="tabpanel" aria-labelledby="settings-tab-general" hidden={activeTab !== "general"}>
				<form
					className="mt-7 border border-trace-divider bg-trace-surface"
					onSubmit={(event) => {
						event.preventDefault();
						void saveProfile();
					}}
				>
					<div className="border-b border-trace-divider px-5 py-4">
						<h2 className="text-[14px] font-black tracking-[.04em]">DRIVER PROFILE</h2>
						<p className="mt-1 max-w-4xl text-[12px] leading-5 text-trace-dim">
							Use a nickname or full name that other drivers will recognize. TRACE attaches it to new captures and includes it in shared{" "}
							<span className="font-mono text-trace-soft">.trace</span> packages; exports of older self-owned sessions use it when no
							session-specific driver is set.
						</p>
					</div>
					<label className="block p-5 text-[12px] font-bold tracking-[.08em] text-trace-dim">
						DISPLAY NAME
						<div className="mt-1.5 flex max-w-2xl">
							<input
								value={profileName}
								maxLength={80}
								onChange={(event) => setProfileName(event.target.value)}
								placeholder="Nickname or full name"
								className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 text-[13px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
							/>
							<button
								type="submit"
								disabled={savingProfile || profileName.trim() === savedProfileName}
								className="w-28 border border-l-0 border-trace-accent bg-trace-accent-wash text-[12px] font-bold text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
							>
								{savingProfile ? "SAVING…" : "SAVE"}
							</button>
						</div>
					</label>
				</form>
				<section className="mt-5 border border-trace-divider bg-trace-surface">
					<div className="border-b border-trace-divider px-5 py-4">
						<h2 className="text-[14px] font-black tracking-[.04em]">SETUP LIBRARY</h2>
						<p className="mt-1 max-w-4xl text-[12px] leading-5 text-trace-dim">
							Control whether TRACE discovers setup files already installed by supported simulators.
						</p>
					</div>
					<label className="flex cursor-pointer items-start gap-3 p-5">
						<input
							type="checkbox"
							checked={autoIndexSetups}
							disabled={indexingSetups}
							onChange={(event) => void changeAutoIndexSetups(event.target.checked)}
							className="mt-0.5 size-4 accent-[var(--color-trace-accent)]"
						/>
						<span>
							<strong className="block text-[12px] text-trace-text">Index existing setups automatically</strong>
							<span className="mt-1 block max-w-3xl text-[11px] leading-5 text-trace-dim">
								Off by default. Enabling this scans detected setup folders now and at startup. Large libraries can cause additional disk
								activity.
							</span>
						</span>
					</label>
				</section>
			</div>
			<div id="settings-panel-connectivity" role="tabpanel" aria-labelledby="settings-tab-connectivity" hidden={activeTab !== "connectivity"}>
				<form
					className="mt-7 border border-trace-divider bg-trace-surface"
					onSubmit={(event) => {
						event.preventDefault();
						void saveLiveSettings();
					}}
				>
					<div className="border-b border-trace-divider px-5 py-4">
						<h2 className="text-[14px] font-black tracking-[.04em]">GO LIVE</h2>
						<p className="mt-1 max-w-4xl text-[12px] leading-5 text-trace-dim">
							Choose the service TRACE will use to create sessions, publish realtime telemetry, and generate spectator links. Keep the hosted
							default or point TRACE at your own compatible deployment.
						</p>
					</div>
					<label className="block p-5 text-[12px] font-bold tracking-[.08em] text-trace-dim">
						LIVE SERVICE ENDPOINT
						<span className="mt-1 block max-w-4xl font-normal leading-5 normal-case tracking-normal text-trace-dim">
							One base URL for session creation, realtime publishing, spectator connections, and shareable browser links. Secure WebSocket URLs
							are derived automatically from HTTPS.
						</span>
						<div className="mt-2 flex">
							<input
								type="url"
								value={liveEndpoint}
								onChange={(event) => setLiveEndpoint(event.target.value)}
								placeholder={DEFAULT_LIVE_SERVICE_ENDPOINT}
								spellCheck={false}
								className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
							/>
							<button
								type="button"
								disabled={savingLiveSettings || liveEndpoint === DEFAULT_LIVE_SERVICE_ENDPOINT}
								onClick={() => setLiveEndpoint(DEFAULT_LIVE_SERVICE_ENDPOINT)}
								className="w-24 shrink-0 border border-l-0 border-trace-divider bg-trace-surface text-[10px] font-bold leading-none text-trace-soft hover:bg-trace-raised hover:text-trace-text disabled:bg-trace-deep disabled:text-trace-dim"
							>
								DEFAULT
							</button>
						</div>
						<span className="mt-2 block truncate font-mono text-[10px] font-normal normal-case tracking-normal text-trace-soft">
							{DEFAULT_LIVE_SERVICE_ENDPOINT}
						</span>
					</label>
					<section className="border-t border-trace-divider p-5" aria-labelledby="automatic-live-heading">
						<div className="flex items-start justify-between gap-6">
							<div>
								<h3 id="automatic-live-heading" className="text-[13px] font-black tracking-[.05em] text-trace-text">
									AUTOMATIC STREAMING
								</h3>
								<p className="mt-1 max-w-3xl text-[12px] leading-5 text-trace-dim">
									Start Go Live when TRACE detects an eligible simulator session. Recording remains local and continues normally if publishing
									fails.
								</p>
							</div>
							<button
								type="button"
								role="switch"
								aria-checked={autoStreamEnabled}
								onClick={() => setAutoStreamEnabled((value) => !value)}
								className={`relative mt-1 h-7 w-12 shrink-0 overflow-hidden border transition-colors ${autoStreamEnabled ? "border-trace-accent bg-trace-accent" : "border-trace-divider bg-trace-deep"}`}
							>
								<span
									className={`absolute left-1 top-1 size-[18px] bg-trace-black transition-transform ${autoStreamEnabled ? "translate-x-5" : "translate-x-0"}`}
								/>
								<span className="sr-only">Automatically stream eligible sessions</span>
							</button>
						</div>

						<div className="mt-5 grid gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
							<div>
								<span className="block font-mono text-[10px] font-bold tracking-[.1em] text-trace-dim">DESTINATION</span>
								<div className="mt-2 grid grid-cols-2 border border-trace-divider p-1" role="group" aria-label="Automatic stream destination">
									{(["local", "hosted"] as const).map((mode) => (
										<button
											type="button"
											key={mode}
											aria-pressed={liveMode === mode}
											onClick={() => setLiveMode(mode)}
											className={`h-9 text-[10px] font-black tracking-[.08em] ${liveMode === mode ? "bg-trace-accent text-trace-black" : "text-trace-dim hover:bg-trace-raised hover:text-trace-text"}`}
										>
											{mode === "local" ? "LOCAL" : "ONLINE"}
										</button>
									))}
								</div>
							</div>
							<div>
								<span className="block font-mono text-[10px] font-bold tracking-[.1em] text-trace-dim">ASSETTO CORSA SESSION TYPES</span>
								<div className="mt-2 flex flex-wrap gap-2">
									{AC_SESSION_TYPES.map((sessionType) => {
										const selected = acSessionTypes.includes(sessionType.id);
										return (
											<button
												type="button"
												key={sessionType.id}
												aria-pressed={selected}
												onClick={() =>
													setAcSessionTypes((values) =>
														selected ? values.filter((value) => value !== sessionType.id) : [...values, sessionType.id],
													)
												}
												className={`h-9 border px-3 text-[10px] font-bold tracking-[.05em] ${selected ? "border-trace-accent bg-trace-accent-wash text-trace-accent" : "border-trace-divider text-trace-dim hover:bg-trace-raised hover:text-trace-text"}`}
											>
												{sessionType.label.toUpperCase()}
											</button>
										);
									})}
								</div>
							</div>
						</div>

						<label className="mt-5 block text-[11px] font-bold tracking-[.08em] text-trace-dim">
							LOCAL SPECTATOR PORT
							<span className="mt-1 block max-w-3xl font-normal leading-5 normal-case tracking-normal">
								Use 0 to select an available port automatically, or choose a fixed port from 1024 to 65535.
							</span>
							<input
								type="number"
								min={0}
								max={65535}
								value={localSpectatorPort}
								onChange={(event) => setLocalSpectatorPort(event.target.value)}
								className="mt-2 h-10 w-40 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
							/>
						</label>
					</section>
					<section className="border-t border-trace-divider p-5" aria-labelledby="discord-activity-heading">
						<div className="flex items-start justify-between gap-6">
							<div>
								<h3 id="discord-activity-heading" className="text-[13px] font-black tracking-[.05em] text-trace-text">
									DISCORD ACTIVITY
								</h3>
								<p className="mt-1 max-w-3xl text-[12px] leading-5 text-trace-dim">
									Show your simulator, session type, car, and track in Discord while TRACE records. Online broadcasts include a public Watch
									Live button; local spectator addresses are never shared.
								</p>
							</div>
							<button
								type="button"
								role="switch"
								aria-checked={discordActivityEnabled}
								onClick={() => setDiscordActivityEnabled((value) => !value)}
								className={`relative mt-1 h-7 w-12 shrink-0 overflow-hidden border transition-colors ${discordActivityEnabled ? "border-trace-accent bg-trace-accent" : "border-trace-divider bg-trace-deep"}`}
							>
								<span
									className={`absolute left-1 top-1 size-[18px] bg-trace-black transition-transform ${discordActivityEnabled ? "translate-x-5" : "translate-x-0"}`}
								/>
								<span className="sr-only">Share active simulator sessions through Discord Rich Presence</span>
							</button>
						</div>
					</section>
					<div className="flex min-h-14 items-center justify-between gap-5 border-t border-trace-divider px-5 py-2">
						<span className="text-[11px] leading-5 text-trace-dim">
							{autoStreamEnabled
								? `${acSessionTypes.length} Assetto Corsa session type${acSessionTypes.length === 1 ? "" : "s"} selected.`
								: "Automatic streaming is off."}
						</span>
						<button
							type="submit"
							disabled={savingLiveSettings || !liveEndpoint.trim() || !liveSettingsChanged}
							className="h-10 w-28 shrink-0 border border-trace-accent bg-trace-accent-wash text-[12px] font-bold leading-none text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
						>
							{savingLiveSettings ? "SAVING…" : "SAVE"}
						</button>
					</div>
				</form>
			</div>
			<div id="settings-panel-updates" role="tabpanel" aria-labelledby="settings-tab-updates" hidden={activeTab !== "updates"}>
				<section className="mt-7 border border-trace-divider bg-trace-surface" aria-labelledby="application-update-heading">
					<div className="border-b border-trace-divider px-5 py-4">
						<h2 id="application-update-heading" className="text-[14px] font-black tracking-[.04em]">
							APPLICATION UPDATE
						</h2>
						<p className="mt-1 max-w-4xl text-[12px] leading-5 text-trace-dim">
							Check TRACE's signed GitHub release channel manually. Automatic checks remain enabled and use the same updater state.
						</p>
					</div>
					<div className="flex flex-wrap items-center justify-between gap-5 p-5">
						<div className="min-w-[260px] flex-1">
							<span className="font-mono text-[10px] font-bold tracking-[.1em] text-trace-dim">INSTALLED VERSION</span>
							<strong className="mt-1 block font-mono text-[15px] text-trace-text">{updater.currentVersion}</strong>
							<p
								className={`mt-2 text-[12px] leading-5 ${updater.phase === "failed" ? "text-trace-warning" : "text-trace-muted"}`}
								aria-live="polite"
							>
								{updateStatus}
							</p>
							{updater.phase === "downloading" && (
								<div className="mt-3 h-1.5 max-w-xl overflow-hidden bg-trace-divider" aria-hidden="true">
									<div
										className={`h-full bg-trace-accent transition-[width] ${updater.downloadProgress == null ? "w-1/3 animate-pulse" : ""}`}
										style={updater.downloadProgress == null ? undefined : { width: `${updater.downloadProgress}%` }}
									/>
								</div>
							)}
						</div>
						<button
							type="button"
							onClick={runUpdateAction}
							disabled={updateBusy}
							className="min-h-11 min-w-44 shrink-0 border border-trace-accent bg-trace-accent-wash px-4 font-mono text-[11px] font-bold tracking-[.06em] text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:cursor-wait disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
						>
							{updater.phase === "checking"
								? "CHECKING…"
								: updater.phase === "downloading" || updater.phase === "installing" || updater.phase === "restarting"
									? "UPDATING…"
									: updater.availableVersion
										? `${updater.phase === "failed" ? "TRY UPDATE" : "UPDATE NOW"} · ${updater.availableVersion}`
										: updater.phase === "failed"
											? "CHECK AGAIN"
											: "CHECK FOR UPDATES"}
						</button>
					</div>
				</section>
			</div>
			<div id="settings-panel-games" role="tabpanel" aria-labelledby="settings-tab-games" hidden={activeTab !== "games"}>
				<div className="mt-7 border border-trace-divider bg-trace-surface">
					<div className="border-b border-trace-divider px-5 py-4">
						<h2 className="text-[14px] font-black tracking-[.04em]">GAME FOLDERS</h2>
						<p className="mt-1 max-w-4xl text-[12px] leading-5 text-trace-dim">
							Game roots give each simulator adapter access to the files and metadata needed for content identification, replay and setup
							workflows, and future integrations. Choose the main game folder—not one of its subfolders.
						</p>
					</div>
					{loading ? (
						<div className="p-6 font-mono text-[12px] text-trace-dim">CHECKING INSTALLED GAMES…</div>
					) : directories.length === 0 ? (
						<div className="p-6 text-[12px] text-trace-dim">No configurable game adapters are installed.</div>
					) : (
						directories.map((directory) => {
							const draft = drafts[directory.simulatorId] ?? "";
							const unchanged = draft.trim() === (directory.path ?? "");
							return (
								<form
									className="p-5"
									key={directory.simulatorId}
									onSubmit={(event) => {
										event.preventDefault();
										void saveDirectory(directory.simulatorId, draft.trim() || null);
									}}
								>
									<div className="flex items-center justify-between gap-4">
										<div>
											<strong className="text-[14px] text-trace-text">{directory.simulatorName}</strong>
											<span
												className={`ml-3 inline-flex border px-2 py-1 font-mono text-[12px] font-bold tracking-[.08em] ${directory.source === "missing" ? "border-trace-warning/50 text-trace-warning" : directory.source === "manual" ? "border-trace-soft/50 text-trace-soft" : "border-trace-accent-muted text-trace-accent"}`}
											>
												{directory.source === "manual" ? "CUSTOM" : directory.source === "detected" ? "AUTO-DETECTED" : "NOT FOUND"}
											</span>
										</div>
										{directory.source === "manual" && (
											<button
												type="button"
												disabled={saving === directory.simulatorId}
												onClick={() => void saveDirectory(directory.simulatorId, null)}
												className="border-0 bg-transparent text-[12px] font-bold text-trace-muted hover:text-trace-text disabled:text-trace-dim"
											>
												USE AUTO-DETECTION
											</button>
										)}
									</div>
									<label className="mt-4 block text-[12px] font-bold tracking-[.08em] text-trace-dim">
										INSTALL DIRECTORY
										<div className="mt-1.5 flex">
											<input
												value={draft}
												onChange={(event) => setDrafts((current) => ({ ...current, [directory.simulatorId]: event.target.value }))}
												placeholder="C:\\Program Files (x86)\\Steam\\steamapps\\common\\assettocorsa"
												className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] font-normal tracking-normal text-trace-text outline-none focus:border-trace-accent"
											/>
											<button
												type="button"
												disabled={saving === directory.simulatorId}
												onClick={() => void chooseDirectory(directory)}
												className="flex h-11 w-28 items-center justify-center gap-2 border border-l-0 border-trace-divider bg-trace-surface text-[12px] font-bold text-trace-soft hover:bg-trace-raised hover:text-trace-text disabled:text-trace-dim"
											>
												<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
													<path d="M1.5 4.5h5l1.2 1.5h6.8v7.5h-13zM1.5 4.5V2.8h4.2l1.2 1.7" />
												</svg>
												BROWSE
											</button>
											<button
												type="submit"
												disabled={saving === directory.simulatorId || unchanged || !draft.trim()}
												className="w-24 border border-l-0 border-trace-accent bg-trace-accent-wash text-[12px] font-bold text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-trace-deep disabled:text-trace-dim"
											>
												{saving === directory.simulatorId ? "SAVING…" : "SAVE"}
											</button>
										</div>
									</label>
									<p className="mt-2 text-[12px] leading-5 text-trace-dim">
										{directory.path
											? `Currently using ${directory.source === "manual" ? "your custom path" : "the detected Steam installation"}.`
											: "TRACE could not locate this game automatically. Paste its installation folder above."}
									</p>
								</form>
							);
						})
					)}
				</div>
			</div>
		</>
	);
}
