import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { formatLapDurationNs, sectorPerformance, sessionOverview, type SessionOverviewLap } from "@trace/ui";

type Sector = {
	index: number;
	duration_ns: number | null;
};

type Lap = {
	id: string;
	index: number;
	duration_ns: number | null;
	validity: string;
	max_tyres_out?: number | null;
	sectors: Sector[];
};

type LapListSnapshot = {
	laps: Lap[];
	selectedLapId: string | null;
};

declare global {
	interface Window {
		traceLapListSnapshot?: LapListSnapshot | null;
	}
}

function LapList() {
	const [snapshot, setSnapshot] = useState<LapListSnapshot | null>(window.traceLapListSnapshot || null);

	useEffect(() => {
		const receive = (event: Event) => setSnapshot((event as CustomEvent<LapListSnapshot | null>).detail);
		window.addEventListener("trace:laps-load", receive);
		return () => window.removeEventListener("trace:laps-load", receive);
	}, []);

	const timingLaps = useMemo<SessionOverviewLap[]>(() => {
		return (snapshot?.laps || []).map((lap) => ({
			id: lap.id,
			index: lap.index,
			durationNs: lap.duration_ns,
			invalid: lap.validity === "invalid" || (lap.max_tyres_out != null && lap.max_tyres_out >= 3),
			sectors: lap.sectors.map((sector) => ({ index: sector.index, durationNs: sector.duration_ns })),
		}));
	}, [snapshot]);
	const overview = useMemo(() => sessionOverview(timingLaps), [timingLaps]);

	if (!snapshot) return null;
	const { laps, selectedLapId } = snapshot;

	return (
		<>
			<div className="panel-heading">
				<strong>LAPS</strong>
				<span>
					{laps.length} {laps.length === 1 ? "lap" : "laps"}
				</span>
			</div>
			<div className="lap-list">
				{laps.map((lap) => {
					const timingLap = timingLaps.find((candidate) => candidate.id === lap.id)!;
					const visibleSectors = lap.sectors.slice(0, 3);
					const hiddenSectorCount = Math.max(0, lap.sectors.length - visibleSectors.length);
					const classes = [
						"lap-row",
						lap.id === selectedLapId ? "active" : "",
						lap.id === overview.bestLapId ? "best" : "",
						timingLap.invalid ? "invalid" : "",
					]
						.filter(Boolean)
						.join(" ");
					return (
						<button
							className={classes}
							key={lap.id}
							type="button"
							onClick={() => window.dispatchEvent(new CustomEvent("trace:lap-open", { detail: { lapId: lap.id } }))}
						>
							<span className="lap-identity">
								<small>LAP {lap.index}</small>
								<strong>{formatLapDurationNs(lap.duration_ns)}</strong>
							</span>
							<span className="sector-list">
								{visibleSectors.length
									? visibleSectors.map((sector) => (
											<span className={`sector-time ${sectorPerformance(timingLaps, timingLap, sector.index)}`} key={sector.index}>
												<i aria-hidden="true" /> S{sector.index} {formatLapDurationNs(sector.duration_ns)}
											</span>
										))
									: "No sectors"}
								{hiddenSectorCount > 0 && <span className="sector-overflow">+{hiddenSectorCount} more</span>}
							</span>
						</button>
					);
				})}
			</div>
		</>
	);
}

const root = document.querySelector("#review-laps-root");
if (root) createRoot(root).render(<LapList />);

export { LapList };
