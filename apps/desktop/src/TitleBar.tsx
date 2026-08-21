import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ReactNode } from "react";

const desktopWindow = isTauri() ? getCurrentWindow() : null;

function runWindowCommand(command: () => Promise<void>) {
  void command().catch((error: unknown) => {
    console.error("TRACE window command failed", error);
  });
}

export function TitleBar() {
  return (
    <div className="col-span-full flex select-none items-stretch border-b border-trace-divider bg-trace-black">
      <div
        className="flex min-w-0 flex-1 items-center gap-3 px-3"
        data-tauri-drag-region
        onDoubleClick={() => {
          if (desktopWindow) runWindowCommand(() => desktopWindow.toggleMaximize());
        }}
      >
        <span className="font-mono text-[9px] font-black tracking-[.16em] text-trace-accent" data-tauri-drag-region>
          TRACE //
        </span>
        <span className="truncate font-mono text-[8px] tracking-[.12em] text-trace-dim" data-tauri-drag-region>
          FIND THE TIME
        </span>
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
      className={`group grid h-9 w-11 place-items-center border-0 border-l border-trace-divider bg-transparent transition-colors focus-visible:z-10 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-trace-accent ${
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
