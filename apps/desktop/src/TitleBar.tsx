import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ReactNode } from "react";
import type { TelemetryStatus } from "./data-source";
import { Tooltip } from "./Tooltip";

const desktopWindow = isTauri() ? getCurrentWindow() : null;

function runWindowCommand(command: () => Promise<void>) {
  void command().catch((error: unknown) => {
    console.error("TRACE window command failed", error);
  });
}

export function TitleBar({ status, onBack, backLabel = "SESSIONS" }: { status: TelemetryStatus | null; onBack?: () => void; backLabel?: string }) {
  const state = status?.connection ?? "waiting";
  const recording = state === "recording";
  const failed = state === "error";

  return (
    <div className="col-span-full grid select-none grid-cols-[176px_minmax(0,1fr)_auto_auto_88px_auto] items-stretch border-b border-trace-divider bg-trace-black max-[900px]:grid-cols-[140px_minmax(0,1fr)_auto_auto_88px_auto]">
      <div
        className="flex items-center border-r border-trace-divider px-5 text-[18px] font-black tracking-[.12em]"
        data-tauri-drag-region
        onDoubleClick={() => {
          if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
        }}
      >
        <span data-tauri-drag-region>TRACE</span>
        <span className="text-trace-accent" data-tauri-drag-region>//</span>
      </div>
      <div
        className="flex min-w-0 items-center text-xs tracking-[.1em] text-trace-soft"
        data-tauri-drag-region
        onDoubleClick={() => {
          if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
        }}
      >
        {onBack && (
          <button type="button" onClick={onBack} className="flex h-full shrink-0 items-center gap-2 border-0 border-r border-trace-divider bg-transparent px-4 font-bold text-trace-muted hover:bg-trace-raised hover:text-trace-text" aria-label="Back to sessions">
            <svg className="size-4 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true"><path d="m10 3-5 5 5 5" /></svg>
            {backLabel}
          </button>
        )}
        <span className="truncate px-[22px]" data-tauri-drag-region>{status?.session ?? "NO ACTIVE SESSION"}</span>
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
      <div id="trace-titlebar-actions" className="flex h-12 items-stretch" />
      <Tooltip className="h-full" content="Remote spectating is not available yet. Configure the hosted or self-hosted Go Live endpoint in Settings.">
        <button
          type="button"
          disabled
          className="h-full w-[88px] border-0 border-l border-trace-accent-muted bg-trace-accent-wash text-[12px] font-black tracking-[.1em] text-trace-accent-muted"
        >
          GO LIVE
        </button>
      </Tooltip>
      <div className="flex" aria-label="Window controls">
        <WindowButton
          label="Minimize"
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
}: {
  children: ReactNode;
  close?: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <Tooltip className="h-full" content={label}>
      <button
        type="button"
        aria-label={label}
        onClick={onClick}
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
