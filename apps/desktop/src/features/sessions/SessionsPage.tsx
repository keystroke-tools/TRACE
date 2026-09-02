import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { telemetryDataSource, type RecordedSessionSummary, type SessionExportFormat } from "../../data-source";
import { PageIntro, SectionHeading } from "../../components/layout";
import { Tooltip } from "../../Tooltip";
import { useToast } from "../../Toast";
import {
	DeleteConfirmation,
	EmptySessions,
	ExportOption,
	FuelUsage,
	LapMetricValue,
	OwnershipBadge,
	SectorBars,
	SectorLegend,
	SessionDetailsEditor,
	formatCompactSessionDate,
	formatFuelUsed,
	formatLapDurationNs,
	formatSessionDate,
	friendlyConditionName,
	friendlySessionType,
	lapDuration,
	lapInvalidityDetail,
	lapIsInvalid,
	sessionSourceGroup,
	sessionSourceLabel,
	setupDifferenceLabel,
	theoreticalBestLap,
} from "./session-components";

function extension(path: string) {
	const fileName = path.replaceAll("\\", "/").split("/").pop() ?? "";
	const separator = fileName.lastIndexOf(".");
	return separator >= 0 ? fileName.slice(separator + 1).toLowerCase() : "";
}

function pairKey(path: string) {
	const normalized = path.replaceAll("\\", "/");
	const separator = normalized.lastIndexOf(".");
	return (separator >= 0 ? normalized.slice(0, separator) : normalized).toLowerCase();
}

export function SessionsPage({
	sessions,
	onOpen,
	onDeleted,
	onUpdated,
	onImported,
}: {
	sessions: RecordedSessionSummary[];
	onOpen: (sessionId: string) => void;
	onDeleted: (sessionId: string) => void;
	onUpdated: (session: RecordedSessionSummary) => void;
	onImported: () => Promise<void>;
}) {
	const showToast = useToast();
	const [query, setQuery] = useState("");
	const [sourceFilter, setSourceFilter] = useState("all");
	const [simulatorFilter, setSimulatorFilter] = useState("all");
	const [sortOrder, setSortOrder] = useState("newest");
	const [importing, setImporting] = useState(false);
	const [draggingTelemetry, setDraggingTelemetry] = useState(false);
	const visibleSessions = useMemo(() => {
		const normalizedQuery = query.trim().toLocaleLowerCase();
		return sessions
			.filter((session) => {
				const source = sessionSourceGroup(session);
				const matchesSource = sourceFilter === "all" || source === sourceFilter;
				const matchesSimulator = simulatorFilter === "all" || session.simulatorId === simulatorFilter;
				const ownershipLabel = session.ownership === "other" ? "other driver" : session.ownership === "mine" ? "my drive" : "not specified";
				const searchable = [
					session.title,
					session.driver,
					ownershipLabel,
					session.simulatorName,
					session.track,
					session.car,
					session.sessionType,
					session.source,
					...session.tags,
				]
					.join(" ")
					.toLocaleLowerCase();
				return matchesSource && matchesSimulator && (!normalizedQuery || searchable.includes(normalizedQuery));
			})
			.slice()
			.sort((left, right) => {
				const difference = new Date(right.startedAt).getTime() - new Date(left.startedAt).getTime();
				return sortOrder === "newest" ? difference : -difference;
			});
	}, [query, sessions, simulatorFilter, sortOrder, sourceFilter]);
	const simulators = useMemo(() => Array.from(new Map(sessions.map((session) => [session.simulatorId, session.simulatorName])).entries()), [sessions]);

	async function deleteRecordedSession(session: RecordedSessionSummary) {
		try {
			const result = await telemetryDataSource.deleteSession(session.id);
			onDeleted(result.sessionId);
			showToast(
				result.cleanupWarning
					? { kind: "error", title: "Deleted with cleanup warning", message: result.cleanupWarning, timeoutMs: 9_000 }
					: { kind: "success", title: "Session deleted", message: `${session.track} was removed from your session library.`, timeoutMs: 4_500 },
			);
			return true;
		} catch (error) {
			showToast({ kind: "error", title: "Could not delete session", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
			return false;
		}
	}

	async function updateRecordedSession(
		session: RecordedSessionSummary,
		title: string | null,
		driver: string | null,
		ownership: RecordedSessionSummary["ownership"],
		tags: string[],
	) {
		try {
			await telemetryDataSource.updateSessionDetails(session.id, title, driver, ownership, tags);
			onUpdated({ ...session, title, driver, ownership, tags });
			showToast({ kind: "success", title: "Session updated", message: "Name, driver attribution, ownership, and tags were saved.", timeoutMs: 3_500 });
			return true;
		} catch (error) {
			showToast({ kind: "error", title: "Could not update session", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
			return false;
		}
	}

	async function importTelemetryPath(path: string) {
		setImporting(true);
		try {
			const result = await telemetryDataSource.importSession(path);
			await onImported();
			showToast({
				kind: "success",
				title: "Telemetry imported",
				message: `${result.lapCount} laps and ${result.sampleCount.toLocaleString()} telemetry samples are ready${result.setupName ? ` · ${result.setupName} was restored to the setup library` : ""}.`,
				timeoutMs: 6_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Import failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
		} finally {
			setImporting(false);
		}
	}

	async function chooseTelemetry() {
		const selected = await open({
			multiple: false,
			directory: false,
			title: "Import telemetry",
			filters: [{ name: "TRACE or MoTeC telemetry", extensions: ["trace", "ld"] }],
		});
		if (typeof selected !== "string") return;
		await importTelemetryPath(selected);
	}

	async function importDroppedTelemetry(paths: string[]) {
		const tracePackages = paths.filter((path) => extension(path) === "trace");
		const logs = paths.filter((path) => extension(path) === "ld");
		const sidecars = paths.filter((path) => extension(path) === "ldx");
		if (tracePackages.length === 1 && logs.length === 0 && sidecars.length === 0) {
			await importTelemetryPath(tracePackages[0]);
			return;
		}
		if (logs.length !== 1 || tracePackages.length > 0 || sidecars.length > 1) {
			showToast({
				kind: "error",
				title: "Choose one telemetry outing",
				message: "Drop one .trace package, one .ld log, or one matching .ld and .ldx pair.",
				timeoutMs: 6_000,
			});
			return;
		}
		if (sidecars.length === 1 && pairKey(logs[0]) !== pairKey(sidecars[0])) {
			showToast({
				kind: "error",
				title: "MoTeC files do not match",
				message: "The .ld and .ldx must have the same name and be in the same folder.",
				timeoutMs: 6_000,
			});
			return;
		}
		await importTelemetryPath(logs[0]);
	}

	useEffect(() => {
		if (!isTauri()) return undefined;
		let disposed = false;
		let unlisten: (() => void) | undefined;
		void getCurrentWebview()
			.onDragDropEvent((event) => {
				if (event.payload.type === "enter") {
					setDraggingTelemetry(event.payload.paths.some((path) => ["trace", "ld", "ldx"].includes(extension(path))));
				}
				if (event.payload.type === "leave") setDraggingTelemetry(false);
				if (event.payload.type === "drop") {
					setDraggingTelemetry(false);
					if (!importing) void importDroppedTelemetry(event.payload.paths);
				}
			})
			.then((stop) => {
				if (disposed) stop();
				else unlisten = stop;
			});
		return () => {
			disposed = true;
			unlisten?.();
		};
	}, [importing]);

	return (
		<>
			{draggingTelemetry && (
				<div className="pointer-events-none fixed inset-3 z-[90] grid place-items-center border-2 border-dashed border-trace-accent bg-trace-deep/95">
					<div className="text-center">
						<span className="font-mono text-[12px] font-black tracking-[.14em] text-trace-accent">DROP TELEMETRY TO IMPORT</span>
						<p className="mt-2 text-[13px] text-trace-soft">.trace, .ld, or a matching .ld + .ldx pair</p>
					</div>
				</div>
			)}
			<div className="flex items-end justify-between gap-6">
				<div>
					<SectionHeading index="02">SESSION LIBRARY</SectionHeading>
					<h1 className="mt-3 text-2xl font-black tracking-[-.02em]">SESSIONS</h1>
					<p className="mt-2 text-[13px] text-trace-muted">Browse drives and replays, then open one to review every lap and its telemetry.</p>
				</div>
				<div className="flex items-center gap-4">
					<span className="font-mono text-[12px] text-trace-faint">
						{visibleSessions.length} shown · {sessions.length} total
					</span>
					<button
						type="button"
						disabled={importing}
						onClick={() => void chooseTelemetry()}
						className="h-10 border border-trace-accent/45 bg-trace-accent-wash px-4 font-mono text-[11px] font-bold tracking-[.08em] text-trace-accent hover:border-trace-accent hover:text-white disabled:border-trace-divider disabled:text-trace-dim"
					>
						{importing ? "IMPORTING…" : "IMPORT TELEMETRY"}
					</button>
				</div>
			</div>

			<div className="mt-6 flex flex-wrap border border-trace-divider bg-trace-surface">
				<label className="flex min-w-[280px] flex-1 items-center gap-3 border-r border-trace-divider px-4 focus-within:bg-trace-raised">
					<svg className="size-3.5 shrink-0 stroke-trace-dim" viewBox="0 0 16 16" fill="none" aria-hidden="true">
						<circle cx="7" cy="7" r="4.5" />
						<path d="m10.5 10.5 3 3" />
					</svg>
					<span className="sr-only">Search sessions</span>
					<input
						type="search"
						value={query}
						onChange={(event) => setQuery(event.target.value)}
						placeholder="Search track, car, or session…"
						className="h-12 min-w-0 flex-1 border-0 bg-transparent text-[12px] text-trace-text outline-none placeholder:text-trace-dim"
					/>
				</label>
				<select
					aria-label="Filter sessions by source"
					value={sourceFilter}
					onChange={(event) => setSourceFilter(event.target.value)}
					className="trace-select h-12 border-0 border-r border-trace-divider bg-trace-surface pl-4 font-mono text-[12px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
				>
					<option value="all">All sources</option>
					<option value="replay">Replays</option>
					<option value="native">Drives</option>
					<option value="imported">Imports</option>
				</select>
				{simulators.length > 1 && (
					<select
						aria-label="Filter sessions by simulator"
						value={simulatorFilter}
						onChange={(event) => setSimulatorFilter(event.target.value)}
						className="trace-select h-12 border-0 border-r border-trace-divider bg-trace-surface pl-4 font-mono text-[12px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
					>
						<option value="all">All simulators</option>
						{simulators.map(([id, name]) => (
							<option value={id} key={id}>
								{name}
							</option>
						))}
					</select>
				)}
				<select
					aria-label="Sort sessions"
					value={sortOrder}
					onChange={(event) => setSortOrder(event.target.value)}
					className="trace-select h-12 border-0 bg-trace-surface pl-4 font-mono text-[12px] font-bold tracking-[.08em] text-trace-soft outline-none focus:bg-trace-raised"
				>
					<option value="newest">Newest first</option>
					<option value="oldest">Oldest first</option>
				</select>
			</div>

			<div className="mt-3 border border-trace-divider bg-trace-surface">
				{sessions.length === 0 ? (
					<EmptySessions title="No sessions yet">
						Select an installed simulator, then start a drive or play a replay. TRACE will save it here automatically.
					</EmptySessions>
				) : visibleSessions.length === 0 ? (
					<EmptySessions title="Nothing matches">Try a different search or change the source filter.</EmptySessions>
				) : (
					visibleSessions.map((session) => (
						<SessionRow
							key={session.id}
							session={session}
							onOpen={() => onOpen(session.id)}
							onDelete={deleteRecordedSession}
							onUpdate={updateRecordedSession}
						/>
					))
				)}
			</div>
		</>
	);
}

function SessionRow({
	session,
	onOpen,
	onDelete,
	onUpdate,
}: {
	session: RecordedSessionSummary;
	onOpen: () => void;
	onDelete: (session: RecordedSessionSummary) => Promise<boolean>;
	onUpdate: (
		session: RecordedSessionSummary,
		title: string | null,
		driver: string | null,
		ownership: RecordedSessionSummary["ownership"],
		tags: string[],
	) => Promise<boolean>;
}) {
	const showToast = useToast();
	const [actionsOpen, setActionsOpen] = useState(false);
	const [confirmingDelete, setConfirmingDelete] = useState(false);
	const [editingDetails, setEditingDetails] = useState(false);
	const [exportMenuOpen, setExportMenuOpen] = useState(false);
	const [draftTitle, setDraftTitle] = useState(session.title ?? "");
	const [draftDriver, setDraftDriver] = useState(session.driver ?? "");
	const [draftOwnership, setDraftOwnership] = useState<RecordedSessionSummary["ownership"]>(session.ownership);
	const [draftTags, setDraftTags] = useState(session.tags.join(", "));
	const [exporting, setExporting] = useState(false);
	const [deleting, setDeleting] = useState(false);
	const [savingDetails, setSavingDetails] = useState(false);
	const actionsMenu = useRef<HTMLDivElement>(null);
	const timedLaps = session.laps.filter((lap) => lap.time !== "—" && lap.validity !== "invalid");
	const bestLap = timedLaps.slice().sort((left, right) => lapDuration(left) - lapDuration(right))[0];

	useEffect(() => {
		if (!actionsOpen) return;
		function dismissOnPointerDown(event: PointerEvent) {
			if (!actionsMenu.current?.contains(event.target as Node)) {
				setActionsOpen(false);
				setConfirmingDelete(false);
				setEditingDetails(false);
				setExportMenuOpen(false);
			}
		}
		function dismissOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape") {
				setActionsOpen(false);
				setConfirmingDelete(false);
				setEditingDetails(false);
				setExportMenuOpen(false);
			}
		}
		document.addEventListener("pointerdown", dismissOnPointerDown);
		document.addEventListener("keydown", dismissOnEscape);
		return () => {
			document.removeEventListener("pointerdown", dismissOnPointerDown);
			document.removeEventListener("keydown", dismissOnEscape);
		};
	}, [actionsOpen]);

	async function exportTelemetry(exportFormat: SessionExportFormat) {
		setExporting(true);
		try {
			const result = await telemetryDataSource.exportSession(session.id, exportFormat);
			showToast({
				kind: "success",
				title: `${result.format} exported`,
				message: `${result.sampleCount.toLocaleString()} samples saved to ${result.path}`,
				timeoutMs: 7_000,
			});
			setExportMenuOpen(false);
			setActionsOpen(false);
		} catch (error) {
			showToast({ kind: "error", title: "Export failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 9_000 });
		} finally {
			setExporting(false);
		}
	}

	async function deleteRecording() {
		setDeleting(true);
		const deleted = await onDelete(session);
		setDeleting(false);
		if (deleted) setActionsOpen(false);
	}

	async function saveDetails() {
		setSavingDetails(true);
		const title = draftTitle.trim() || null;
		const seen = new Set<string>();
		const tags = draftTags
			.split(",")
			.map((tag) => tag.trim())
			.filter((tag) => {
				const key = tag.toLocaleLowerCase();
				if (!tag || seen.has(key)) return false;
				seen.add(key);
				return true;
			});
		const driver = draftDriver.trim() || null;
		const saved = await onUpdate(session, title, driver, draftOwnership, tags);
		setSavingDetails(false);
		if (saved) {
			setEditingDetails(false);
			setActionsOpen(false);
		}
	}

	return (
		<article className="relative border-b border-trace-divider last:border-b-0">
			<div className="flex items-stretch">
				<button
					type="button"
					aria-label={`View ${session.track} session`}
					onClick={onOpen}
					className="group grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_88px_124px_20px] items-center gap-6 border-0 bg-transparent px-5 py-5 text-left hover:bg-trace-raised"
				>
					<div className="min-w-0">
						<div className="flex min-w-0 items-center gap-3">
							<span className="shrink-0 font-mono text-[12px] font-extrabold tracking-[.1em] text-trace-accent">
								{friendlySessionType(session)}
							</span>
							<h2 className="min-w-0 truncate text-base font-black tracking-[.02em]">{session.title ?? session.track}</h2>
							{session.ownership !== "unknown" && <OwnershipBadge ownership={session.ownership} />}
						</div>
						<span className="mt-2 flex min-w-0 items-center gap-2 text-[12px] text-trace-dim">
							<span className="shrink-0 text-trace-soft">{session.car}</span>
							{session.title && (
								<>
									<span aria-hidden="true">·</span>
									<span className="min-w-0 truncate">{session.track}</span>
								</>
							)}
							<span aria-hidden="true">·</span>
							<Tooltip className="shrink-0" content={session.startedAt}>
								{formatSessionDate(session.startedAt)}
							</Tooltip>
							{session.driver && (
								<>
									<span aria-hidden="true">·</span>
									<span className="min-w-0 truncate text-trace-muted">{session.driver}</span>
								</>
							)}
						</span>
					</div>
					<div className="font-mono text-right">
						<strong className="block text-sm text-trace-soft">{session.laps.length}</strong>
						<span className="mt-1 block text-[12px] tracking-[.08em] text-trace-dim">LAPS</span>
					</div>
					<div className="font-mono text-right">
						<strong className="block text-sm text-trace-soft">{bestLap?.time ?? "—"}</strong>
						<span className="mt-1 block text-[12px] tracking-[.08em] text-trace-dim">BEST LAP</span>
					</div>
					<svg
						className="size-4 fill-none stroke-current text-trace-muted transition-transform group-hover:translate-x-0.5 group-hover:text-trace-text"
						viewBox="0 0 16 16"
						aria-hidden="true"
					>
						<path d="m6 4 4 4-4 4" />
					</svg>
				</button>
				<div className="flex shrink-0 items-stretch border-l border-trace-divider">
					<Tooltip className="h-full" content="Share as a .trace package">
						<button
							type="button"
							aria-label={`Share ${session.track} session as a TRACE package`}
							disabled={exporting || !session.exportable}
							onClick={() => void exportTelemetry("trace")}
							className="grid h-full w-12 place-items-center border-0 border-r border-trace-divider bg-transparent text-trace-accent hover:bg-trace-accent-wash hover:text-trace-text disabled:text-trace-dim disabled:hover:bg-transparent"
						>
							<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
								<path d="M8 10.5V2.5m0 0L5.2 5.3M8 2.5l2.8 2.8M3 9.5v3h10v-3" />
							</svg>
						</button>
					</Tooltip>
					<div className="relative flex" ref={actionsMenu}>
						<Tooltip className="h-full" content="Session actions">
							<button
								type="button"
								aria-label={`Actions for ${session.track} session`}
								aria-expanded={actionsOpen}
								onClick={() => {
									setActionsOpen((value) => !value);
									setConfirmingDelete(false);
									setEditingDetails(false);
									setExportMenuOpen(false);
								}}
								className="grid h-full w-12 place-items-center border-0 bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-text"
							>
								<svg className="size-4 fill-current" viewBox="0 0 16 16" aria-hidden="true">
									<circle cx="3" cy="8" r="1.2" />
									<circle cx="8" cy="8" r="1.2" />
									<circle cx="13" cy="8" r="1.2" />
								</svg>
							</button>
						</Tooltip>
						{actionsOpen && (
							<div className="absolute right-0 top-[calc(100%-10px)] z-20 max-h-[calc(100vh-80px)] w-72 overflow-y-auto border border-trace-divider bg-trace-black p-2 shadow-[0_12px_30px_#000]">
								{confirmingDelete ? (
									<DeleteConfirmation
										session={session}
										deleting={deleting}
										onCancel={() => setConfirmingDelete(false)}
										onConfirm={() => void deleteRecording()}
									/>
								) : editingDetails ? (
									<SessionDetailsEditor
										title={draftTitle}
										driver={draftDriver}
										ownership={draftOwnership}
										tags={draftTags}
										saving={savingDetails}
										onTitleChange={setDraftTitle}
										onDriverChange={setDraftDriver}
										onOwnershipChange={setDraftOwnership}
										onTagsChange={setDraftTags}
										onCancel={() => setEditingDetails(false)}
										onSave={() => void saveDetails()}
									/>
								) : (
									<>
										<span className="block px-2 pb-2 pt-1 text-[12px] font-bold text-trace-soft">Session actions</span>
										<button
											type="button"
											onClick={() => {
												setDraftTitle(session.title ?? "");
												setDraftDriver(session.driver ?? "");
												setDraftOwnership(session.ownership);
												setDraftTags(session.tags.join(", "));
												setExportMenuOpen(false);
												setEditingDetails(true);
											}}
											className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-text hover:bg-trace-raised"
										>
											Name, driver & tags…
										</button>
										<button
											type="button"
											aria-expanded={exportMenuOpen}
											disabled={exporting || !session.exportable}
											onClick={() => setExportMenuOpen((value) => !value)}
											className="flex w-full items-center justify-between border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-text hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent"
										>
											<span>{exporting ? "Exporting…" : "Data exports…"}</span>
											<svg
												className={`size-3 fill-none stroke-current transition-transform ${exportMenuOpen ? "rotate-90" : ""}`}
												viewBox="0 0 12 12"
												aria-hidden="true"
											>
												<path d="m4.5 2.5 3 3.5-3 3.5" />
											</svg>
										</button>
										{exportMenuOpen && session.exportable && (
											<div className="ml-2 border-l border-trace-divider bg-trace-deep pb-1 pl-1 pt-2">
												<span className="block px-2 pb-1 font-mono text-[9px] font-bold tracking-[.12em] text-trace-dim">
													DATA EXPORTS
												</span>
												<ExportOption
													label="Raw telemetry"
													detail="Arrow IPC · all captured channels"
													disabled={exporting}
													onClick={() => void exportTelemetry("arrow")}
												/>
												<ExportOption
													label="Spreadsheet"
													detail="CSV · core channels"
													disabled={exporting}
													onClick={() => void exportTelemetry("csv")}
												/>
											</div>
										)}
										{!session.exportable && (
											<p className="px-2 py-2 text-[12px] leading-4 text-trace-dim">This session has no finalized telemetry to export.</p>
										)}
										<div className="my-1 border-t border-trace-divider" />
										<button
											type="button"
											disabled={!session.deletable}
											onClick={() => {
												setExportMenuOpen(false);
												setConfirmingDelete(true);
											}}
											className="block w-full border-0 bg-transparent px-2 py-2.5 text-left text-[12px] font-bold text-trace-warning hover:bg-trace-raised disabled:text-trace-dim disabled:hover:bg-transparent"
										>
											{session.deletable ? "Delete session…" : "Session in progress"}
										</button>
									</>
								)}
							</div>
						)}
					</div>
				</div>
			</div>
		</article>
	);
}
