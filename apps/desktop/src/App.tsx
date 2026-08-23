import { useEffect, useState } from "react";
import type { RecordedSessionSummary, TelemetryStatus } from "./data-source";
import { telemetryDataSource } from "./data-source";
import { Footer } from "./app/Footer";
import { Navigation, type Section } from "./app/Navigation";
import { ComparePage } from "./features/compare/ComparePage";
import { LivePage } from "./features/live/LivePage";
import { LapVisualizer } from "./features/sessions/LapVisualizer";
import { SessionDetail } from "./features/sessions/SessionDetail";
import { SessionsPage } from "./features/sessions/SessionsPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { SetupsPage } from "./features/setups/SetupsPage";
import { TitleBar } from "./TitleBar";

export function App() {
	const [status, setStatus] = useState<TelemetryStatus | null>(null);
	const [sessions, setSessions] = useState<RecordedSessionSummary[]>([]);
	const [section, setSection] = useState<Section>("LIVE");
	const [openSessionId, setOpenSessionId] = useState<string | null>(null);
	const [openLapIndex, setOpenLapIndex] = useState<number | null>(null);
	const openSession = sessions.find((session) => session.id === openSessionId) ?? null;

	async function selectSimulator(simulatorId: string) {
		await telemetryDataSource.selectSimulator(simulatorId);
		setStatus(await telemetryDataSource.getStatus());
	}

	useEffect(() => {
		void Promise.all([telemetryDataSource.getStatus(), telemetryDataSource.getSessions()]).then(([nextStatus, nextSessions]) => {
			setStatus(nextStatus);
			setSessions(nextSessions);
		});
	}, []);

	useEffect(() => {
		const timer = window.setInterval(() => {
			void telemetryDataSource.getStatus().then(setStatus);
			if (section === "SESSIONS") void telemetryDataSource.getSessions().then(setSessions);
		}, 1_000);
		return () => window.clearInterval(timer);
	}, [section]);

	return (
		<main className="grid h-screen grid-cols-[176px_1fr] grid-rows-[48px_minmax(0,1fr)_38px] bg-trace-base text-trace-text max-[900px]:grid-cols-[140px_1fr]">
			<TitleBar
				status={status}
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
				{section === "LIVE" && <LivePage status={status} onOpenSessions={() => setSection("SESSIONS")} onSelectSimulator={selectSimulator} />}
				{section === "SESSIONS" &&
					(openSession ? (
						openLapIndex == null ? (
							<SessionDetail session={openSession} onOpenLap={setOpenLapIndex} />
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
				{section === "SETUPS" && <SetupsPage />}
				{section === "SETTINGS" && <SettingsPage />}
			</section>
			<Footer status={status} />
		</main>
	);
}
