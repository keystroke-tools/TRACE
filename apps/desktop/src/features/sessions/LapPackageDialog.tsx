import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { telemetryDataSource, type RecordedLapSummary, type RecordedSessionSummary } from "../../data-source";
import { useToast } from "../../Toast";
import { rememberSharedSession, sharedSessionsFor, type StoredSharedSession } from "./share-history";

export type LapPackageAction = "share" | "export";

export function LapPackageDialog({ session, action, onClose }: { session: RecordedSessionSummary; action: LapPackageAction; onClose: () => void }) {
	const showToast = useToast();
	const laps = useMemo(() => session.laps.filter((lap) => lap.durationNs != null), [session.laps]);
	const initialLap = laps.find((lap) => lap.isFastest) ?? laps.find((lap) => lap.validity === "valid") ?? laps[0];
	const [selected, setSelected] = useState<Set<number>>(() => new Set(initialLap ? [initialLap.index] : []));
	const [submitting, setSubmitting] = useState(false);
	const [history, setHistory] = useState<StoredSharedSession[]>(() => sharedSessionsFor(session.id));
	const selectedIndices = [...selected].sort((left, right) => left - right);
	const matchingShare = action === "share" ? history.find((share) => sameLaps(share.lapIndices, selectedIndices)) : undefined;

	useEffect(() => {
		function closeOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape" && !submitting) onClose();
		}
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose, submitting]);

	function toggleLap(index: number) {
		setSelected((current) => {
			const next = new Set(current);
			if (next.has(index)) next.delete(index);
			else next.add(index);
			return next;
		});
	}

	async function copyLink(url: string) {
		try {
			await navigator.clipboard.writeText(url);
			showToast({ kind: "success", title: "Share link copied", message: url, timeoutMs: 5_000 });
		} catch {
			showToast({ kind: "error", title: "Could not copy link", message: url, timeoutMs: 9_000 });
		}
	}

	async function submit() {
		const lapIndices = selectedIndices;
		if (lapIndices.length === 0) return;
		if (matchingShare) {
			await copyLink(matchingShare.shareUrl);
			return;
		}
		setSubmitting(true);
		try {
			if (action === "share") {
				const result = await telemetryDataSource.shareSession(session.id, lapIndices);
				const remembered = rememberSharedSession(session.id, lapIndices, result);
				setHistory((current) => [remembered, ...current.filter((item) => item.shareId !== remembered.shareId)]);
				await copyLink(result.shareUrl);
			} else {
				const result = await telemetryDataSource.exportSession(session.id, "trace", lapIndices);
				showToast({
					kind: "success",
					title: ".trace package exported",
					message: `${lapIndices.length} ${lapIndices.length === 1 ? "lap" : "laps"} · ${result.sampleCount.toLocaleString()} samples saved to ${result.path}`,
					timeoutMs: 8_000,
				});
				onClose();
			}
		} catch (error) {
			showToast({
				kind: "error",
				title: action === "share" ? "Could not share session" : "Export failed",
				message: error instanceof Error ? error.message : String(error),
				timeoutMs: 9_000,
			});
		} finally {
			setSubmitting(false);
		}
	}

	return createPortal(
		<div
			className="fixed inset-0 z-[80] grid place-items-center bg-black/75 p-5"
			onMouseDown={(event) => event.target === event.currentTarget && !submitting && onClose()}
		>
			<section
				role="dialog"
				aria-modal="true"
				aria-label={action === "share" ? "Share selected laps" : "Export selected laps"}
				className="flex max-h-[min(760px,calc(100vh-40px))] w-full max-w-2xl flex-col overflow-hidden border border-trace-divider bg-trace-black"
			>
				<header className="flex items-start justify-between gap-4 border-b border-trace-divider px-5 py-4">
					<div>
						<span className="font-mono text-[10px] font-bold tracking-[.14em] text-trace-accent">
							{action === "share" ? "HOSTED SHARE" : "LOCAL PACKAGE"}
						</span>
						<h2 className="mt-1 text-lg font-black">Choose the laps to include</h2>
						<p className="mt-1 text-[12px] leading-5 text-trace-muted">
							Only selected telemetry is written, keeping long race packages manageable.
						</p>
					</div>
					<button
						type="button"
						onClick={onClose}
						disabled={submitting}
						className="grid size-8 shrink-0 place-items-center border border-trace-divider text-trace-muted hover:text-white"
						aria-label="Close lap picker"
					>
						<svg className="size-3.5 fill-none stroke-current" viewBox="0 0 16 16" aria-hidden="true">
							<path d="m3 3 10 10M13 3 3 13" />
						</svg>
					</button>
				</header>

				<div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
					<div className="flex flex-wrap items-center gap-2">
						<SelectionButton label="FASTEST" onClick={() => initialLap && setSelected(new Set([initialLap.index]))} />
						<SelectionButton
							label="VALID"
							onClick={() => setSelected(new Set(laps.filter((lap) => lap.validity === "valid").map((lap) => lap.index)))}
						/>
						<SelectionButton label="ALL" onClick={() => setSelected(new Set(laps.map((lap) => lap.index)))} />
						<button
							type="button"
							onClick={() => setSelected(new Set())}
							className="ml-auto border-0 bg-transparent px-2 py-1 font-mono text-[10px] font-bold text-trace-dim hover:text-white"
						>
							CLEAR
						</button>
					</div>

					<div className="grid max-h-72 grid-cols-1 gap-px overflow-y-auto bg-trace-divider sm:grid-cols-2">
						{laps.map((lap) => (
							<LapChoice key={lap.index} lap={lap} selected={selected.has(lap.index)} onToggle={() => toggleLap(lap.index)} />
						))}
					</div>

					{action === "share" && history.length > 0 && (
						<section className="border-t border-trace-divider pt-4">
							<div className="mb-2 flex items-center justify-between">
								<span className="font-mono text-[10px] font-bold tracking-[.12em] text-trace-dim">ACTIVE LINKS</span>
								<span className="text-[11px] text-trace-dim">Stored on this device until expiry</span>
							</div>
							<div className="divide-y divide-trace-divider border-y border-trace-divider">
								{history.slice(0, 4).map((share) => (
									<button
										key={share.shareId}
										type="button"
										onClick={() => void copyLink(share.shareUrl)}
										className="flex w-full items-center justify-between gap-4 border-0 bg-transparent px-1 py-3 text-left hover:bg-trace-raised"
									>
										<span className="min-w-0">
											<strong className="block truncate text-[12px] text-trace-soft">Laps {compactLapList(share.lapIndices)}</strong>
											<span className="mt-0.5 block text-[11px] text-trace-dim">
												Expires {new Date(share.expiresAtUnixMs).toLocaleString()}
											</span>
										</span>
										<span className="shrink-0 font-mono text-[10px] font-bold text-trace-accent">COPY LINK</span>
									</button>
								))}
							</div>
						</section>
					)}
				</div>

				<footer className="flex items-center justify-between gap-4 border-t border-trace-divider px-5 py-4">
					<span className="text-[12px] text-trace-muted">
						{selected.size} {selected.size === 1 ? "lap" : "laps"} selected
					</span>
					<button
						type="button"
						disabled={selected.size === 0 || submitting}
						onClick={() => void submit()}
						className="h-9 bg-trace-accent px-4 font-mono text-[10px] font-black tracking-[.08em] text-trace-black hover:bg-white disabled:bg-trace-raised disabled:text-trace-dim"
					>
						{submitting
							? action === "share"
								? "UPLOADING…"
								: "EXPORTING…"
							: matchingShare
								? "COPY EXISTING LINK"
								: action === "share"
									? "CREATE LINK"
									: "EXPORT .TRACE"}
					</button>
				</footer>
			</section>
		</div>,
		document.body,
	);
}

function LapChoice({ lap, selected, onToggle }: { lap: RecordedLapSummary; selected: boolean; onToggle: () => void }) {
	return (
		<button
			type="button"
			aria-pressed={selected}
			onClick={onToggle}
			className={`flex min-w-0 items-center gap-3 border-0 px-3 py-3 text-left ${selected ? "bg-trace-accent-wash" : "bg-trace-deep hover:bg-trace-raised"}`}
		>
			<span
				className={`grid size-4 shrink-0 place-items-center border ${selected ? "border-trace-accent bg-trace-accent text-trace-black" : "border-trace-divider"}`}
			>
				{selected && (
					<svg className="size-3 fill-none stroke-current stroke-[2]" viewBox="0 0 16 16" aria-hidden="true">
						<path d="m3 8 3 3 7-7" />
					</svg>
				)}
			</span>
			<span className="min-w-0 flex-1">
				<strong className="block font-mono text-[12px] text-trace-text">Lap {lap.index}</strong>
				<span className="mt-0.5 block text-[11px] text-trace-dim">
					{lap.validity === "invalid" ? "Invalid" : lap.validity === "valid" ? "Valid" : "Unverified"}
				</span>
			</span>
			<strong className={lap.isFastest ? "font-mono text-[12px] text-trace-purple" : "font-mono text-[12px] text-trace-soft"}>{lap.time}</strong>
		</button>
	);
}

function SelectionButton({ label, onClick }: { label: string; onClick: () => void }) {
	return (
		<button
			type="button"
			onClick={onClick}
			className="border border-trace-divider bg-trace-deep px-2.5 py-1.5 font-mono text-[10px] font-bold text-trace-soft hover:border-trace-accent/60 hover:text-white"
		>
			{label}
		</button>
	);
}

function compactLapList(indices: number[]) {
	if (indices.length <= 5) return indices.join(", ");
	return `${indices.slice(0, 4).join(", ")} +${indices.length - 4}`;
}

function sameLaps(left: number[], right: number[]) {
	return left.length === right.length && left.every((value, index) => value === right[index]);
}
