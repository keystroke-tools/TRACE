import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

export type ToastKind = "success" | "error" | "info";

export interface ToastInput {
	message: string;
	title?: string;
	kind?: ToastKind;
	timeoutMs?: number | null;
}

interface ToastRecord extends ToastInput {
	id: number;
	kind: ToastKind;
	timeoutMs: number | null;
}

const ToastContext = createContext<((toast: ToastInput) => number) | null>(null);
const EXIT_DURATION_MS = 180;

export function ToastProvider({ children }: { children: ReactNode }) {
	const nextId = useRef(1);
	const [toasts, setToasts] = useState<ToastRecord[]>([]);
	const showToast = useCallback((toast: ToastInput) => {
		const id = nextId.current++;
		setToasts((current) =>
			[
				...current,
				{
					...toast,
					id,
					kind: toast.kind ?? "info",
					timeoutMs: toast.timeoutMs === undefined ? 4_500 : toast.timeoutMs,
				},
			].slice(-5),
		);
		return id;
	}, []);
	const removeToast = useCallback((id: number) => {
		setToasts((current) => current.filter((toast) => toast.id !== id));
	}, []);

	return (
		<ToastContext.Provider value={showToast}>
			{children}
			<div
				className="pointer-events-none fixed bottom-6 right-6 z-[110] flex w-[min(380px,calc(100vw-48px))] flex-col gap-2"
				aria-live="polite"
				aria-atomic="false"
			>
				{toasts.map((toast) => (
					<Toast toast={toast} onRemove={removeToast} key={toast.id} />
				))}
			</div>
		</ToastContext.Provider>
	);
}

export function useToast() {
	const showToast = useContext(ToastContext);
	if (!showToast) throw new Error("useToast must be used inside ToastProvider");
	return showToast;
}

function Toast({ toast, onRemove }: { toast: ToastRecord; onRemove: (id: number) => void }) {
	const [leaving, setLeaving] = useState(false);
	const leavingRef = useRef(false);
	const dismissTimer = useRef<number | null>(null);
	const removalTimer = useRef<number | null>(null);

	const dismiss = useCallback(() => {
		if (leavingRef.current) return;
		leavingRef.current = true;
		setLeaving(true);
		removalTimer.current = window.setTimeout(() => onRemove(toast.id), EXIT_DURATION_MS);
	}, [onRemove, toast.id]);

	useEffect(() => {
		if (toast.timeoutMs !== null) dismissTimer.current = window.setTimeout(dismiss, toast.timeoutMs);
		return () => {
			if (dismissTimer.current !== null) window.clearTimeout(dismissTimer.current);
			if (removalTimer.current !== null) window.clearTimeout(removalTimer.current);
		};
	}, [dismiss, toast.timeoutMs]);

	const accent = toast.kind === "success" ? "border-l-trace-accent" : toast.kind === "error" ? "border-l-trace-warning" : "border-l-trace-purple";
	const label = toast.title ?? (toast.kind === "success" ? "Done" : toast.kind === "error" ? "Action failed" : "TRACE");

	return (
		<div
			className={`${leaving ? "trace-toast-leave" : "trace-toast-enter"} pointer-events-auto relative overflow-hidden border border-l-[3px] border-trace-divider ${accent} bg-trace-black shadow-[0_14px_36px_#000]`}
			role={toast.kind === "error" ? "alert" : "status"}
		>
			<div className="flex gap-3 p-4">
				<div className="min-w-0 flex-1">
					<strong className="block font-mono text-[12px] tracking-[.1em] text-trace-soft">{label.toUpperCase()}</strong>
					<p className="mt-1.5 break-words text-[12px] leading-5 text-trace-faint">{toast.message}</p>
				</div>
				<button
					type="button"
					onClick={dismiss}
					className="grid size-7 shrink-0 place-items-center border-0 bg-transparent text-base text-trace-muted hover:bg-trace-raised hover:text-trace-text"
					aria-label="Dismiss notification"
				>
					×
				</button>
			</div>
			{toast.timeoutMs !== null && !leaving && (
				<span
					className={`trace-toast-progress absolute bottom-0 left-0 h-px ${toast.kind === "success" ? "bg-trace-accent" : toast.kind === "error" ? "bg-trace-warning" : "bg-trace-purple"}`}
					style={{ animationDuration: `${toast.timeoutMs}ms` }}
					aria-hidden="true"
				/>
			)}
		</div>
	);
}
