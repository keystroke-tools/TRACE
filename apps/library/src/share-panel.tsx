import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

type Summary = {
	session_id: string;
	title: string | null;
	driver_name: string | null;
	track: string | null;
	share_id?: string | null;
};

type ShareSnapshot = {
	summary: Summary;
	shareUrl: string | null;
	open: boolean;
};

type ShareResponse = { share_id: string; share_url: string };

declare global {
	interface Window {
		traceShareSnapshot?: ShareSnapshot | null;
	}
}

async function request<T>(path: string, options: RequestInit): Promise<T> {
	const response = await fetch(path, { credentials: "same-origin", ...options });
	if (response.status === 204) return undefined as T;
	const contentType = response.headers.get("content-type") || "";
	const body = contentType.includes("application/json") ? await response.json() : await response.text();
	if (!response.ok) {
		const detail = typeof body === "string" ? body.trim() : body?.error;
		throw new Error(detail || `Request failed (${response.status})`);
	}
	return body as T;
}

function emit(name: string, detail?: unknown) {
	window.dispatchEvent(new CustomEvent(name, { detail }));
}

function toast(message: string, kind: "success" | "error" = "success") {
	emit("trace:toast", { message, kind });
}

function slug(...parts: Array<string | null | undefined>) {
	return (
		parts
			.filter(Boolean)
			.join(" ")
			.normalize("NFKD")
			.toLocaleLowerCase()
			.replace(/[^a-z0-9]+/g, "-")
			.replace(/^-|-$/g, "")
			.slice(0, 80)
			.replace(/-$/, "") || "session"
	);
}

function shareUrl(snapshot: ShareSnapshot) {
	if (snapshot.shareUrl) return snapshot.shareUrl;
	const shareId = snapshot.summary.share_id;
	if (!shareId) return "";
	const identity = snapshot.summary.driver_name || snapshot.summary.title || "session";
	return new URL(`/shared/${slug(identity, snapshot.summary.track)}--${shareId}`, window.location.origin).href;
}

function SharePanel() {
	const [snapshot, setSnapshot] = useState<ShareSnapshot | null>(window.traceShareSnapshot || null);
	const [busy, setBusy] = useState(false);
	const [confirming, setConfirming] = useState(false);
	const input = useRef<HTMLInputElement>(null);
	const timer = useRef<number | null>(null);

	useEffect(() => {
		const receive = (event: Event) => {
			setSnapshot((event as CustomEvent<ShareSnapshot | null>).detail);
			setConfirming(false);
		};
		window.addEventListener("trace:share-load", receive);
		return () => window.removeEventListener("trace:share-load", receive);
	}, []);

	useEffect(() => {
		const open = () => void toggle();
		window.addEventListener("trace:share-open", open);
		return () => window.removeEventListener("trace:share-open", open);
	});

	useEffect(
		() => () => {
			if (timer.current) window.clearTimeout(timer.current);
		},
		[],
	);

	async function toggle() {
		if (!snapshot || busy) return;
		if (snapshot.summary.share_id) {
			const open = !snapshot.open;
			setSnapshot({ ...snapshot, open });
			emit("trace:share-visibility", { open });
			return;
		}
		setBusy(true);
		try {
			const created = await request<ShareResponse>(`/api/v1/library/sessions/${encodeURIComponent(snapshot.summary.session_id)}/share`, {
				method: "POST",
			});
			emit("trace:share-updated", { shareId: created.share_id, shareUrl: created.share_url, open: true });
			toast("Unlisted session link created.");
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not create the share link.", "error");
		} finally {
			setBusy(false);
		}
	}

	async function copy() {
		if (!snapshot) return;
		const value = shareUrl(snapshot);
		try {
			await navigator.clipboard.writeText(value);
			toast("Share link copied.");
		} catch {
			input.current?.focus();
			input.current?.select();
			const copied = document.execCommand("copy");
			toast(copied ? "Share link copied." : "The link is selected. Press Ctrl+C to copy it.", copied ? "success" : "error");
		}
	}

	async function revoke() {
		if (!snapshot) return;
		if (!confirming) {
			setConfirming(true);
			if (timer.current) window.clearTimeout(timer.current);
			timer.current = window.setTimeout(() => setConfirming(false), 3000);
			return;
		}
		setBusy(true);
		try {
			await request<void>(`/api/v1/library/sessions/${encodeURIComponent(snapshot.summary.session_id)}/share`, { method: "DELETE" });
			emit("trace:share-updated", { shareId: null, shareUrl: null, open: false });
			toast("Public access revoked.");
		} catch (error) {
			toast(error instanceof Error ? error.message : "Could not revoke public access.", "error");
		} finally {
			setBusy(false);
			setConfirming(false);
		}
	}

	if (!snapshot?.open || !snapshot.summary.share_id) return null;
	return (
		<section className="share-panel">
			<div>
				<p className="eyebrow">UNLISTED LINK</p>
				<strong>Anyone with this link can review and download the session.</strong>
				<small>The link remains active until you revoke it or delete the session.</small>
			</div>
			<div className="share-link-row">
				<input ref={input} type="text" readOnly value={shareUrl(snapshot)} onClick={(event) => event.currentTarget.select()} />
				<button type="button" disabled={busy} onClick={() => void copy()}>
					Copy link
				</button>
				<button className="danger" type="button" disabled={busy} onClick={() => void revoke()}>
					{confirming ? "Confirm revoke" : "Revoke"}
				</button>
			</div>
		</section>
	);
}

const root = document.querySelector("#share-panel-root");
if (root) createRoot(root).render(<SharePanel />);

export { SharePanel };
