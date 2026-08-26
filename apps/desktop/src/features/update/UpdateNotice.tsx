import { useUpdater } from "./UpdateContext";

export function UpdateNotice() {
	const { availableVersion, dismissNotice, downloadProgress, error, installUpdate, noticeVisible, phase } = useUpdater();
	if (!availableVersion || !noticeVisible) return null;

	const busy = phase === "downloading" || phase === "installing" || phase === "restarting";
	const message =
		phase === "failed"
			? error || "The update could not be installed."
			: phase === "downloading"
				? `Downloading${downloadProgress == null ? "…" : ` · ${downloadProgress}%`}`
				: phase === "installing"
					? "Installing update…"
					: phase === "restarting"
						? "Restarting TRACE…"
						: `${availableVersion} is ready to install.`;

	return (
		<section className="shrink-0 border-t border-trace-divider p-3" aria-live="polite" aria-label="TRACE update available">
			<div className="flex items-center gap-2 text-trace-accent">
				<svg className="size-4 shrink-0 fill-none stroke-current stroke-[1.5]" viewBox="0 0 16 16" aria-hidden="true">
					<path d="M8 2v8m0 0 3-3m-3 3L5 7M3 13h10" />
				</svg>
				<strong className="min-w-0 font-mono text-[10px] tracking-[.08em]">UPDATE AVAILABLE</strong>
			</div>
			<p className="mt-2 break-words text-[11px] leading-4 text-trace-muted">{message}</p>
			{phase === "downloading" && (
				<div className="mt-2 h-1 overflow-hidden bg-trace-divider" aria-hidden="true">
					<div
						className={`h-full bg-trace-accent transition-[width] ${downloadProgress == null ? "w-1/3 animate-pulse" : ""}`}
						style={downloadProgress == null ? undefined : { width: `${downloadProgress}%` }}
					/>
				</div>
			)}
			<button
				type="button"
				onClick={() => void installUpdate()}
				disabled={busy}
				className="mt-3 w-full border border-trace-accent bg-trace-accent-wash px-2 py-2 font-mono text-[10px] font-black tracking-[.05em] text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:cursor-wait disabled:border-trace-accent-muted disabled:text-trace-accent-muted"
			>
				{phase === "failed" ? "TRY AGAIN" : busy ? "UPDATING…" : `UPDATE · ${availableVersion}`}
			</button>
			{!busy && (
				<button
					type="button"
					onClick={dismissNotice}
					className="mt-1 w-full border-0 bg-transparent py-1.5 font-mono text-[9px] font-bold tracking-[.08em] text-trace-dim hover:text-trace-text"
				>
					LATER
				</button>
			)}
		</section>
	);
}
