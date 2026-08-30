import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { telemetryDataSource, type SetupImporterDescriptor, type SetupImportResult } from "../../data-source";
import { PageIntro } from "../../components/layout";
import { useToast } from "../../Toast";
import { saveSetupFolder, savedSetupFolder } from "./setup-preferences";

export function SetupsPage() {
	const showToast = useToast();
	const [importers, setImporters] = useState<SetupImporterDescriptor[]>([]);
	const [simulatorId, setSimulatorId] = useState("");
	const [setupsFolder, setSetupsFolder] = useState("");
	const [folderFound, setFolderFound] = useState(false);
	const [detecting, setDetecting] = useState(true);
	const [overwrite, setOverwrite] = useState(false);
	const [importMode, setImportMode] = useState<"archives" | "files">("archives");
	const [sourceCarId, setSourceCarId] = useState("");
	const [sourceTrackId, setSourceTrackId] = useState("");
	const [layoutId, setLayoutId] = useState("");
	const [importing, setImporting] = useState(false);
	const [indexing, setIndexing] = useState(false);
	const [dragging, setDragging] = useState(false);
	const [results, setResults] = useState<SetupImportResult[]>([]);
	const importer = importers.find((value) => value.simulatorId === simulatorId) ?? null;

	useEffect(() => {
		let active = true;
		void telemetryDataSource
			.getSetupImporters()
			.then((values) => {
				if (!active) return;
				setImporters(values);
				setSimulatorId((current) => current || values[0]?.simulatorId || "");
				if (values.length === 0) setDetecting(false);
			})
			.catch((error) => {
				if (!active) return;
				setDetecting(false);
				showToast({
					kind: "error",
					title: "Setup importers unavailable",
					message: error instanceof Error ? error.message : String(error),
					timeoutMs: 8_000,
				});
			});
		return () => {
			active = false;
		};
	}, [showToast]);

	useEffect(() => {
		if (!simulatorId) return undefined;
		const savedFolder = savedSetupFolder(simulatorId);
		if (savedFolder) {
			setSetupsFolder(savedFolder);
			setFolderFound(true);
			setDetecting(false);
			setResults([]);
			return undefined;
		}
		let active = true;
		setDetecting(true);
		setSetupsFolder("");
		setFolderFound(false);
		setResults([]);
		void telemetryDataSource
			.detectSetupFolder(simulatorId)
			.then((folder) => {
				if (!active) return;
				setSetupsFolder(folder.path ?? "");
				setFolderFound(folder.found);
				setDetecting(false);
			})
			.catch((error) => {
				if (!active) return;
				setDetecting(false);
				showToast({
					kind: "error",
					title: "Setup folder unavailable",
					message: error instanceof Error ? error.message : String(error),
					timeoutMs: 8_000,
				});
			});
		return () => {
			active = false;
		};
	}, [showToast, simulatorId]);

	async function importArchives(paths: string[]) {
		if (!importer) return;
		const archives = paths.filter((path) => importer.archiveExtensions.some((extension) => path.toLowerCase().endsWith(`.${extension.toLowerCase()}`)));
		if (archives.length === 0) {
			showToast({
				kind: "error",
				title: "No setup archives found",
				message: `Choose or drop ${importer.archiveExtensions.map((value) => `.${value}`).join(" or ")} files.`,
				timeoutMs: 5_000,
			});
			return;
		}
		if (!setupsFolder.trim()) {
			showToast({
				kind: "error",
				title: "Choose the setups folder",
				message: `TRACE needs the ${importer.simulatorName} setups directory before it can install files.`,
				timeoutMs: 6_000,
			});
			return;
		}
		setImporting(true);
		try {
			const nextResults = await telemetryDataSource.importSetupArchives(importer.simulatorId, archives, setupsFolder.trim(), overwrite);
			setResults(nextResults);
			const installed = nextResults.reduce((total, result) => total + result.files.length, 0);
			const skipped = nextResults.reduce((total, result) => total + result.skipped.length, 0);
			const failures = nextResults.filter((result) => !result.success).length;
			const warnings = nextResults.filter((result) => result.indexWarning).length;
			showToast({
				kind: failures > 0 || warnings > 0 ? "error" : "success",
				title: failures > 0 ? "Some setups could not be imported" : warnings > 0 ? "Setups installed but not indexed" : "Setups imported",
				message: `${installed} file${installed === 1 ? "" : "s"} installed${skipped > 0 ? ` · ${skipped} kept because they already exist` : ""}${warnings > 0 ? " · Review the setup-library warning." : ""}.`,
				timeoutMs: failures > 0 || warnings > 0 ? 8_000 : 5_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Setup import failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setImporting(false);
		}
	}

	async function importFiles(paths: string[]) {
		if (!importer) return;
		const files = paths.filter((path) => importer.fileExtensions.some((extension) => path.toLowerCase().endsWith(`.${extension.toLowerCase()}`)));
		if (files.length === 0) {
			showToast({
				kind: "error",
				title: "No setup files found",
				message: `Choose ${importer.fileExtensions.map((value) => `.${value}`).join(" or ")} files.`,
				timeoutMs: 5_000,
			});
			return;
		}
		if (!setupsFolder.trim() || !sourceTrackId.trim()) {
			showToast({
				kind: "error",
				title: "Setup destination incomplete",
				message:
					"Choose the setups folder and enter the simulator's source track identifier. Enter a car identifier only when the file does not declare one.",
				timeoutMs: 7_000,
			});
			return;
		}
		setImporting(true);
		try {
			const nextResults = await telemetryDataSource.importSetupFiles({
				simulatorId: importer.simulatorId,
				setupPaths: files,
				setupsFolder: setupsFolder.trim(),
				sourceCarId: sourceCarId.trim() || null,
				sourceTrackId: sourceTrackId.trim(),
				layoutId: layoutId.trim() || null,
				overwrite,
			});
			setResults(nextResults);
			const failures = nextResults.filter((result) => !result.success || result.indexWarning).length;
			showToast({
				kind: failures > 0 ? "error" : "success",
				title: failures > 0 ? "Some setup files could not be imported" : "Setup files imported",
				message: `${nextResults.reduce((total, result) => total + result.files.length, 0)} installed · ${nextResults.reduce((total, result) => total + result.skipped.length, 0)} already existed.`,
				timeoutMs: failures > 0 ? 8_000 : 5_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Setup import failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setImporting(false);
		}
	}

	useEffect(() => {
		if (!("__TAURI_INTERNALS__" in window)) return undefined;
		let disposed = false;
		let unlisten: (() => void) | undefined;
		void getCurrentWebview()
			.onDragDropEvent((event) => {
				if (event.payload.type === "enter")
					setDragging(
						event.payload.paths.some(
							(path) =>
								(importMode === "archives" ? importer?.archiveExtensions : importer?.fileExtensions)?.some((extension) =>
									path.toLowerCase().endsWith(`.${extension.toLowerCase()}`),
								) ?? false,
						),
					);
				if (event.payload.type === "leave") setDragging(false);
				if (event.payload.type === "drop") {
					setDragging(false);
					if (!importing) void (importMode === "archives" ? importArchives(event.payload.paths) : importFiles(event.payload.paths));
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
	}, [importMode, importer, importing, overwrite, setupsFolder, sourceCarId, sourceTrackId]);

	async function chooseSetupsFolder() {
		if (!importer) return;
		const selected = await open({ directory: true, multiple: false, title: `Choose ${importer.folderLabel}` });
		if (typeof selected !== "string") return;
		setSetupsFolder(selected);
		saveSetupFolder(importer.simulatorId, selected);
		setFolderFound(true);
	}

	async function chooseArchives() {
		if (!importer) return;
		const selected = await open({
			multiple: true,
			directory: false,
			title: `Choose ${importer.archiveLabel}`,
			filters: [{ name: importer.archiveLabel, extensions: importer.archiveExtensions }],
		});
		if (selected == null) return;
		await importArchives(Array.isArray(selected) ? selected : [selected]);
	}

	async function chooseFiles() {
		if (!importer) return;
		const selected = await open({
			multiple: true,
			directory: false,
			title: `Choose ${importer.fileLabel}`,
			filters: [{ name: importer.fileLabel, extensions: importer.fileExtensions }],
		});
		if (selected == null) return;
		await importFiles(Array.isArray(selected) ? selected : [selected]);
	}

	async function indexExisting() {
		if (!importer || !setupsFolder.trim()) return;
		setIndexing(true);
		try {
			const result = await telemetryDataSource.indexExistingSetups(importer.simulatorId, setupsFolder.trim());
			showToast({
				kind: result.errors.length > 0 ? "error" : "success",
				title: "Existing setup scan complete",
				message: `${result.indexed} setups indexed${result.ignored > 0 ? ` · ${result.ignored} unrelated files ignored` : ""}${result.limited ? " · scan limit reached" : ""}.`,
				timeoutMs: result.errors.length > 0 ? 8_000 : 5_000,
			});
		} catch (error) {
			showToast({ kind: "error", title: "Setup scan failed", message: error instanceof Error ? error.message : String(error), timeoutMs: 8_000 });
		} finally {
			setIndexing(false);
		}
	}

	return (
		<>
			<PageIntro
				index="05"
				eyebrow="CAR SETUPS"
				title="INSTALL SHARED SETUPS"
				description="Choose a simulator-aware importer and let TRACE put shared setup files in the layout that simulator expects."
			/>
			<div className="mt-7 border border-trace-divider bg-trace-surface">
				<div className="flex items-center justify-between gap-5 border-b border-trace-divider px-5 py-4">
					<div>
						<strong className="block text-[13px] tracking-[.04em] text-white">SETUP IMPORTER</strong>
						<span className="mt-1 block text-[11px] leading-4 text-trace-dim">
							Each simulator owns its detection and archive rules. More importers can be added without changing this workspace.
						</span>
					</div>
					<select
						value={simulatorId}
						onChange={(event) => setSimulatorId(event.target.value)}
						disabled={importers.length < 2}
						className="trace-select h-10 min-w-52 border border-trace-divider bg-trace-deep px-3 text-[12px] font-bold text-trace-text outline-none disabled:text-trace-muted"
						aria-label="Setup importer simulator"
					>
						{importers.map((value) => (
							<option value={value.simulatorId} key={value.simulatorId}>
								{value.simulatorName}
							</option>
						))}
						{importers.length === 0 && <option value="">No setup importers available</option>}
					</select>
				</div>
				<div className="grid gap-px bg-trace-divider lg:grid-cols-[minmax(0,1fr)_220px]">
					<div className="bg-trace-surface p-5">
						<label className="font-mono text-[11px] font-black tracking-[.12em] text-trace-soft" htmlFor="setups-folder">
							{importer?.folderLabel.toUpperCase() ?? "SETUPS FOLDER"}
						</label>
						<p className="mt-1 text-[12px] leading-5 text-trace-dim">{importer?.folderHint ?? "Select a setup importer to continue."}</p>
						<div className="mt-4 flex min-w-0">
							<input
								id="setups-folder"
								value={setupsFolder}
								onChange={(event) => {
									setSetupsFolder(event.target.value);
									setFolderFound(false);
								}}
								onBlur={() => {
									if (importer) saveSetupFolder(importer.simulatorId, setupsFolder);
								}}
								placeholder={detecting ? "Detecting…" : "Choose the setups folder"}
								spellCheck={false}
								className="h-11 min-w-0 flex-1 border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] text-trace-text outline-none focus:border-trace-accent"
							/>
							<button
								type="button"
								onClick={() => void chooseSetupsFolder()}
								className="h-11 shrink-0 border border-l-0 border-trace-divider bg-trace-raised px-4 text-[11px] font-bold text-trace-soft hover:border-trace-accent hover:text-white"
							>
								CHOOSE FOLDER
							</button>
							<button
								type="button"
								disabled={indexing || !setupsFolder.trim()}
								onClick={() => void indexExisting()}
								className="h-11 shrink-0 border border-l-0 border-trace-divider bg-trace-raised px-4 text-[11px] font-bold text-trace-soft hover:border-trace-accent hover:text-white disabled:text-trace-dim"
							>
								{indexing ? "INDEXING…" : "INDEX EXISTING"}
							</button>
						</div>
						<p className={`mt-2 font-mono text-[10px] leading-4 ${folderFound ? "text-trace-accent" : "text-trace-dim"}`}>
							{folderFound ? "FOLDER READY" : "VERIFY THIS LOCATION BEFORE IMPORTING"}
						</p>
					</div>
					<label className="flex cursor-pointer items-center gap-3 bg-trace-deep px-5 py-4 hover:bg-trace-raised">
						<input
							type="checkbox"
							checked={overwrite}
							onChange={(event) => setOverwrite(event.target.checked)}
							className="size-4 accent-[var(--color-trace-accent)]"
						/>
						<span>
							<strong className="block text-[12px] text-trace-text">Replace existing files</strong>
							<span className="mt-1 block text-[11px] leading-4 text-trace-dim">
								Off by default. Matching setups are kept and reported as skipped.
							</span>
						</span>
					</label>
				</div>
				<div className="grid grid-cols-2 border-t border-trace-divider" role="tablist" aria-label="Setup import type">
					{(
						[
							{ id: "archives", label: "ARCHIVES" },
							{ id: "files", label: "INDIVIDUAL FILES" },
						] as const
					).map((mode) => (
						<button
							type="button"
							role="tab"
							aria-selected={importMode === mode.id}
							onClick={() => {
								setImportMode(mode.id);
								setResults([]);
							}}
							className={`h-11 border-b-2 font-mono text-[11px] font-bold tracking-[.08em] ${importMode === mode.id ? "border-trace-accent bg-trace-deep text-white" : "border-transparent bg-trace-surface text-trace-dim hover:bg-trace-deep hover:text-white"}`}
							key={mode.id}
						>
							{mode.label}
						</button>
					))}
				</div>
				{importMode === "files" && (
					<div className="grid gap-4 border-b border-trace-divider bg-trace-surface p-5 md:grid-cols-3">
						<label className="min-w-0">
							<span className="font-mono text-[10px] font-black tracking-[.1em] text-trace-soft">SOURCE TRACK ID</span>
							<input
								value={sourceTrackId}
								onChange={(event) => setSourceTrackId(event.target.value)}
								placeholder="e.g. ks_zandvoort"
								spellCheck={false}
								className="mt-2 h-10 w-full border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] text-trace-text outline-none focus:border-trace-accent"
							/>
						</label>
						<label className="min-w-0">
							<span className="font-mono text-[10px] font-black tracking-[.1em] text-trace-soft">SOURCE CAR ID · OPTIONAL</span>
							<input
								value={sourceCarId}
								onChange={(event) => setSourceCarId(event.target.value)}
								placeholder="Read from CAR / MODEL when available"
								spellCheck={false}
								className="mt-2 h-10 w-full border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] text-trace-text outline-none focus:border-trace-accent"
							/>
						</label>
						<label className="min-w-0">
							<span className="font-mono text-[10px] font-black tracking-[.1em] text-trace-soft">LAYOUT ID · OPTIONAL</span>
							<input
								value={layoutId}
								onChange={(event) => setLayoutId(event.target.value)}
								placeholder="Exact layout when applicable"
								spellCheck={false}
								className="mt-2 h-10 w-full border border-trace-divider bg-trace-deep px-3 font-mono text-[12px] text-trace-text outline-none focus:border-trace-accent"
							/>
						</label>
						<p className="md:col-span-3 text-[11px] leading-5 text-trace-dim">{importer?.fileHint}</p>
					</div>
				)}
				<button
					type="button"
					disabled={importing || detecting}
					onClick={() => void (importMode === "archives" ? chooseArchives() : chooseFiles())}
					className={`grid min-h-[210px] w-full place-items-center border-0 border-t border-dashed p-8 text-center transition-colors ${dragging ? "border-trace-accent bg-trace-accent-wash" : "border-trace-divider bg-trace-black/30 hover:bg-trace-deep"} disabled:text-trace-dim`}
				>
					<span>
						<svg
							className={`mx-auto size-9 fill-none stroke-current ${dragging ? "text-trace-accent" : "text-trace-muted"}`}
							viewBox="0 0 32 32"
							aria-hidden="true"
						>
							<path strokeWidth="1.5" d="M16 22V7m0 0-5 5m5-5 5 5M7 19v7h18v-7" />
						</svg>
						<strong className="mt-4 block text-[16px] tracking-[.04em] text-white">
							{importing
								? "IMPORTING SETUPS…"
								: dragging
									? "DROP TO IMPORT"
									: importMode === "archives"
										? "DROP SETUP ARCHIVES HERE"
										: "DROP SETUP FILES HERE"}
						</strong>
						<span className="mt-2 block text-[12px] leading-5 text-trace-muted">
							or click to choose multiple {importMode === "archives" ? "archives" : "files"}
						</span>
						<span className="mx-auto mt-4 block max-w-xl font-mono text-[10px] leading-4 text-trace-dim">
							{(importMode === "archives" ? importer?.archiveHint : importer?.fileHint)?.toUpperCase()}
						</span>
					</span>
				</button>
			</div>
			{results.length > 0 && (
				<div className="mt-5 border border-trace-divider bg-trace-surface">
					<div className="flex items-center justify-between border-b border-trace-divider px-5 py-4">
						<strong className="text-[13px] tracking-[.06em] text-white">LAST IMPORT</strong>
						<span className="font-mono text-[10px] text-trace-dim">
							{results.length} ITEM{results.length === 1 ? "" : "S"}
						</span>
					</div>
					<div>
						{results.map((result, index) => (
							<article
								className="grid gap-4 border-b border-trace-divider px-5 py-4 last:border-b-0 md:grid-cols-[minmax(180px,1fr)_minmax(220px,1.5fr)_auto] md:items-center"
								key={`${result.archiveName}-${index}`}
							>
								<div className="min-w-0">
									<strong className="block truncate text-[12px] text-trace-text">{result.archiveName}</strong>
									<span className={`mt-1 block font-mono text-[10px] ${result.success ? "text-trace-accent" : "text-trace-warning"}`}>
										{result.success ? `${result.car ?? "UNKNOWN CAR"} / ${result.track ?? "UNKNOWN TRACK"}` : "NOT IMPORTED"}
									</span>
								</div>
								<div className="min-w-0 text-[11px] leading-5 text-trace-dim">
									<span className="block truncate">{result.error ?? result.destination ?? "No destination reported"}</span>
									{result.indexWarning && <span className="block text-trace-warning">{result.indexWarning}</span>}
								</div>
								<div className="font-mono text-[10px] leading-5 text-right text-trace-muted">
									<span className="block">{result.files.length} INSTALLED</span>
									{result.skipped.length > 0 && <span className="block text-trace-dim">{result.skipped.length} SKIPPED</span>}
								</div>
							</article>
						))}
					</div>
				</div>
			)}
			<p className="mt-4 max-w-4xl text-[11px] leading-5 text-trace-dim">
				Assetto Corsa is the first setup-import adapter. Imported setups are indexed for exact session matches; open a session to mark the setup you
				used, compare its INI values, and include it in a shared .trace package. TRACE cannot automatically identify which setup was active while you
				drove.
			</p>
		</>
	);
}
