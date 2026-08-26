import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { telemetryDataSource, type LiveBroadcastOptions, type LiveBroadcastStatus, type RecordedSessionSummary, type TelemetryStatus } from "./data-source";
import { Footer } from "./app/Footer";
import { Navigation, type Section } from "./app/Navigation";
import { ComparePage } from "./features/compare/ComparePage";
import { LivePage } from "./features/live/LivePage";
import { OverlaysPage } from "./features/overlays/OverlaysPage";
import { LapVisualizer } from "./features/sessions/LapVisualizer";
import { SessionDetail } from "./features/sessions/SessionDetail";
import { SessionsPage } from "./features/sessions/SessionsPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { SetupsPage } from "./features/setups/SetupsPage";
import { TitleBar } from "./TitleBar";
import { useToast } from "./Toast";

export function App() {
	const showToast = useToast();
	const [status, setStatus] = useState<TelemetryStatus | null>(null);
	const [liveBroadcast, setLiveBroadcast] = useState<LiveBroadcastStatus | null>(null);
	const [liveMode, setLiveMode] = useState<LiveBroadcastOptions["mode"]>(() =>
		window.localStorage.getItem("trace.liveBroadcastMode") === "local" ? "local" : "hosted",
	);
	const [sessions, setSessions] = useState<RecordedSessionSummary[]>([]);
	const [section, setSection] = useState<Section>("LIVE");
	const [openSessionId, setOpenSessionId] = useState<string | null>(null);
	const [openLapIndex, setOpenLapIndex] = useState<number | null>(null);
	const openSession = sessions.find((session) => session.id === openSessionId) ?? null;
	const selectLiveMode = (mode: LiveBroadcastOptions["mode"]) => {
		setLiveMode(mode);
		window.localStorage.setItem("trace.liveBroadcastMode", mode);
	};
	const liveOptions = (): LiveBroadcastOptions => {
		if (liveMode === "hosted") return { mode: "hosted" };
		const value = Number.parseInt(window.localStorage.getItem("trace.localSpectatorPort") ?? "", 10);
		return { mode: "local", localPort: Number.isInteger(value) && value >= 1024 && value <= 65535 ? value : undefined };
	};

	async function selectSimulator(simulatorId: string) {
		await telemetryDataSource.selectSimulator(simulatorId);
		setStatus(await telemetryDataSource.getStatus());
	}

	async function startRecordedBroadcast(sessionId: string, options: LiveBroadcastOptions) {
		try {
			const next = await telemetryDataSource.startRecordedLiveBroadcast(sessionId, options);
			setLiveBroadcast(next);
			showToast({
				kind: "success",
				title: "Preparing live replay",
				message:
					options.mode === "local"
						? "TRACE is starting a local spectator service and loading the recording."
						: "TRACE is loading the recording and connecting to the configured Go Live service.",
				timeoutMs: 4_500,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Could not start Go Live", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
		}
	}

	async function startActiveBroadcast(options: LiveBroadcastOptions) {
		try {
			setLiveBroadcast(await telemetryDataSource.startActiveLiveBroadcast(options));
			showToast({
				kind: "success",
				title: "Connecting active session",
				message: options.mode === "local" ? "TRACE is starting a local spectator screen." : "TRACE is publishing the current simulator session.",
				timeoutMs: 4_500,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Could not start Go Live", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
		}
	}

	async function stopLiveBroadcast() {
		try {
			setLiveBroadcast(await telemetryDataSource.stopLiveBroadcast());
			showToast({ kind: "success", title: "Ending live session", message: "TRACE is closing the publisher and spectator session.", timeoutMs: 4_000 });
		} catch (error) {
			showToast({ kind: "error", title: "Could not stop Go Live", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
		}
	}

	async function copyLiveLink() {
		if (!liveBroadcast?.spectatorUrl) return;
		try {
			await navigator.clipboard.writeText(liveBroadcast.spectatorUrl);
			showToast({ kind: "success", title: "Live link copied", message: liveBroadcast.spectatorUrl, timeoutMs: 5_000 });
		} catch (error) {
			showToast({ kind: "error", title: "Could not copy live link", message: error instanceof Error ? error.message : String(error), timeoutMs: 7_000 });
		}
	}

	async function openLiveLink() {
		if (!liveBroadcast?.spectatorUrl) return;
		try {
			await openUrl(liveBroadcast.spectatorUrl);
		} catch (error) {
			showToast({ kind: "error", title: "Could not open live link", message: error instanceof Error ? error.message : String(error), timeoutMs: 7_000 });
		}
	}

	useEffect(() => {
		void Promise.all([telemetryDataSource.getStatus(), telemetryDataSource.getSessions(), telemetryDataSource.getLiveBroadcastStatus()]).then(
			([nextStatus, nextSessions, nextLiveBroadcast]) => {
				setStatus(nextStatus);
				setSessions(nextSessions);
				setLiveBroadcast(nextLiveBroadcast);
			},
		);
	}, []);

	useEffect(() => {
		const timer = window.setInterval(() => {
			void telemetryDataSource.getStatus().then(setStatus);
			void telemetryDataSource.getLiveBroadcastStatus().then(setLiveBroadcast);
			if (section === "SESSIONS" || section === "OVERLAYS") void telemetryDataSource.getSessions().then(setSessions);
		}, 1_000);
		return () => window.clearInterval(timer);
	}, [section]);

	return (
		<main className="grid h-screen grid-cols-[var(--trace-sidebar)_1fr] grid-rows-[48px_minmax(0,1fr)_38px] bg-trace-base text-trace-text [--trace-sidebar:200px] max-[900px]:[--trace-sidebar:156px]">
			<TitleBar
				status={status}
				liveBroadcast={liveBroadcast}
				liveMode={liveMode}
				onLiveModeChange={selectLiveMode}
				onStopLive={() => void stopLiveBroadcast()}
				backLabel={openLapIndex == null ? "SESSIONS" : "SESSION"}
				onBack={
					openSession
						? () => {
								if (openLapIndex != null) setOpenLapIndex(null);
								else setOpenSessionId(null);
							}
						: undefined
				}
			/>
			<Navigation
				active={section}
				onChange={(next) => {
					setSection(next);
					if (next !== "SESSIONS") {
						setOpenSessionId(null);
						setOpenLapIndex(null);
					}
				}}
			/>
			<section className="trace-grid overflow-auto p-7">
				{section === "LIVE" && (
					<LivePage
						status={status}
						liveBroadcast={liveBroadcast}
						onStartLive={() => void startActiveBroadcast(liveOptions())}
						onStopLive={() => void stopLiveBroadcast()}
						onCopyLiveLink={() => void copyLiveLink()}
						onOpenLiveLink={() => void openLiveLink()}
						onOpenSessions={() => setSection("SESSIONS")}
						onSelectSimulator={selectSimulator}
					/>
				)}
				{section === "SESSIONS" &&
					(openSession ? (
						openLapIndex == null ? (
							<SessionDetail
								session={openSession}
								onOpenLap={setOpenLapIndex}
								liveBroadcast={liveBroadcast}
								onStartLive={() => void startRecordedBroadcast(openSession.id, liveOptions())}
								onStopLive={() => void stopLiveBroadcast()}
								onCopyLiveLink={() => void copyLiveLink()}
								onOpenLiveLink={() => void openLiveLink()}
							/>
						) : (
							<LapVisualizer session={openSession} lapIndex={openLapIndex} />
						)
					) : (
						<SessionsPage
							sessions={sessions}
							onOpen={(sessionId) => {
								setOpenSessionId(sessionId);
								setOpenLapIndex(null);
							}}
							onDeleted={(sessionId) => setSessions((current) => current.filter((session) => session.id !== sessionId))}
							onUpdated={(updated) => setSessions((current) => current.map((session) => (session.id === updated.id ? updated : session)))}
							onImported={async () => setSessions(await telemetryDataSource.getSessions())}
						/>
					))}
				{section === "COMPARE" && <ComparePage sessions={sessions} />}
				{section === "OVERLAYS" && <OverlaysPage sessions={sessions} status={status} />}
				{section === "SETUPS" && <SetupsPage />}
				{section === "SETTINGS" && <SettingsPage />}
			</section>
			<Footer status={status} />
		</main>
	);
}
