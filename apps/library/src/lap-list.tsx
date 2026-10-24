import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";

type Sector = {
	index: number;
	duration_ns: number | null;
};

type Lap = {
	id: string;
	index: number;
	duration_ns: number | null;
	validity: string;
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

function formatDuration(nanoseconds: number | null) {
	if (nanoseconds == null) return "—";
	const milliseconds = Math.round(nanoseconds / 1_000_000);
	const minutes = Math.floor(milliseconds / 60_000);
	const seconds = Math.floor((milliseconds % 60_000) / 1000);
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds % 1000).padStart(3, "0")}`;
}

function LapList() {
	const [snapshot, setSnapshot] = useState<LapListSnapshot | null>(window.traceLapListSnapshot || null);

	useEffect(() => {
		const receive = (event: Event) => setSnapshot((event as CustomEvent<LapListSnapshot | null>).detail);
		window.addEventListener("trace:laps-load", receive);
		return () => window.removeEventListener("trace:laps-load", receive);
	}, []);

	const bestId = useMemo(() => {
		if (!snapshot) return null;
		const timed = snapshot.laps.filter((lap) => lap.duration_ns != null);
		return timed.length ? timed.reduce((best, lap) => (lap.duration_ns! < best.duration_ns! ? lap : best)).id : null;
	}, [snapshot]);

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
					const classes = [
						"lap-row",
						lap.id === selectedLapId ? "active" : "",
						lap.id === bestId ? "best" : "",
						lap.validity !== "valid" ? "invalid" : "",
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
								<strong>{formatDuration(lap.duration_ns)}</strong>
							</span>
							<span className="sector-list">
								{lap.sectors.length
									? lap.sectors.map((sector) => (
											<span key={sector.index}>
												S{sector.index} {formatDuration(sector.duration_ns)}
											</span>
										))
									: "No sectors"}
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
