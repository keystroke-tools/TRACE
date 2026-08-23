import { isTauri } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useState } from "react";
import { useToast } from "../../Toast";
import { PEDAL_OVERLAY_WIDTH } from "./PedalOverlay";

const OVERLAY_LABEL = "pedal-overlay";

export function PedalOverlayLauncher() {
	const showToast = useToast();
	const [opening, setOpening] = useState(false);
	const [closing, setClosing] = useState(false);

	async function openOverlay() {
		setOpening(true);
		try {
			if (!isTauri()) {
				window.open(`${window.location.pathname}?view=pedals`, OVERLAY_LABEL, `popup,width=${PEDAL_OVERLAY_WIDTH},height=200`);
				return;
			}

			const existing = await WebviewWindow.getByLabel(OVERLAY_LABEL);
			if (existing) {
				await existing.unminimize();
				await existing.show();
				await existing.setFocus();
				return;
			}

			await new Promise<void>((resolve, reject) => {
				const overlay = new WebviewWindow(OVERLAY_LABEL, {
					url: "/?view=pedals",
					title: "TRACE // PEDALS",
					width: PEDAL_OVERLAY_WIDTH,
					height: 200,
					minWidth: 360,
					minHeight: 100,
					resizable: true,
					decorations: false,
					transparent: true,
					alwaysOnTop: true,
					shadow: false,
				});
				overlay.once("tauri://created", () => resolve());
				overlay.once("tauri://error", (event) => reject(event.payload));
			});
		} catch (error) {
			showToast({
				kind: "error",
				title: "Pedal overlay unavailable",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 7_000,
			});
		} finally {
			setOpening(false);
		}
	}

	async function closeOverlay() {
		setClosing(true);
		try {
			if (!isTauri()) {
				showToast({
					kind: "info",
					title: "Close the popup window",
					message: "TRACE cannot close a browser popup after it loses its window reference.",
				});
				return;
			}
			const existing = await WebviewWindow.getByLabel(OVERLAY_LABEL);
			if (!existing) {
				showToast({ kind: "info", title: "Overlay is not open", message: "There is no standalone pedal overlay to close." });
				return;
			}
			await existing.close();
		} catch (error) {
			showToast({
				kind: "error",
				title: "Could not close overlay",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 7_000,
			});
		} finally {
			setClosing(false);
		}
	}

	return (
		<div className="flex items-stretch gap-3">
			<button
				type="button"
				disabled={opening}
				onClick={() => void openOverlay()}
				className="border border-trace-divider bg-trace-raised px-4 py-3 text-[12px] font-black tracking-[.1em] text-trace-soft hover:border-trace-accent-muted hover:text-trace-accent disabled:opacity-50"
			>
				{opening ? "OPENING…" : "OPEN PEDAL OVERLAY"}
			</button>
			<button
				type="button"
				disabled={closing}
				onClick={() => void closeOverlay()}
				className="border border-trace-divider bg-trace-deep px-3 py-3 text-[11px] font-black tracking-[.1em] text-trace-dim hover:border-trace-danger hover:bg-trace-danger/10 hover:text-red-300 disabled:opacity-50"
			>
				{closing ? "CLOSING…" : "CLOSE OVERLAY"}
			</button>
		</div>
	);
}
