import { useEffect, useMemo, useState } from "react";
import { telemetryDataSource, type SetupLibraryEntry } from "../../data-source";
import { formatCompactSessionDate } from "../sessions/session-components";
import { SetupEditor } from "./SetupEditor";

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
	const [simulatorId, setSimulatorId] = useState("");
	const [selectedCarKey, setSelectedCarKey] = useState("");
	const [selectedTrackKey, setSelectedTrackKey] = useState("");
	const [editingSetup, setEditingSetup] = useState<SetupLibraryEntry | null>(null);
	const [libraryRevision, setLibraryRevision] = useState(0);

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
	}, [refreshKey, libraryRevision]);

	const simulators = useMemo(
		() => [...new Map(entries.map((entry) => [entry.simulatorId, entry.simulatorName])).entries()].sort((left, right) => left[1].localeCompare(right[1])),
		[entries],
	);
	useEffect(() => {
		if (simulators.length > 0 && !simulators.some(([id]) => id === simulatorId)) setSimulatorId(simulators[0][0]);
	}, [simulatorId, simulators]);
	const filtered = useMemo(() => {
		const needle = query.trim().toLocaleLowerCase();
		return entries.filter(
			(entry) =>
				(!simulatorId || entry.simulatorId === simulatorId) &&
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
	const activeSimulator = groups.find((group) => group.key === simulatorId) ?? groups[0] ?? null;
	const selectedCar = activeSimulator?.cars.find((car) => car.key === selectedCarKey) ?? activeSimulator?.cars[0] ?? null;
	const selectedTrack = selectedCar?.tracks.find((track) => track.key === selectedTrackKey) ?? selectedCar?.tracks[0] ?? null;

	useEffect(() => {
		if (selectedCar && selectedCar.key !== selectedCarKey) setSelectedCarKey(selectedCar.key);
	}, [selectedCar, selectedCarKey]);

	useEffect(() => {
		if (selectedTrack && selectedTrack.key !== selectedTrackKey) setSelectedTrackKey(selectedTrack.key);
	}, [selectedTrack, selectedTrackKey]);

	if (editingSetup) {
		return (
			<section className="mt-4 border border-trace-divider bg-trace-surface" aria-label={`Edit ${editingSetup.name}`}>
				<SetupEditor
					setup={editingSetup}
					onClose={() => setEditingSetup(null)}
					onSaved={() => {
						setLibraryRevision((value) => value + 1);
						setEditingSetup(null);
					}}
				/>
			</section>
		);
	}

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
					onChange={(event) => {
						setSimulatorId(event.target.value);
						setSelectedCarKey("");
						setSelectedTrackKey("");
					}}
					disabled={simulators.length < 2}
					className="trace-select h-10 border border-trace-divider bg-trace-black px-3 text-[12px] font-bold text-trace-text outline-none"
					aria-label="Filter setup library by simulator"
				>
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
				<div className="grid h-[clamp(430px,58vh,620px)] grid-cols-[minmax(180px,0.8fr)_minmax(210px,0.95fr)_minmax(300px,1.65fr)] overflow-hidden bg-trace-black">
					<section className="flex min-h-0 min-w-0 flex-col border-r border-trace-divider" aria-label="Cars">
						<ExplorerHeading label="Cars" count={activeSimulator?.cars.length ?? 0} />
						<div className="min-h-0 flex-1 overflow-y-auto py-1">
							{activeSimulator?.cars.map((car) => {
								const isSelected = car.key === selectedCar?.key;
								return (
									<button
										type="button"
										key={car.key}
										onClick={() => {
											setSelectedCarKey(car.key);
											setSelectedTrackKey(car.tracks[0]?.key ?? "");
										}}
										aria-pressed={isSelected}
										className={`block w-full min-w-0 border-l-2 px-4 py-3 text-left transition-colors ${
											isSelected ? "border-trace-accent bg-trace-accent/15" : "border-transparent bg-trace-black hover:bg-trace-deep"
										}`}
									>
										<span className={`block text-[12px] font-bold leading-5 ${isSelected ? "text-white" : "text-trace-soft"}`}>
											{car.name}
										</span>
										<span className="block truncate font-mono text-[9px] leading-4 text-trace-dim">{car.rawName}</span>
										<span className="mt-1 block font-mono text-[8px] font-bold text-trace-muted">{car.tracks.length} TRACKS</span>
									</button>
								);
							})}
						</div>
					</section>

					<section className="flex min-h-0 min-w-0 flex-col border-r border-trace-divider" aria-label="Tracks">
						<ExplorerHeading label="Tracks" count={selectedCar?.tracks.length ?? 0} />
						<div className="min-h-0 flex-1 overflow-y-auto py-1">
							{selectedCar?.tracks.map((track) => {
								const isSelected = track.key === selectedTrack?.key;
								return (
									<button
										type="button"
										key={track.key}
										onClick={() => setSelectedTrackKey(track.key)}
										aria-pressed={isSelected}
										className={`block w-full min-w-0 border-l-2 px-4 py-3 text-left transition-colors ${
											isSelected ? "border-trace-accent bg-trace-accent/15" : "border-transparent bg-trace-black hover:bg-trace-deep"
										}`}
									>
										<span className={`block text-[12px] font-bold leading-5 ${isSelected ? "text-white" : "text-trace-soft"}`}>
											{track.name}
										</span>
										<span className="block truncate font-mono text-[9px] leading-4 text-trace-dim">
											{track.rawName}
											{track.layoutId ? ` · ${track.layoutId}` : ""}
										</span>
										<span className="mt-1 block font-mono text-[8px] font-bold text-trace-muted">{track.setups.length} SETUPS</span>
									</button>
								);
							})}
						</div>
					</section>

					<section className="flex min-h-0 min-w-0 flex-col" aria-label="Setup files">
						<ExplorerHeading label="Setup files" count={selectedTrack?.setups.length ?? 0} />
						{selectedTrack && (
							<div className="bg-trace-deep px-4 py-3">
								<strong className="block text-[12px] text-white">{selectedTrack.name}</strong>
								<span className="mt-0.5 block truncate font-mono text-[9px] text-trace-dim">
									{selectedCar?.name} · {selectedTrack.rawName}
									{selectedTrack.layoutId ? ` · ${selectedTrack.layoutId}` : ""}
								</span>
							</div>
						)}
						<div className="min-h-0 flex-1 overflow-y-auto py-1">
							{selectedTrack?.setups.map((setup) => (
								<SetupFileRow setup={setup} onOpen={() => setEditingSetup(setup)} key={setup.id} />
							))}
						</div>
					</section>
				</div>
			)}
		</section>
	);
}

function ExplorerHeading({ label, count }: { label: string; count: number }) {
	return (
		<header className="flex h-10 shrink-0 items-center justify-between bg-trace-surface px-4">
			<h3 className="font-mono text-[9px] font-black uppercase tracking-[.12em] text-trace-soft">{label}</h3>
			<span className="font-mono text-[8px] font-bold text-trace-muted">{count}</span>
		</header>
	);
}

function SetupFileRow({ setup, onOpen }: { setup: SetupLibraryEntry; onOpen: () => void }) {
	return (
		<button
			type="button"
			onClick={onOpen}
			disabled={!setup.available}
			className="grid w-full min-w-0 grid-cols-[30px_minmax(0,1fr)] gap-3 border-l-2 border-transparent px-4 py-3 text-left hover:border-trace-accent hover:bg-trace-accent/10 disabled:cursor-not-allowed disabled:opacity-50 xl:grid-cols-[30px_minmax(0,1fr)_auto] xl:items-center"
		>
			<span className="grid size-[30px] place-items-center bg-trace-deep font-mono text-[8px] font-black text-trace-muted" aria-hidden="true">
				INI
			</span>
			<div className="min-w-0">
				<strong className="block break-words text-[12px] text-trace-text">{setup.name}</strong>
				<p className="mt-1 break-words text-[10px] leading-4 text-trace-dim">
					{setup.sourceArchive ?? "Local setup"} · indexed {formatCompactSessionDate(setup.importedAt)}
				</p>
			</div>
			<div className="col-start-2 flex flex-wrap items-center gap-1.5 xl:col-start-auto xl:justify-end">
				{setup.linkedSessionCount > 0 && (
					<span className="border border-trace-divider px-1.5 py-1 font-mono text-[8px] font-bold text-trace-soft">
						USED BY {setup.linkedSessionCount} SESSION{setup.linkedSessionCount === 1 ? "" : "S"}
					</span>
				)}
				{!setup.available && (
					<span className="border border-trace-warning/50 px-1.5 py-1 font-mono text-[8px] font-bold text-trace-warning">FILE MISSING</span>
				)}
			</div>
		</button>
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
