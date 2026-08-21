import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ReactNode } from "react";
import type { TelemetryStatus } from "./data-source";

const desktopWindow = isTauri() ? getCurrentWindow() : null;

function runWindowCommand(command: () => Promise<void>) {
  void command().catch((error: unknown) => {
    console.error("TRACE window command failed", error);
  });
}

export function TitleBar({ status }: { status: TelemetryStatus | null }) {
  const state = status?.connection ?? "waiting";
  const recording = state === "recording";
  const failed = state === "error";

  return (
    <div className="col-span-full grid select-none grid-cols-[176px_minmax(0,1fr)_auto_auto] items-stretch border-b border-trace-divider bg-trace-black max-[900px]:grid-cols-[140px_minmax(0,1fr)_auto_auto]">
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
        className="flex min-w-0 items-center px-[22px] text-xs tracking-[.1em] text-trace-soft"
        data-tauri-drag-region
        onDoubleClick={() => {
          if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
        }}
      >
        <span className="truncate" data-tauri-drag-region>{status?.session ?? "NO ACTIVE SESSION"}</span>
      </div>
      <div className="flex items-center gap-2.5 border-l border-trace-divider px-4 font-mono text-[10px] font-bold tracking-[.1em] text-trace-muted">
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
    <button
      type="button"
      aria-label={label}
      title={label}
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
  );
}
