import { useEffect, useMemo, useState } from "react";
import { telemetryDataSource, type SetupLibraryEntry } from "../../data-source";
import { formatCompactSessionDate } from "../sessions/session-components";

interface TrackGroup {
	key: string;
	name: string;
	rawName: string;
	layoutId?: string | null;
	setups: SetupLibraryEntry[];
}

interface CarGroup {
	key: string;
	name: string;
	rawName: string;
	tracks: TrackGroup[];
}

interface SimulatorGroup {
	key: string;
	name: string;
	cars: CarGroup[];
}

export function SetupExplorer({ refreshKey }: { refreshKey: number }) {
	const [entries, setEntries] = useState<SetupLibraryEntry[]>([]);
	const [state, setState] = useState<"loading" | "ready" | "error">("loading");
	const [query, setQuery] = useState("");
	const [simulatorId, setSimulatorId] = useState("all");

	useEffect(() => {
		let active = true;
		setState("loading");
		void telemetryDataSource
			.getSetupLibrary()
			.then((values) => {
				if (!active) return;
				setEntries(values);
				setState("ready");
			})
			.catch(() => {
				if (active) setState("error");
			});
		return () => {
			active = false;
		};
	}, [refreshKey]);

	const simulators = useMemo(
		() => [...new Map(entries.map((entry) => [entry.simulatorId, entry.simulatorName])).entries()].sort((left, right) => left[1].localeCompare(right[1])),
		[entries],
	);
	const filtered = useMemo(() => {
		const needle = query.trim().toLocaleLowerCase();
		return entries.filter(
			(entry) =>
				(simulatorId === "all" || entry.simulatorId === simulatorId) &&
				(!needle ||
					[
						entry.name,
						entry.simulatorName,
						entry.simulatorId,
						entry.carName,
						entry.sourceCarId,
						entry.trackName,
						entry.sourceTrackId,
						entry.layoutId,
						entry.sourceArchive,
					].some((value) => value?.toLocaleLowerCase().includes(needle))),
		);
	}, [entries, query, simulatorId]);
	const groups = useMemo(() => groupSetups(filtered), [filtered]);

	return (
		<section className="mt-4 border border-trace-divider bg-trace-surface" aria-labelledby="setup-library-heading">
			<div className="flex flex-wrap items-end justify-between gap-4 border-b border-trace-divider px-5 py-4">
				<div>
					<h2 id="setup-library-heading" className="text-[14px] font-black tracking-[.04em] text-white">
						LOCAL SETUP LIBRARY
					</h2>
					<p className="mt-1 text-[11px] leading-4 text-trace-dim">Indexed setup files grouped by their simulator-provided identities.</p>
				</div>
				<span className="font-mono text-[10px] font-bold text-trace-dim">{entries.length.toLocaleString()} SETUPS</span>
			</div>
			<div className="grid gap-3 border-b border-trace-divider bg-trace-deep p-4 sm:grid-cols-[minmax(0,1fr)_220px]">
				<label className="min-w-0">
					<span className="sr-only">Search setup library</span>
					<input
						value={query}
						onChange={(event) => setQuery(event.target.value)}
						placeholder="Search setup, car, track, layout, or raw ID"
						className="h-10 w-full border border-trace-divider bg-trace-black px-3 text-[12px] text-trace-text outline-none focus:border-trace-accent"
					/>
				</label>
				<select
					value={simulatorId}
					onChange={(event) => setSimulatorId(event.target.value)}
					className="trace-select h-10 border border-trace-divider bg-trace-black px-3 text-[12px] font-bold text-trace-text outline-none"
					aria-label="Filter setup library by simulator"
				>
					<option value="all">All simulators</option>
					{simulators.map(([id, name]) => (
						<option value={id} key={id}>
							{name}
						</option>
					))}
				</select>
			</div>
			{state === "loading" ? (
				<p className="p-8 font-mono text-[11px] text-trace-dim">LOADING SETUP LIBRARY…</p>
			) : state === "error" ? (
				<p className="p-8 text-[12px] text-trace-warning">TRACE could not read the local setup library.</p>
			) : groups.length === 0 ? (
				<div className="p-8 text-center">
					<strong className="text-[14px] text-trace-soft">{entries.length === 0 ? "No setups indexed yet" : "No setups match this search"}</strong>
					<p className="mx-auto mt-2 max-w-xl text-[12px] leading-5 text-trace-dim">
						{entries.length === 0
							? "Use Import or Index existing to add simulator setup files to this library."
							: "Try a setup name, friendly content name, or raw simulator identifier."}
					</p>
				</div>
			) : (
				<div className="divide-y divide-trace-divider">
					{groups.map((simulator) => (
						<section key={simulator.key}>
							<div className="flex items-center justify-between bg-trace-black px-5 py-3">
								<div>
									<strong className="text-[13px] text-white">{simulator.name}</strong>
									{simulator.name !== simulator.key && <span className="ml-2 font-mono text-[9px] text-trace-dim">{simulator.key}</span>}
								</div>
								<span className="font-mono text-[9px] text-trace-dim">{simulator.cars.length} CARS</span>
							</div>
							<div className="divide-y divide-trace-divider">
								{simulator.cars.map((car) => (
									<div className="grid min-w-0 lg:grid-cols-[240px_minmax(0,1fr)]" key={car.key}>
										<div className="border-b border-trace-divider bg-trace-deep px-5 py-4 lg:border-b-0 lg:border-r">
											<strong className="block text-[13px] leading-5 text-trace-text">{car.name}</strong>
											<span className="mt-1 block break-all font-mono text-[9px] leading-4 text-trace-dim">{car.rawName}</span>
											<span className="mt-2 block font-mono text-[9px] text-trace-muted">{car.tracks.length} TRACKS</span>
										</div>
										<div className="divide-y divide-trace-divider">
											{car.tracks.map((track) => (
												<TrackSetups track={track} forceOpen={Boolean(query.trim())} key={track.key} />
											))}
										</div>
									</div>
								))}
							</div>
						</section>
					))}
				</div>
			)}
		</section>
	);
}

function TrackSetups({ track, forceOpen }: { track: TrackGroup; forceOpen: boolean }) {
	return (
		<details className="group" open={forceOpen || undefined}>
			<summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-4 py-3 hover:bg-trace-deep">
				<span className="min-w-0">
					<strong className="block text-[12px] text-trace-soft">{track.name}</strong>
					<span className="mt-0.5 block break-all font-mono text-[9px] text-trace-dim">
						{track.rawName}
						{track.layoutId ? ` · ${track.layoutId}` : ""}
					</span>
				</span>
				<span className="flex shrink-0 items-center gap-2 font-mono text-[9px] font-bold text-trace-muted">
					{track.setups.length} SETUPS
					<svg className="size-3 fill-none stroke-current transition-transform group-open:rotate-180" viewBox="0 0 12 12" aria-hidden="true">
						<path d="m2.5 4 3.5 3.5L9.5 4" />
					</svg>
				</span>
			</summary>
			<div className="divide-y divide-trace-divider border-t border-trace-divider bg-trace-black/30">
				{track.setups.map((setup) => (
					<article className="grid gap-2 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center" key={setup.id}>
						<div className="min-w-0">
							<strong className="block break-words text-[12px] text-trace-text">{setup.name}</strong>
							<p className="mt-1 break-words text-[10px] leading-4 text-trace-dim">
								{setup.sourceArchive ?? "Local setup"} · indexed {formatCompactSessionDate(setup.importedAt)}
							</p>
						</div>
						<div className="flex flex-wrap items-center gap-1.5 sm:justify-end">
							{setup.linkedSessionCount > 0 && (
								<span className="border border-trace-divider px-1.5 py-1 font-mono text-[8px] font-bold text-trace-soft">
									USED BY {setup.linkedSessionCount} SESSION{setup.linkedSessionCount === 1 ? "" : "S"}
								</span>
							)}
							{!setup.available && (
								<span className="border border-trace-warning/50 px-1.5 py-1 font-mono text-[8px] font-bold text-trace-warning">
									FILE MISSING
								</span>
							)}
						</div>
					</article>
				))}
			</div>
		</details>
	);
}

function groupSetups(entries: SetupLibraryEntry[]): SimulatorGroup[] {
	const simulators = new Map<string, { name: string; cars: Map<string, { name: string; rawName: string; tracks: Map<string, TrackGroup> }> }>();
	for (const entry of entries) {
		const simulator = simulators.get(entry.simulatorId) ?? { name: entry.simulatorName, cars: new Map() };
		simulators.set(entry.simulatorId, simulator);
		const car = simulator.cars.get(entry.sourceCarId) ?? { name: entry.carName, rawName: entry.sourceCarId, tracks: new Map() };
		simulator.cars.set(entry.sourceCarId, car);
		const trackKey = `${entry.sourceTrackId}\u0000${entry.layoutId ?? ""}`;
		const track = car.tracks.get(trackKey) ?? {
			key: trackKey,
			name: entry.trackName,
			rawName: entry.sourceTrackId,
			layoutId: entry.layoutId,
			setups: [],
		};
		car.tracks.set(trackKey, track);
		track.setups.push(entry);
	}
	return [...simulators.entries()]
		.map(([key, simulator]) => ({
			key,
			name: simulator.name,
			cars: [...simulator.cars.entries()]
				.map(([carKey, car]) => ({ key: carKey, name: car.name, rawName: car.rawName, tracks: [...car.tracks.values()] }))
				.sort((left, right) => left.name.localeCompare(right.name)),
		}))
		.sort((left, right) => left.name.localeCompare(right.name));
}
