import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
	telemetryDataSource,
	type LiveBroadcastOptions,
	type LiveBroadcastStatus,
	type LiveSettings,
	type RecordedSessionSummary,
	type TelemetryStatus,
} from "./data-source";
import { Footer } from "./app/Footer";
import { Navigation, type Section } from "./app/Navigation";
import { ComparePage } from "./features/compare/ComparePage";
import { LivePage } from "./features/live/LivePage";
import { OverlaysPage } from "./features/overlays/OverlaysPage";
import { LapVisualizer } from "./features/sessions/LapVisualizer";
import { SessionDetail } from "./features/sessions/SessionDetail";
import { SessionsPage } from "./features/sessions/SessionsPage";
import { SessionSummaryModal } from "./features/sessions/SessionSummaryModal";
import { SettingsPage } from "./features/settings/SettingsPage";
import { SetupsPage } from "./features/setups/SetupsPage";
import { autoIndexSetupsEnabled, indexDetectedSetupLibraries } from "./features/setups/setup-preferences";
import { TitleBar } from "./TitleBar";
import { useToast } from "./Toast";

export function App() {
	const showToast = useToast();
	const [status, setStatus] = useState<TelemetryStatus | null>(null);
	const [liveBroadcast, setLiveBroadcast] = useState<LiveBroadcastStatus | null>(null);
	const [liveSettings, setLiveSettings] = useState<LiveSettings | null>(null);
	const liveMode = liveSettings?.autoStream.mode ?? "hosted";
	const [sessions, setSessions] = useState<RecordedSessionSummary[]>([]);
	const [section, setSection] = useState<Section>("LIVE");
	const [openSessionId, setOpenSessionId] = useState<string | null>(null);
	const [openLapIndex, setOpenLapIndex] = useState<number | null>(null);
	const [summarySessionId, setSummarySessionId] = useState<string | null>(null);
	const announcedAutomaticSession = useRef<string | null>(null);
	const announcedAutomaticError = useRef<string | null>(null);
	const handledCompletedSession = useRef<string | null>(null);
	const openSession = sessions.find((session) => session.id === openSessionId) ?? null;
	const summarySession = sessions.find((session) => session.id === summarySessionId) ?? null;

	useEffect(() => {
		if (autoIndexSetupsEnabled()) void indexDetectedSetupLibraries();
	}, []);
	const selectLiveMode = async (mode: LiveBroadcastOptions["mode"]) => {
		if (!liveSettings || liveSettings.autoStream.mode === mode) return;
		try {
			setLiveSettings(
				await telemetryDataSource.setLiveSettings({
					...liveSettings,
					autoStream: { ...liveSettings.autoStream, mode },
				}),
			);
		} catch (error) {
			showToast({
				kind: "error",
				title: "Could not change Go Live destination",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 8_000,
			});
		}
	};
	const liveOptions = (): LiveBroadcastOptions => {
		if (liveMode === "hosted") return { mode: "hosted" };
		return { mode: "local", localPort: liveSettings?.autoStream.localPort ?? undefined };
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
		void Promise.all([
			telemetryDataSource.getStatus(),
			telemetryDataSource.getSessions(),
			telemetryDataSource.getLiveBroadcastStatus(),
			telemetryDataSource.getLiveSettings(),
		]).then(([nextStatus, nextSessions, nextLiveBroadcast, nextLiveSettings]) => {
			setStatus(nextStatus);
			setSessions(nextSessions);
			setLiveBroadcast(nextLiveBroadcast);
			setLiveSettings(nextLiveSettings);
		});
	}, []);

	useEffect(() => {
		const timer = window.setInterval(() => {
			void telemetryDataSource.getStatus().then(setStatus);
			void telemetryDataSource.getLiveBroadcastStatus().then(setLiveBroadcast);
			if (section === "SESSIONS" || section === "OVERLAYS") void telemetryDataSource.getSessions().then(setSessions);
		}, 1_000);
		return () => window.clearInterval(timer);
	}, [section]);

	useEffect(() => {
		const update = (event: Event) => setLiveSettings((event as CustomEvent<LiveSettings>).detail);
		window.addEventListener("trace:live-settings", update);
		return () => window.removeEventListener("trace:live-settings", update);
	}, []);

	useEffect(() => {
		const completedSessionId = status?.completedSessionId;
		if (!completedSessionId || handledCompletedSession.current === completedSessionId) return;
		handledCompletedSession.current = completedSessionId;
		void telemetryDataSource.getSessions().then((nextSessions) => {
			if (!nextSessions.some((session) => session.id === completedSessionId)) return;
			setSessions(nextSessions);
			setSection("SESSIONS");
			setOpenSessionId(completedSessionId);
			setOpenLapIndex(null);
			setSummarySessionId(completedSessionId);
		});
	}, [status?.completedSessionId]);

	useEffect(() => {
		if (!liveBroadcast?.automatic) return;
		if (liveBroadcast.phase === "live" && liveBroadcast.liveSessionId && announcedAutomaticSession.current !== liveBroadcast.liveSessionId) {
			announcedAutomaticSession.current = liveBroadcast.liveSessionId;
			showToast({
				kind: "success",
				title: "Session went live automatically",
				message: liveBroadcast.spectatorUrl ?? "TRACE is publishing the current simulator session.",
				timeoutMs: 6_000,
			});
		}
		if (liveBroadcast.phase === "error" && liveBroadcast.error && announcedAutomaticError.current !== liveBroadcast.error) {
			announcedAutomaticError.current = liveBroadcast.error;
			showToast({ kind: "error", title: "Automatic Go Live failed", message: liveBroadcast.error, timeoutMs: 9_000 });
		}
	}, [liveBroadcast, showToast]);

	return (
		<main className="grid h-screen grid-cols-[var(--trace-sidebar)_1fr] grid-rows-[48px_minmax(0,1fr)_38px] bg-trace-base text-trace-text [--trace-sidebar:200px] max-[900px]:[--trace-sidebar:156px]">
			<TitleBar
				status={status}
				liveBroadcast={liveBroadcast}
				liveMode={liveMode}
				onLiveModeChange={(mode) => void selectLiveMode(mode)}
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
								onShowSummary={() => setSummarySessionId(openSession.id)}
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
			{summarySession && <SessionSummaryModal session={summarySession} sessions={sessions} onClose={() => setSummarySessionId(null)} />}
		</main>
	);
}
