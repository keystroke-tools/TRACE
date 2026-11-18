import { useEffect } from "react";
import { CURRENT_CHANGELOG, normalizeVersion } from "./changelog";

export function WhatsNewDialog({ version, onClose }: { version: string; onClose: () => void }) {
	useEffect(() => {
		function closeOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape") onClose();
		}
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);

	return (
		<div className="fixed inset-0 z-[120] grid place-items-center bg-black/80 p-8" role="presentation">
			<section
				role="dialog"
				aria-modal="true"
				aria-labelledby="whats-new-title"
				aria-describedby="whats-new-intro"
				className="w-full max-w-[680px] border border-trace-divider bg-trace-deep"
			>
				<header className="flex items-start justify-between gap-6 border-b border-trace-divider px-7 py-6">
					<div>
						<span className="font-mono text-[11px] font-bold tracking-[.14em] text-trace-accent">TRACE v{normalizeVersion(version)}</span>
						<h2 id="whats-new-title" className="mt-2 text-[26px] font-black leading-tight tracking-[-.03em]">
							Here&apos;s what changed
						</h2>
						<p id="whats-new-intro" className="mt-2 text-[13px] leading-5 text-trace-muted">
							{CURRENT_CHANGELOG.intro}
						</p>
					</div>
					<button
						type="button"
						onClick={onClose}
						aria-label="Close what's new"
						className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-surface text-lg text-trace-muted hover:border-trace-soft hover:text-white"
					>
						×
					</button>
				</header>

				<div className="px-7 py-6">
					<div className="divide-y divide-trace-divider border-y border-trace-divider">
						{CURRENT_CHANGELOG.highlights.map((highlight, index) => (
							<article key={highlight.title} className="grid grid-cols-[38px_1fr] gap-4 py-5">
								<span className="font-mono text-[12px] font-bold text-trace-accent">{String(index + 1).padStart(2, "0")}</span>
								<div>
									<h3 className="text-[15px] font-black leading-5 text-trace-text">{highlight.title}</h3>
									<p className="mt-1.5 text-[13px] leading-5 text-trace-muted">{highlight.description}</p>
								</div>
							</article>
						))}
					</div>
				</div>

				<footer className="flex items-center justify-between gap-5 border-t border-trace-divider bg-trace-surface px-7 py-5">
					<p className="text-[11px] leading-4 text-trace-dim">You can reopen this from Settings → Updates &amp; About.</p>
					<button
						type="button"
						onClick={onClose}
						autoFocus
						className="min-h-10 shrink-0 border border-trace-accent bg-trace-accent px-5 font-mono text-[11px] font-black tracking-[.08em] text-trace-black hover:bg-trace-accent-strong"
					>
						CONTINUE
					</button>
				</footer>
			</section>
		</div>
	);
}
