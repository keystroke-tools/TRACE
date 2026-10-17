import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import type { MotecImportOptions, MotecImportPreview } from "../../data-source";

export function MotecImportModal({
	preview,
	onCancel,
	onImport,
}: {
	preview: MotecImportPreview;
	onCancel: () => void;
	onImport: (options: MotecImportOptions) => Promise<void>;
}) {
	const [options, setOptions] = useState<MotecImportOptions>(preview);
	const [saving, setSaving] = useState(false);

	useEffect(() => {
		function closeOnEscape(event: KeyboardEvent) {
			if (event.key === "Escape" && !saving) onCancel();
		}
		document.addEventListener("keydown", closeOnEscape);
		return () => document.removeEventListener("keydown", closeOnEscape);
	}, [onCancel, saving]);

	async function submit(event: FormEvent) {
		event.preventDefault();
		setSaving(true);
		try {
			await onImport({
				...options,
				track: options.track.trim(),
				car: options.car.trim(),
				driver: options.driver.trim(),
				title: options.title.trim(),
			});
		} finally {
			setSaving(false);
		}
	}

	return (
		<div className="fixed inset-0 z-[100] grid place-items-center bg-black/80 p-6" role="presentation">
			<form
				onSubmit={(event) => void submit(event)}
				className="w-full max-w-[720px] border border-trace-divider bg-trace-deep"
				role="dialog"
				aria-modal="true"
			>
				<header className="flex items-start justify-between gap-6 border-b border-trace-divider px-7 py-6">
					<div>
						<span className="font-mono text-[11px] font-bold tracking-[.14em] text-trace-accent">REVIEW MOTEC IMPORT</span>
						<h2 className="mt-2 text-xl font-black tracking-[-.02em]">Confirm session details</h2>
						<p className="mt-2 text-[13px] leading-5 text-trace-muted">
							TRACE found {preview.completeLapCount} complete {preview.completeLapCount === 1 ? "lap" : "laps"} and{" "}
							{preview.sampleCount.toLocaleString()} samples. Check the inferred details before importing.
						</p>
					</div>
					<button
						type="button"
						disabled={saving}
						onClick={onCancel}
						aria-label="Cancel import"
						className="grid size-9 shrink-0 place-items-center border border-trace-divider bg-trace-surface text-lg text-trace-muted hover:border-trace-soft hover:text-white"
					>
						×
					</button>
				</header>

				<div className="grid grid-cols-2 gap-x-5 gap-y-4 px-7 py-6">
					<Field label="SIMULATOR">
						<select
							value={options.simulatorKey}
							onChange={(event) => setOptions({ ...options, simulatorKey: event.target.value })}
							className="trace-select h-11 w-full border border-trace-divider bg-trace-surface px-3 text-[13px] text-trace-text outline-none focus:border-trace-accent"
						>
							<option value="assetto-corsa">Assetto Corsa</option>
							<option value="le-mans-ultimate">Le Mans Ultimate</option>
							<option value="unknown-simulator">Other / unknown</option>
						</select>
					</Field>
					<Field label="SESSION TYPE">
						<select
							value={options.sessionType}
							onChange={(event) => setOptions({ ...options, sessionType: event.target.value })}
							className="trace-select h-11 w-full border border-trace-divider bg-trace-surface px-3 text-[13px] text-trace-text outline-none focus:border-trace-accent"
						>
							<option value="">Unknown</option>
							<option value="Practice">Practice</option>
							<option value="Qualifying">Qualifying</option>
							<option value="Race">Race</option>
							<option value="Hotlap">Hotlap</option>
						</select>
					</Field>
					<TextField
						label="TRACK"
						required
						value={options.track}
						placeholder="Circuit or layout"
						onChange={(track) => setOptions({ ...options, track })}
					/>
					<TextField label="CAR" required value={options.car} placeholder="Car or class" onChange={(car) => setOptions({ ...options, car })} />
					<TextField label="DRIVER" value={options.driver} placeholder="Optional" onChange={(driver) => setOptions({ ...options, driver })} />
					<TextField label="SESSION NAME" value={options.title} placeholder="Optional" onChange={(title) => setOptions({ ...options, title })} />
				</div>

				<footer className="flex items-center justify-between gap-5 border-t border-trace-divider px-7 py-4">
					<p className="max-w-md text-[11px] leading-4 text-trace-dim">
						These details identify compatible laps for maps and comparisons. Raw MoTeC channels remain available after import.
					</p>
					<div className="flex shrink-0 gap-3">
						<button
							type="button"
							disabled={saving}
							onClick={onCancel}
							className="h-10 border border-trace-divider px-4 font-mono text-[11px] font-bold tracking-[.08em] text-trace-muted hover:text-white"
						>
							CANCEL
						</button>
						<button
							type="submit"
							disabled={saving || !options.track.trim() || !options.car.trim()}
							className="h-10 border border-trace-accent/60 bg-trace-accent-wash px-5 font-mono text-[11px] font-black tracking-[.1em] text-trace-accent hover:bg-trace-accent hover:text-trace-black disabled:border-trace-divider disabled:bg-transparent disabled:text-trace-dim"
						>
							{saving ? "IMPORTING…" : "IMPORT SESSION"}
						</button>
					</div>
				</footer>
			</form>
		</div>
	);
}

function Field({ label, children }: { label: string; children: ReactNode }) {
	return (
		<label className="block">
			<span className="mb-2 block font-mono text-[10px] font-bold tracking-[.12em] text-trace-muted">{label}</span>
			{children}
		</label>
	);
}

function TextField({
	label,
	value,
	placeholder,
	required = false,
	onChange,
}: {
	label: string;
	value: string;
	placeholder: string;
	required?: boolean;
	onChange: (value: string) => void;
}) {
	return (
		<Field label={label}>
			<input
				required={required}
				value={value}
				placeholder={placeholder}
				onChange={(event) => onChange(event.target.value)}
				className="h-11 w-full border border-trace-divider bg-trace-surface px-3 text-[13px] text-trace-text outline-none placeholder:text-trace-dim focus:border-trace-accent"
			/>
		</Field>
	);
}
