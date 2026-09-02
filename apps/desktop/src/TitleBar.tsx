import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useState, type MouseEvent, type ReactNode } from "react";
import { telemetryDataSource, type LiveBroadcastOptions, type LiveBroadcastStatus, type TelemetryStatus } from "./data-source";
import { Tooltip } from "./Tooltip";

const desktopWindow = isTauri() ? getCurrentWindow() : null;

function runWindowCommand(command: () => Promise<void>) {
	void command().catch((error: unknown) => {
		console.error("TRACE window command failed", error);
	});
}

export function TitleBar({
	status,
	liveBroadcast,
	liveMode,
	onLiveModeChange,
	onStopLive,
	onBack,
	backLabel = "SESSIONS",
}: {
	status: TelemetryStatus | null;
	liveBroadcast: LiveBroadcastStatus | null;
	liveMode: LiveBroadcastOptions["mode"];
	onLiveModeChange: (mode: LiveBroadcastOptions["mode"]) => void;
	onStopLive: () => void;
	onBack?: () => void;
	backLabel?: string;
}) {
	const [confirmingExit, setConfirmingExit] = useState(false);
	const [exiting, setExiting] = useState(false);
	const state = status?.connection ?? "waiting";
	const recording = state === "recording";
	const failed = state === "error";
	const liveActive = ["connecting", "reconnecting", "live", "ending"].includes(liveBroadcast?.phase ?? "idle");
	const liveButtonLabel =
		liveBroadcast?.phase === "connecting"
			? "CANCEL"
			: liveBroadcast?.phase === "reconnecting"
				? "RECONNECTING…"
				: liveBroadcast?.phase === "ending"
					? "ENDING…"
					: liveBroadcast?.phase === "live"
						? "STOP LIVE"
						: "GO LIVE";

	async function requestFullExit() {
		let shouldConfirm = true;
		try {
			shouldConfirm = (await telemetryDataSource.getAppBehaviorSettings()).confirmExitEnabled;
		} catch (error) {
			console.error("TRACE could not read the exit preference", error);
		}
		if (shouldConfirm) {
			setConfirmingExit(true);
			return;
		}
		await quitCompletely();
	}

	async function quitCompletely() {
		setExiting(true);
		try {
			await telemetryDataSource.quitApp();
		} catch (error) {
			setExiting(false);
			console.error("TRACE could not quit", error);
		}
	}

	return (
		<div className="col-span-full grid select-none grid-cols-[var(--trace-sidebar)_minmax(0,1fr)_auto_auto_auto_88px_auto] items-stretch border-b border-trace-divider bg-trace-black">
			{confirmingExit && (
				<div
					className="fixed inset-0 z-[200] grid place-items-center bg-black/75 p-6"
					role="presentation"
					onMouseDown={(event) => {
						if (event.target === event.currentTarget && !exiting) setConfirmingExit(false);
					}}
				>
					<section
						role="dialog"
						aria-modal="true"
						aria-labelledby="quit-trace-title"
						className="w-full max-w-md border border-trace-divider bg-trace-deep p-6 shadow-2xl"
					>
						<span className="font-mono text-[10px] font-bold tracking-[.14em] text-trace-warning">FULL EXIT</span>
						<h2 id="quit-trace-title" className="mt-2 text-lg font-black text-trace-text">
							Quit TRACE completely?
						</h2>
						<p className="mt-3 text-[13px] leading-5 text-trace-muted">
							Recording, overlays, and active live sessions will stop. Right-click exit confirmation can be disabled in Settings.
						</p>
						<div className="mt-6 grid grid-cols-2 gap-3">
							<button
								type="button"
								disabled={exiting}
								onClick={() => setConfirmingExit(false)}
								className="h-10 border border-trace-divider text-[12px] font-bold text-trace-soft hover:bg-trace-raised disabled:opacity-50"
							>
								CANCEL
							</button>
							<button
								type="button"
								disabled={exiting}
								onClick={() => void quitCompletely()}
								className="h-10 border border-trace-warning text-[12px] font-bold text-trace-warning hover:bg-trace-warning hover:text-trace-black disabled:opacity-50"
							>
								{exiting ? "QUITTING…" : "QUIT TRACE"}
							</button>
						</div>
					</section>
				</div>
			)}
			<div
				className="flex items-center border-r border-trace-divider px-5 text-[18px] font-black tracking-[.12em]"
				data-tauri-drag-region
				onDoubleClick={() => {
					if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
				}}
			>
				<span data-tauri-drag-region>TRACE</span>
				<span className="text-trace-accent" data-tauri-drag-region>
					//
				</span>
			</div>
			<div
				className="flex min-w-0 items-center text-xs tracking-[.1em] text-trace-soft"
				data-tauri-drag-region
				onDoubleClick={() => {
					if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
				}}
			>
				{onBack && (
					<button
						type="button"
						onClick={onBack}
						className="flex h-full shrink-0 items-center gap-2 border-0 border-r border-trace-divider bg-transparent px-4 font-bold text-trace-muted hover:bg-trace-raised hover:text-trace-text"
						aria-label="Back to sessions"
					>
						<svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
							<path d="m10 3-5 5 5 5" />
						</svg>
						{backLabel}
					</button>
				)}
				<span className="truncate px-[22px]" data-tauri-drag-region>
					{status?.session ?? "NO ACTIVE SESSION"}
				</span>
			</div>
			<div className="flex items-center gap-2.5 border-l border-trace-divider px-4 font-mono text-[12px] font-bold tracking-[.1em] text-trace-muted">
				<span
					className={`size-2 rounded-full ${
						recording
							? "animate-pulse bg-trace-accent shadow-[0_0_10px_var(--color-trace-accent)]"
							: failed
								? "bg-trace-warning shadow-[0_0_8px_var(--color-trace-warning)]"
								: "bg-trace-dim"
					}`}
					aria-hidden="true"
				/>
				<span>{state.toUpperCase()}</span>
			</div>
			<div className="flex items-center border-l border-trace-divider p-1" role="group" aria-label="Go Live destination">
				{(["local", "hosted"] as const).map((mode) => (
					<button
						type="button"
						key={mode}
						aria-pressed={liveMode === mode}
						disabled={liveActive}
						onClick={() => onLiveModeChange(mode)}
						className={`h-8 min-w-[62px] px-2 text-[10px] font-black tracking-[.08em] ${liveMode === mode ? "bg-trace-accent text-trace-black" : "text-trace-dim hover:bg-trace-raised hover:text-trace-text"} disabled:cursor-default`}
					>
						{mode === "local" ? "LOCAL" : "ONLINE"}
					</button>
				))}
			</div>
			<div id="trace-titlebar-actions" className="flex h-12 items-stretch" />
			<Tooltip
				className="h-full"
				content={liveActive ? "End the active Go Live broadcast" : "Open a finalized session to stream its recorded telemetry."}
			>
				<button
					type="button"
					disabled={!liveActive || liveBroadcast?.phase === "ending"}
					onClick={onStopLive}
					className="h-full w-[88px] border-0 border-l border-trace-accent-muted bg-trace-accent-wash text-[11px] font-black tracking-[.08em] text-trace-accent disabled:text-trace-accent-muted"
				>
					{liveButtonLabel}
				</button>
			</Tooltip>
			<div className="flex" aria-label="Window controls">
				<WindowButton
					label="Minimize to taskbar"
					onClick={() => {
						if (desktopWindow) runWindowCommand(() => desktopWindow.minimize());
					}}
				>
					<svg viewBox="0 0 12 12" aria-hidden="true">
						<path d="M2 8.5h8" />
					</svg>
				</WindowButton>
				<WindowButton
					label="Maximize or restore"
					onClick={() => {
						if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
					}}
				>
					<svg viewBox="0 0 12 12" aria-hidden="true">
						<rect x="2.5" y="2.5" width="7" height="7" />
					</svg>
				</WindowButton>
				<WindowButton
					label="Close"
					close
					onContextMenu={(event) => {
						event.preventDefault();
						void requestFullExit();
					}}
					onClick={() => {
						if (desktopWindow) runWindowCommand(() => desktopWindow.close());
					}}
				>
					<svg viewBox="0 0 12 12" aria-hidden="true">
						<path d="m2.5 2.5 7 7m0-7-7 7" />
					</svg>
				</WindowButton>
			</div>
		</div>
	);
}

function WindowButton({
	children,
	close = false,
	label,
	onClick,
	onContextMenu,
}: {
	children: ReactNode;
	close?: boolean;
	label: string;
	onClick: () => void;
	onContextMenu?: (event: MouseEvent<HTMLButtonElement>) => void;
}) {
	return (
		<Tooltip className="h-full" content={close ? "Close · right-click to quit completely" : label}>
			<button
				type="button"
				aria-label={label}
				onClick={onClick}
				onContextMenu={onContextMenu}
				className={`group grid h-12 w-12 place-items-center border-0 border-l border-trace-divider bg-transparent transition-colors focus-visible:z-10 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-trace-accent ${
					close ? "hover:bg-trace-danger" : "hover:bg-trace-raised"
				}`}
			>
				<span
					className={`block size-3.5 [&_svg]:size-full [&_svg]:fill-none [&_svg]:stroke-current [&_svg]:stroke-[1.25] ${
						close ? "text-trace-soft group-hover:text-white" : "text-trace-muted group-hover:text-trace-text"
					}`}
				>
					{children}
				</span>
			</button>
		</Tooltip>
	);
}
