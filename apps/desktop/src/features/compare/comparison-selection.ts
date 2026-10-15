import type { RecordedLapSummary, RecordedSessionSummary } from "../../data-source";
import { lapDuration, lapIsInvalid, sessionSourceGroup } from "../sessions/session-components";

export interface ComparisonSelection {
	referenceSessionId: string;
	referenceLapIndex: number;
	analysedSessionId: string;
	analysedLapIndex: number;
}

export interface ReferenceSuggestion {
	session: RecordedSessionSummary;
	lap: RecordedLapSummary;
	imported: boolean;
	gainSeconds: number;
}

export function validComparisonLaps(session: RecordedSessionSummary) {
	return session.laps
		.filter((lap) => !lapIsInvalid(lap) && lap.time !== "—")
		.slice()
		.sort((left, right) => lapDuration(left) - lapDuration(right));
}

export function fasterReferenceSuggestions(
	sessions: RecordedSessionSummary[],
	targetSession: RecordedSessionSummary | undefined,
	targetLap: RecordedLapSummary | null,
	targetSessionId: string,
	targetLapIndex: number | null,
) {
	if (!targetSession || !targetLap) return [];
	const targetDuration = lapDuration(targetLap);
	return sessions
		.filter((session) => session.simulatorId === targetSession.simulatorId && session.track === targetSession.track && session.car === targetSession.car)
		.flatMap((session) =>
			validComparisonLaps(session)
				.filter((lap) => lapDuration(lap) < targetDuration && (session.id !== targetSessionId || lap.index !== targetLapIndex))
				.map((lap): ReferenceSuggestion => ({
					session,
					lap,
					imported: sessionSourceGroup(session) === "imported",
					gainSeconds: (targetDuration - lapDuration(lap)) / 1_000_000_000,
				})),
		)
		.sort((left, right) => Number(right.imported) - Number(left.imported) || lapDuration(left.lap) - lapDuration(right.lap))
		.slice(0, 10);
}
