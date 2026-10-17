import type { RecordedSessionSummary } from "../../data-source";

export interface RankedDrivingItem {
	key: string;
	label: string;
	detail?: string;
	sessionCount: number;
	lapCount: number;
}

export interface ComboSessionPoint {
	sessionId: string;
	startedAt: string;
	bestLapNs: number;
}

export interface CombinationStat {
	key: string;
	simulatorId: string;
	simulatorName: string;
	track: string;
	car: string;
	sessionCount: number;
	lapCount: number;
	bestLapNs: number;
	bestSessionId: string;
	bestStartedAt: string;
	bestDriver?: string | null;
	points: ComboSessionPoint[];
}

export interface DrivingStats {
	sessionCount: number;
	lapCount: number;
	drivingTimeNs: number;
	combinationCount: number;
	simulators: RankedDrivingItem[];
	tracks: RankedDrivingItem[];
	cars: RankedDrivingItem[];
	combinations: CombinationStat[];
}

function isDrivenSession(session: RecordedSessionSummary) {
	const source = session.source.toLocaleLowerCase();
	return !source.includes("import") && !source.includes("replay");
}

function completedLaps(session: RecordedSessionSummary) {
	return session.laps.filter((lap) => typeof lap.durationNs === "number" && lap.durationNs > 0);
}

function paceLaps(session: RecordedSessionSummary) {
	return completedLaps(session).filter((lap) => lap.validity !== "invalid");
}

function rank(
	sessions: RecordedSessionSummary[],
	identity: (session: RecordedSessionSummary) => { key: string; label: string; detail?: string },
): RankedDrivingItem[] {
	const values = new Map<string, RankedDrivingItem>();
	for (const session of sessions) {
		const item = identity(session);
		const current = values.get(item.key) ?? { ...item, sessionCount: 0, lapCount: 0 };
		current.sessionCount += 1;
		current.lapCount += completedLaps(session).length;
		values.set(item.key, current);
	}
	return [...values.values()].sort(
		(left, right) => right.lapCount - left.lapCount || right.sessionCount - left.sessionCount || left.label.localeCompare(right.label),
	);
}

export function calculateDrivingStats(allSessions: RecordedSessionSummary[]): DrivingStats {
	const sessions = allSessions.filter(isDrivenSession);
	const combinations = new Map<string, CombinationStat>();
	let lapCount = 0;
	let drivingTimeNs = 0;

	for (const session of sessions) {
		const completed = completedLaps(session);
		const laps = paceLaps(session);
		lapCount += completed.length;
		drivingTimeNs += completed.reduce((total, lap) => total + (lap.durationNs ?? 0), 0);
		if (laps.length === 0) continue;
		const bestLapNs = Math.min(...laps.map((lap) => lap.durationNs as number));
		const key = [session.simulatorId, session.track, session.car].join("\u0000");
		const current = combinations.get(key);
		if (!current) {
			combinations.set(key, {
				key,
				simulatorId: session.simulatorId,
				simulatorName: session.simulatorName,
				track: session.track,
				car: session.car,
				sessionCount: 1,
				lapCount: completed.length,
				bestLapNs,
				bestSessionId: session.id,
				bestStartedAt: session.startedAt,
				bestDriver: session.driver,
				points: [{ sessionId: session.id, startedAt: session.startedAt, bestLapNs }],
			});
			continue;
		}
		current.sessionCount += 1;
		current.lapCount += completed.length;
		current.points.push({ sessionId: session.id, startedAt: session.startedAt, bestLapNs });
		if (bestLapNs < current.bestLapNs) {
			current.bestLapNs = bestLapNs;
			current.bestSessionId = session.id;
			current.bestStartedAt = session.startedAt;
			current.bestDriver = session.driver;
		}
	}

	const combinationList = [...combinations.values()]
		.map((combination) => ({
			...combination,
			points: combination.points.sort((left, right) => new Date(left.startedAt).getTime() - new Date(right.startedAt).getTime()),
		}))
		.sort((left, right) => right.lapCount - left.lapCount || left.track.localeCompare(right.track));

	return {
		sessionCount: sessions.length,
		lapCount,
		drivingTimeNs,
		combinationCount: combinationList.length,
		simulators: rank(sessions, (session) => ({ key: session.simulatorId, label: session.simulatorName })),
		tracks: rank(sessions, (session) => ({ key: `${session.simulatorId}\u0000${session.track}`, label: session.track, detail: session.simulatorName })),
		cars: rank(sessions, (session) => ({ key: `${session.simulatorId}\u0000${session.car}`, label: session.car, detail: session.simulatorName })),
		combinations: combinationList,
	};
}
