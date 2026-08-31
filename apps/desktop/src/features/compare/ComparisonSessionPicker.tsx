import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { RecordedSessionSummary } from "../../data-source";
import { formatCompactSessionDate, formatSessionType, friendlySessionType, lapDuration, lapIsInvalid } from "../sessions/session-components";

export function ComparisonSessionPicker({
	sessions,
	value,
	onChange,
	label,
	rich,
}: {
	sessions: RecordedSessionSummary[];
	value: string;
	onChange: (value: string) => void;
	label: string;
	rich: boolean;
}) {
	if (!rich) {
		return (
			<select
				value={value}
				onChange={(event) => onChange(event.target.value)}
				className="trace-select h-9 min-w-0 border border-trace-divider bg-trace-deep px-3 text-[11px] font-bold leading-none text-trace-text outline-none"
				aria-label={`${label} session`}
			>
				{sessions.map((session) => (
					<option value={session.id} key={session.id}>
						{basicSessionLabel(session)}
					</option>
				))}
			</select>
		);
	}

	return <RichComparisonSessionPicker sessions={sessions} value={value} onChange={onChange} label={label} />;
}

function RichComparisonSessionPicker({
	sessions,
	value,
	onChange,
	label,
}: {
	sessions: RecordedSessionSummary[];
	value: string;
	onChange: (value: string) => void;
	label: string;
}) {
	const [open, setOpen] = useState(false);
	const [query, setQuery] = useState("");
	const [position, setPosition] = useState({ left: 12, bottom: 12 });
	const button = useRef<HTMLButtonElement>(null);
	const popover = useRef<HTMLDivElement>(null);
	const selected = sessions.find((session) => session.id === value) ?? null;
	const queryTokens = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
	const visibleSessions = sessions.filter((session) => {
		const bestLap = bestValidLap(session);
		const searchable = [
			session.driver,
			session.title,
			session.car,
			session.track,
			session.sessionType,
			friendlySessionType(session),
			formatCompactSessionDate(session.startedAt),
			bestLap?.time,
		]
			.filter(Boolean)
			.join(" ")
			.toLocaleLowerCase();
		return queryTokens.every((token) => searchable.includes(token));
	});

	useEffect(() => {
		if (!open) return;
		function placePopover() {
			const bounds = button.current?.getBoundingClientRect();
			if (!bounds) return;
			const width = Math.min(520, window.innerWidth - 24);
			setPosition({
				left: Math.max(12, Math.min(bounds.left, window.innerWidth - width - 12)),
				bottom: window.innerHeight - bounds.top + 8,
			});
		}
		function dismiss(event: PointerEvent) {
			const target = event.target as Node;
			if (!button.current?.contains(target) && !popover.current?.contains(target)) setOpen(false);
		}
		function closeOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape") {
				setOpen(false);
				button.current?.focus();
			}
		}
		placePopover();
		document.addEventListener("pointerdown", dismiss);
		document.addEventListener("keydown", closeOnEscape);
		window.addEventListener("resize", placePopover);
		return () => {
			document.removeEventListener("pointerdown", dismiss);
			document.removeEventListener("keydown", closeOnEscape);
			window.removeEventListener("resize", placePopover);
		};
	}, [open]);

	const selectedBest = selected ? bestValidLap(selected) : null;
	return (
		<>
			<button
				ref={button}
				type="button"
				aria-label={`${label} session`}
				aria-haspopup="listbox"
				aria-expanded={open}
				onClick={() => {
					setQuery("");
					setOpen((current) => !current);
				}}
				className={`grid h-9 min-w-0 grid-cols-[minmax(0,1fr)_auto_12px] items-center gap-2 border bg-trace-deep px-3 text-left ${open ? "border-trace-soft" : "border-trace-divider hover:border-trace-soft"}`}
			>
				<span className="truncate text-[11px] font-bold text-trace-text">{selected ? sessionIdentity(selected) : "Choose session"}</span>
				<span className="shrink-0 font-mono text-[10px] font-black text-trace-purple">{selectedBest?.time ?? "—"}</span>
				<svg
					className={`size-3 fill-none stroke-current text-trace-muted transition-transform ${open ? "rotate-180" : ""}`}
					viewBox="0 0 12 12"
					aria-hidden="true"
				>
					<path d="m2.5 4.5 3.5 3 3.5-3" />
				</svg>
			</button>
			{open &&
				createPortal(
					<div
						ref={popover}
						role="dialog"
						aria-label={`${label} sessions`}
						className="fixed z-[90] max-h-[360px] w-[520px] overflow-y-auto border border-trace-divider bg-trace-black"
						style={{ left: position.left, bottom: position.bottom, maxWidth: "calc(100vw - 24px)" }}
					>
						<div className="sticky top-0 z-10 border-b border-trace-divider bg-trace-black p-3">
							<div className="flex items-center justify-between gap-4 px-1">
								<strong className="font-mono text-[10px] tracking-[.1em] text-trace-soft">CHOOSE {label.toUpperCase()} SESSION</strong>
								<span className="font-mono text-[9px] text-trace-dim">
									{visibleSessions.length} / {sessions.length}
								</span>
							</div>
							<label className="relative mt-2 block">
								<span className="sr-only">Search sessions</span>
								<svg
									className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 fill-none stroke-current text-trace-dim"
									viewBox="0 0 16 16"
									aria-hidden="true"
								>
									<circle cx="7" cy="7" r="4.5" />
									<path d="m10.5 10.5 3 3" />
								</svg>
								<input
									autoFocus
									value={query}
									onChange={(event) => setQuery(event.target.value)}
									placeholder="Search driver, car, track, date, or lap time"
									className="h-9 w-full border border-trace-divider bg-trace-deep pl-9 pr-3 text-[11px] text-trace-text outline-none placeholder:text-trace-dim focus:border-trace-accent"
								/>
							</label>
						</div>
						{visibleSessions.map((session) => {
							const bestLap = bestValidLap(session);
							const current = session.id === value;
							return (
								<button
									type="button"
									aria-current={current ? "true" : undefined}
									onClick={() => {
										onChange(session.id);
										setOpen(false);
										button.current?.focus();
									}}
									className={`grid w-full grid-cols-[minmax(0,1fr)_112px] items-center gap-5 border-b border-trace-divider px-4 py-3 text-left last:border-b-0 ${current ? "bg-trace-purple-wash" : "hover:bg-trace-deep"}`}
									key={session.id}
								>
									<span className="min-w-0">
										<span className="flex min-w-0 items-center gap-2">
											<strong className="truncate text-[13px] leading-5 text-trace-text">{sessionIdentity(session)}</strong>
											{current && (
												<span className="shrink-0 border border-trace-purple/50 bg-trace-purple-wash px-1.5 py-0.5 font-mono text-[8px] font-black text-trace-purple">
													CURRENT
												</span>
											)}
										</span>
										<span className="mt-1 block truncate text-[11px] leading-4 text-trace-muted">
											{session.car} · {session.track}
										</span>
										<span className="mt-1 flex flex-wrap items-center gap-x-2 font-mono text-[9px] font-bold leading-4 text-trace-dim">
											<span>{formatSessionType(session.sessionType)}</span>
											<span aria-hidden="true">·</span>
											<time dateTime={session.startedAt}>{formatCompactSessionDate(session.startedAt)}</time>
										</span>
									</span>
									<span className="text-right font-mono">
										<strong className="block text-[14px] text-trace-purple">{bestLap?.time ?? "—"}</strong>
										<span className="mt-1 block text-[9px] font-bold tracking-[.08em] text-trace-dim">
											{bestLap ? `BEST · LAP ${bestLap.index}` : "NO CLEAN LAP"}
										</span>
									</span>
								</button>
							);
						})}
						{visibleSessions.length === 0 && (
							<div className="px-5 py-8 text-center">
								<strong className="block text-[12px] text-trace-soft">No matching sessions</strong>
								<p className="mt-1 text-[11px] leading-5 text-trace-dim">
									Try fewer terms or search a different car, track, driver, date, or lap time.
								</p>
							</div>
						)}
					</div>,
					document.body,
				)}
		</>
	);
}

function sessionIdentity(session: RecordedSessionSummary) {
	return session.driver?.trim() || session.title?.trim() || "Unnamed session";
}

function bestValidLap(session: RecordedSessionSummary) {
	return session.laps.filter((lap) => lap.time !== "—" && !lapIsInvalid(lap)).sort((left, right) => lapDuration(left) - lapDuration(right))[0];
}

function basicSessionLabel(session: RecordedSessionSummary) {
	const bestLap = bestValidLap(session);
	const best = bestLap ? `Best ${bestLap.time}` : "No clean lap";
	return `${sessionIdentity(session)} · ${best} · ${formatCompactSessionDate(session.startedAt)} · ${session.car} @ ${session.track} · ${friendlySessionType(session)}`;
}
