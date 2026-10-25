import { useMemo, useState } from "react";
import { telemetryDataSource, type MotecFolderScan } from "../../data-source";
import { ImportProgressOverlay } from "../../components/ImportProgressOverlay";
import { useToast } from "../../Toast";

export function MotecFolderImportModal({ scan, onClose, onImported }: { scan: MotecFolderScan; onClose: () => void; onImported: () => Promise<void> }) {
	const showToast = useToast();
	const importable = useMemo(() => scan.entries.filter((entry) => entry.preview), [scan.entries]);
	const [selected, setSelected] = useState(() => new Set(importable.map((entry) => entry.path)));
	const [imported, setImported] = useState(() => new Set<string>());
	const [failures, setFailures] = useState(() => new Map<string, string>());
	const [progress, setProgress] = useState<{ current: number; total: number } | null>(null);

	function toggle(path: string) {
		setSelected((current) => {
			const next = new Set(current);
			if (next.has(path)) next.delete(path);
			else next.add(path);
			return next;
		});
	}

	async function importSelected() {
		const entries = importable.filter((entry) => selected.has(entry.path) && !imported.has(entry.path));
		if (!entries.length) return;
		setProgress({ current: 0, total: entries.length });
		const nextImported = new Set(imported);
		const nextFailures = new Map(failures);
		for (const [index, entry] of entries.entries()) {
			setProgress({ current: index + 1, total: entries.length });
			try {
				await telemetryDataSource.importSession(entry.path);
				nextImported.add(entry.path);
				nextFailures.delete(entry.path);
			} catch (error) {
				nextFailures.set(entry.path, error instanceof Error ? error.message : String(error));
			}
			setImported(new Set(nextImported));
			setFailures(new Map(nextFailures));
		}
		if (nextImported.size > imported.size) await onImported();
		setProgress(null);
		const importedNow = nextImported.size - imported.size;
		showToast({
			kind: nextFailures.size ? "error" : "success",
			title: nextFailures.size ? "Folder import finished with errors" : "Folder import complete",
			message: `${importedNow} ${importedNow === 1 ? "session" : "sessions"} imported${nextFailures.size ? ` · ${nextFailures.size} failed` : ""}.`,
			timeoutMs: nextFailures.size ? 9_000 : 5_000,
		});
		if (!nextFailures.size) onClose();
	}

	const remaining = [...selected].filter((path) => !imported.has(path)).length;
	return (
		<div className="fixed inset-0 z-[95] grid place-items-center bg-black/80 p-5" role="dialog" aria-modal="true" aria-labelledby="motec-folder-title">
			{progress && (
				<ImportProgressOverlay
					eyebrow="BULK MOTEC IMPORT"
					title={`Importing session ${progress.current} of ${progress.total}`}
					detail="Validating the telemetry log and adding its laps to your library."
					stage="IMPORTING"
					current={progress.current}
					total={progress.total}
				/>
			)}
			<section className="flex max-h-[min(760px,calc(100vh-40px))] w-full max-w-4xl flex-col border border-trace-divider bg-trace-deep">
				<header className="flex items-start justify-between gap-5 border-b border-trace-divider p-5">
					<div className="min-w-0">
						<p className="font-mono text-[10px] font-bold tracking-[.14em] text-trace-accent">BULK MOTEC IMPORT</p>
						<h2 id="motec-folder-title" className="mt-2 text-xl font-black">
							Review discovered outings
						</h2>
						<p className="mt-1 truncate text-[12px] text-trace-muted" title={scan.folder}>
							{scan.folder}
						</p>
					</div>
					<button
						type="button"
						disabled={Boolean(progress)}
						onClick={onClose}
						className="px-2 py-1 font-mono text-[11px] text-trace-muted hover:text-white disabled:text-trace-dim"
					>
						CLOSE
					</button>
				</header>

				<div className="flex items-center justify-between gap-4 border-b border-trace-divider bg-trace-surface px-5 py-3 text-[11px]">
					<span className="text-trace-soft">
						{importable.length} ready · {scan.entries.length - importable.length} unavailable
						{scan.limited ? " · first 250 logs shown" : ""}
					</span>
					<button
						type="button"
						disabled={Boolean(progress) || !importable.length}
						onClick={() => setSelected(selected.size === importable.length ? new Set() : new Set(importable.map((entry) => entry.path)))}
						className="font-mono text-[10px] font-bold tracking-[.08em] text-trace-accent hover:text-white disabled:text-trace-dim"
					>
						{selected.size === importable.length ? "CLEAR ALL" : "SELECT ALL"}
					</button>
				</div>

				<div className="min-h-0 flex-1 overflow-y-auto">
					{scan.entries.length === 0 && <p className="p-8 text-center text-[13px] text-trace-muted">No MoTeC .ld logs were found in this folder.</p>}
					{scan.entries.map((entry) => {
						const error = failures.get(entry.path) || entry.error;
						const done = imported.has(entry.path);
						return (
							<label
								key={entry.path}
								className={`grid grid-cols-[20px_minmax(0,1fr)_auto] items-start gap-3 border-b border-trace-divider px-5 py-4 ${entry.preview ? "hover:bg-trace-raised" : "bg-trace-surface/45"}`}
							>
								<input
									type="checkbox"
									checked={selected.has(entry.path)}
									disabled={!entry.preview || Boolean(progress) || done}
									onChange={() => toggle(entry.path)}
									className="mt-1 accent-trace-accent"
								/>
								<span className="min-w-0">
									<strong className="block truncate text-[13px] text-trace-text" title={entry.path}>
										{entry.fileName}
									</strong>
									{entry.preview && (
										<small className="mt-1 block truncate text-[11px] text-trace-muted">
											{entry.preview.driver || "Unknown driver"} · {entry.preview.car || "Unknown car"} ·{" "}
											{entry.preview.track || "Unknown track"} · {entry.preview.completeLapCount} laps
										</small>
									)}
									{error && <small className="mt-1 block text-[11px] leading-4 text-trace-danger">{error}</small>}
								</span>
								<span
									className={`font-mono text-[9px] font-bold tracking-[.08em] ${done ? "text-trace-accent" : entry.sidecarPresent ? "text-trace-muted" : "text-trace-warning"}`}
								>
									{done ? "IMPORTED" : entry.sidecarPresent ? "LD + LDX" : "NO LDX"}
								</span>
							</label>
						);
					})}
				</div>

				<footer className="flex items-center justify-between gap-4 border-t border-trace-divider p-5">
					<p className="text-[11px] text-trace-muted">
						{progress
							? `Importing ${progress.current} of ${progress.total}…`
							: "Logs without sidecars may still import when lap markers exist in the log."}
					</p>
					<button
						type="button"
						disabled={Boolean(progress) || remaining === 0}
						onClick={() => void importSelected()}
						className="h-10 shrink-0 bg-trace-accent px-5 font-mono text-[11px] font-black tracking-[.08em] text-black hover:bg-white disabled:bg-trace-divider disabled:text-trace-dim"
					>
						{progress ? `IMPORTING ${progress.current}/${progress.total}` : `IMPORT ${remaining} SELECTED`}
					</button>
				</footer>
			</section>
		</div>
	);
}
