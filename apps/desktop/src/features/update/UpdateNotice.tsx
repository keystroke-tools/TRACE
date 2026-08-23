import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { useEffect, useState } from "react";

type UpdatePhase = "available" | "downloading" | "installing" | "restarting" | "failed";

export function UpdateNotice() {
	const [update, setUpdate] = useState<Update | null>(null);
	const [phase, setPhase] = useState<UpdatePhase>("available");
	const [downloadedBytes, setDownloadedBytes] = useState(0);
	const [totalBytes, setTotalBytes] = useState<number | null>(null);
	const [dismissed, setDismissed] = useState(false);

	useEffect(() => {
		if (!isTauri()) return;
		let active = true;
		const timer = window.setTimeout(() => {
			void check({ timeout: 15_000 })
				.then((availableUpdate) => {
					if (active && availableUpdate) setUpdate(availableUpdate);
				})
				.catch((error: unknown) => {
					console.warn("TRACE update check failed", error);
				});
		}, 2_500);
		return () => {
			active = false;
			window.clearTimeout(timer);
		};
	}, []);

	if (!update || dismissed) return null;
	const availableUpdate = update;

	const version = `v${availableUpdate.version.replace(/^v/i, "")}`;
	const progress = totalBytes && totalBytes > 0 ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : null;
	const busy = phase === "downloading" || phase === "installing" || phase === "restarting";

	async function installUpdate() {
		setPhase("downloading");
		setDownloadedBytes(0);
		setTotalBytes(null);
		try {
			await availableUpdate.downloadAndInstall((event: DownloadEvent) => {
				switch (event.event) {
					case "Started":
						setTotalBytes(event.data.contentLength ?? null);
						break;
					case "Progress":
						setDownloadedBytes((current) => current + event.data.chunkLength);
						break;
					case "Finished":
						setPhase("installing");
						break;
				}
			});
			setPhase("restarting");
			await relaunch();
		} catch (error) {
			console.error("TRACE update installation failed", error);
			setPhase("failed");
		}
	}

	return (
		<aside
			className="fixed right-5 top-16 z-[70] w-[420px] max-w-[calc(100vw-2rem)] overflow-hidden border border-trace-accent/50 bg-trace-black shadow-[0_18px_55px_rgba(0,0,0,.7)]"
			aria-live="polite"
			aria-label="TRACE update available"
		>
			<div className="flex items-start gap-4 p-4">
				<span
					className="mt-0.5 grid size-9 shrink-0 place-items-center border border-trace-accent/45 bg-trace-accent-wash text-trace-accent"
					aria-hidden="true"
				>
					<svg className="size-4 fill-none stroke-current stroke-[1.5]" viewBox="0 0 16 16">
						<path d="M8 2v8m0 0 3-3m-3 3L5 7M3 13h10" />
					</svg>
				</span>
				<div className="min-w-0 flex-1">
					<strong className="block text-[13px] font-bold text-trace-text">
						{phase === "failed" ? "THE UPDATE COULD NOT BE INSTALLED" : phase === "restarting" ? "RESTARTING TRACE" : "A NEW VERSION IS AVAILABLE"}
					</strong>
					<p className="mt-1 text-[12px] leading-5 text-trace-muted">
						{phase === "failed"
							? "Check your connection and try again. Your current installation was not changed."
							: busy
								? phase === "installing"
									? `Installing TRACE ${version}…`
									: phase === "restarting"
										? `TRACE ${version} is installed and ready to restart.`
										: `Downloading TRACE ${version}${progress == null ? "…" : ` · ${progress}%`}`
								: `TRACE ${version} is ready to download and install.`}
					</p>
					{phase === "downloading" && (
						<div className="mt-3 h-1 overflow-hidden bg-trace-divider" aria-hidden="true">
							<div
								className={`h-full bg-trace-accent transition-[width] ${progress == null ? "w-1/3 animate-pulse" : ""}`}
								style={progress == null ? undefined : { width: `${progress}%` }}
							/>
						</div>
					)}
					<div className="mt-3 flex items-center gap-2">
						<button
							type="button"
							onClick={() => void installUpdate()}
							disabled={busy}
							className="border border-trace-accent bg-trace-accent px-3 py-2 font-mono text-[10px] font-black tracking-[.08em] text-trace-black hover:bg-trace-text disabled:cursor-wait disabled:border-trace-accent-muted disabled:bg-trace-accent-wash disabled:text-trace-accent-muted"
						>
							{phase === "failed" ? `TRY AGAIN · ${version}` : busy ? "UPDATING…" : `UPDATE NOW · ${version}`}
						</button>
						{!busy && (
							<button
								type="button"
								onClick={() => {
									void availableUpdate.close();
									setDismissed(true);
								}}
								className="px-3 py-2 font-mono text-[10px] font-bold tracking-[.08em] text-trace-dim hover:text-trace-text"
							>
								LATER
							</button>
						)}
					</div>
				</div>
			</div>
		</aside>
	);
}
