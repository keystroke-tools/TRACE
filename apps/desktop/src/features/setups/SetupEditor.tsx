import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { telemetryDataSource, type SetupDocument, type SetupDocumentValue, type SetupLibraryEntry } from "../../data-source";
import { useToast } from "../../Toast";

interface SetupEditorProps {
	setup: SetupLibraryEntry;
	onClose: () => void;
	onSaved: (setupId: string) => void;
}

export function SetupEditor({ setup, onClose, onSaved }: SetupEditorProps) {
	const showToast = useToast();
	const [document, setDocument] = useState<SetupDocument | null>(null);
	const [state, setState] = useState<"loading" | "ready" | "error">("loading");
	const [error, setError] = useState("");
	const [selectedGroup, setSelectedGroup] = useState("");
	const [values, setValues] = useState<Record<string, string>>({});
	const [linkSides, setLinkSides] = useState(false);
	const [copyName, setCopyName] = useState(() => copyNameFrom(setup.name));
	const [saving, setSaving] = useState(false);

	useEffect(() => {
		let active = true;
		setState("loading");
		setError("");
		void telemetryDataSource
			.getSetupDocument(setup.id)
			.then((nextDocument) => {
				if (!active) return;
				setDocument(nextDocument);
				setSelectedGroup(nextDocument.groups[0]?.name ?? "");
				setValues(
					Object.fromEntries(
						nextDocument.groups
							.flatMap((group) => group.values)
							.filter((value) => value.editable)
							.map((value) => [value.section, value.value]),
					),
				);
				setState("ready");
			})
			.catch((reason) => {
				if (!active) return;
				setError(reason instanceof Error ? reason.message : String(reason));
				setState("error");
			});
		return () => {
			active = false;
		};
	}, [setup.id]);

	const allFields = useMemo(() => document?.groups.flatMap((group) => group.values) ?? [], [document]);
	const group = document?.groups.find((candidate) => candidate.name === selectedGroup) ?? document?.groups[0] ?? null;
	const changed = allFields.filter((field) => field.editable && values[field.section] !== field.value).length;

	function setFieldValue(field: SetupDocumentValue, value: string) {
		setValues((current) => {
			const next = { ...current, [field.section]: value };
			if (linkSides) {
				const partner = sidePartner(field.section);
				if (partner && Object.hasOwn(current, partner)) next[partner] = value;
			}
			return next;
		});
	}

	async function saveCopy() {
		if (!document || !copyName.trim()) return;
		setSaving(true);
		try {
			const result = await telemetryDataSource.saveSetupCopy({
				sourceSetupId: document.setupId,
				name: copyName.trim(),
				values: allFields.filter((field) => field.editable).map((field) => ({ section: field.section, value: values[field.section] ?? field.value })),
			});
			showToast({
				kind: "success",
				title: "Setup copy saved",
				message: `${result.name} was added to the local setup library. The source file was not changed.`,
				timeoutMs: 6_000,
			});
			onSaved(result.setupId);
		} catch (reason) {
			showToast({
				kind: "error",
				title: "Setup copy could not be saved",
				message: reason instanceof Error ? reason.message : String(reason),
				timeoutMs: 8_000,
			});
		} finally {
			setSaving(false);
		}
	}

	return (
		<div className="h-[clamp(500px,68vh,760px)] overflow-hidden bg-trace-black">
			<header className="flex min-h-16 flex-wrap items-center justify-between gap-4 bg-trace-surface px-5 py-3">
				<div className="flex min-w-0 items-center gap-3">
					<button
						type="button"
						onClick={onClose}
						className="grid size-9 shrink-0 place-items-center bg-trace-deep text-[16px] text-trace-soft hover:bg-trace-raised hover:text-white"
						aria-label="Back to setup library"
					>
						‹
					</button>
					<div className="min-w-0">
						<span className="block font-mono text-[8px] font-black tracking-[.12em] text-trace-muted">SETUP EDITOR</span>
						<strong className="block truncate text-[13px] text-white">{setup.name}</strong>
						<span className="block truncate font-mono text-[9px] text-trace-dim">
							{setup.carName} · {setup.trackName}
						</span>
					</div>
				</div>
				<div className="flex min-w-[360px] flex-1 items-center justify-end gap-2">
					<label className="flex h-9 min-w-0 max-w-[320px] flex-1 items-center border border-trace-divider bg-trace-deep focus-within:border-trace-accent">
						<span className="sr-only">Name for setup copy</span>
						<span className="shrink-0 px-3 font-mono text-[8px] font-black tracking-[.1em] text-trace-muted">SAVE AS</span>
						<input
							value={copyName}
							onChange={(event) => setCopyName(event.target.value)}
							className="h-full min-w-0 flex-1 border-l border-trace-divider bg-transparent px-3 text-[11px] text-white outline-none"
							placeholder="New setup name"
						/>
					</label>
					<button
						type="button"
						onClick={() => void saveCopy()}
						disabled={state !== "ready" || saving || !copyName.trim()}
						className="h-9 shrink-0 bg-trace-accent px-4 text-[10px] font-black text-trace-black hover:bg-trace-accent-bright disabled:cursor-not-allowed disabled:opacity-40"
					>
						{saving ? "SAVING…" : "SAVE COPY"}
					</button>
				</div>
			</header>

			{state === "loading" ? (
				<p className="p-8 font-mono text-[11px] text-trace-dim">READING SETUP…</p>
			) : state === "error" ? (
				<div className="p-8">
					<strong className="text-[13px] text-trace-warning">TRACE could not open this setup.</strong>
					<p className="mt-2 text-[11px] leading-5 text-trace-dim">{error}</p>
				</div>
			) : document ? (
				<div className="grid h-[calc(100%-4rem)] min-h-0 grid-cols-[220px_minmax(0,1fr)]">
					<aside className="flex min-h-0 flex-col border-r border-trace-divider bg-trace-deep">
						<div className="px-4 py-3">
							<div className="flex items-center justify-between gap-3">
								<span className="font-mono text-[9px] font-black tracking-[.1em] text-trace-soft">CATEGORIES</span>
								<span className="font-mono text-[8px] text-trace-muted">{document.groups.length}</span>
							</div>
							<p className="mt-2 text-[10px] leading-4 text-trace-dim">
								{document.metadataAvailable
									? "Names, limits, and help are read from Assetto Corsa."
									: "Car metadata is unavailable; raw setup names are shown."}
							</p>
						</div>
						<nav className="min-h-0 flex-1 overflow-y-auto py-1" aria-label="Setup categories">
							{document.groups.map((candidate) => {
								const active = candidate.name === group?.name;
								return (
									<button
										type="button"
										key={candidate.name}
										onClick={() => setSelectedGroup(candidate.name)}
										className={`flex w-full items-center justify-between border-l-2 px-4 py-2.5 text-left ${
											active
												? "border-trace-accent bg-trace-accent/15 text-white"
												: "border-transparent text-trace-soft hover:bg-trace-raised"
										}`}
									>
										<span className="text-[11px] font-bold">{candidate.name}</span>
										<span className="font-mono text-[8px] text-trace-muted">{candidate.values.length}</span>
									</button>
								);
							})}
						</nav>
					</aside>

					<main className="flex min-h-0 min-w-0 flex-col">
						<div className="flex min-h-12 flex-wrap items-center justify-between gap-3 bg-trace-deep px-5 py-2">
							<div>
								<strong className="text-[12px] text-white">{group?.name}</strong>
								<span className="ml-2 font-mono text-[9px] text-trace-muted">
									{changed} CHANGE{changed === 1 ? "" : "S"}
								</span>
							</div>
							<button
								type="button"
								onClick={() => setLinkSides((value) => !value)}
								aria-pressed={linkSides}
								className={`h-8 px-3 font-mono text-[9px] font-black ${
									linkSides ? "bg-trace-accent/15 text-trace-accent" : "bg-trace-surface text-trace-dim hover:text-white"
								}`}
							>
								LINK LEFT / RIGHT {linkSides ? "ON" : "OFF"}
							</button>
						</div>
						<div className="min-h-0 flex-1 overflow-y-auto p-5">
							<div className="grid gap-x-8 gap-y-1 xl:grid-cols-2">
								{group?.values.map((field) => (
									<SetupField
										field={field}
										value={values[field.section] ?? field.value}
										onChange={(value) => setFieldValue(field, value)}
										key={field.section}
									/>
								))}
							</div>
						</div>
						<footer className="flex min-h-11 items-center justify-between gap-4 bg-trace-deep px-5 py-2">
							<p className="text-[10px] leading-4 text-trace-dim">Saving always creates a new file beside the source setup.</p>
							<span className="shrink-0 font-mono text-[9px] font-bold text-trace-soft">SOURCE UNCHANGED</span>
						</footer>
					</main>
				</div>
			) : null}
		</div>
	);
}

function SetupField({ field, value, onChange }: { field: SetupDocumentValue; value: string; onChange: (value: string) => void }) {
	const [helpOpen, setHelpOpen] = useState(false);
	const range = [field.minimum, field.maximum].every((item) => item != null) ? `${field.minimum}–${field.maximum}` : null;
	const slider = sliderBounds(field);
	const numericValue = Number(value);
	const clampedValue = slider && Number.isFinite(numericValue) ? Math.min(slider.maximum, Math.max(slider.minimum, numericValue)) : null;
	const sliderFill = slider && clampedValue != null ? ((clampedValue - slider.minimum) / (slider.maximum - slider.minimum)) * 100 : 0;
	return (
		<article className="min-w-0 border-b border-trace-divider/70 py-3">
			<div className="flex min-w-0 items-start justify-between gap-4">
				<div className="min-w-0 pt-0.5">
					<strong className="block text-[11px] leading-5 text-trace-text">{field.label}</strong>
					<span className="block truncate font-mono text-[8px] leading-4 text-trace-muted">{field.section}</span>
				</div>
				{field.editable ? (
					<input
						value={value}
						onChange={(event) => onChange(event.target.value)}
						className="h-9 w-[104px] shrink-0 border border-trace-divider bg-trace-surface px-3 text-right font-mono text-[11px] font-bold text-white outline-none focus:border-trace-accent"
						aria-label={field.label}
					/>
				) : (
					<output className="min-h-9 max-w-[160px] break-words bg-trace-deep px-3 py-2 text-right font-mono text-[10px] text-trace-soft">
						{field.value}
					</output>
				)}
			</div>
			{field.editable && slider && (
				<div className="mt-2">
					<input
						type="range"
						min={slider.minimum}
						max={slider.maximum}
						step={slider.step ?? "any"}
						value={clampedValue ?? slider.minimum}
						onChange={(event) => onChange(event.target.value)}
						className="trace-seek trace-value-slider w-full"
						style={{ "--trace-slider-fill": `${sliderFill}%` } as CSSProperties}
						aria-label={`Adjust ${field.label}`}
					/>
					<div className="-mt-0.5 flex justify-between font-mono text-[8px] text-trace-muted">
						<span>{field.minimum}</span>
						<span>{field.maximum}</span>
					</div>
				</div>
			)}
			<div className="mt-1 flex min-h-5 flex-wrap items-center gap-x-3 gap-y-1">
				{field.description && (
					<button
						type="button"
						onClick={() => setHelpOpen((open) => !open)}
						aria-expanded={helpOpen}
						className="font-mono text-[8px] font-black tracking-[.08em] text-trace-dim hover:text-trace-accent"
					>
						SETUP GUIDE {helpOpen ? "−" : "+"}
					</button>
				)}
				{range && !slider && (
					<span className="font-mono text-[8px] text-trace-muted">
						AC LIMITS {range}
						{field.step ? ` · STEP ${field.step}` : ""}
					</span>
				)}
				{slider && field.step && <span className="font-mono text-[8px] text-trace-muted">STEP {field.step}</span>}
			</div>
			{helpOpen && field.description && (
				<div className="mt-2 bg-trace-deep px-3 py-2.5">
					<p className="whitespace-pre-line text-[10px] leading-4 text-trace-dim">{field.description}</p>
				</div>
			)}
		</article>
	);
}

function sliderBounds(field: SetupDocumentValue) {
	const minimum = Number(field.minimum);
	const maximum = Number(field.maximum);
	const current = Number(field.value);
	if (!Number.isFinite(minimum) || !Number.isFinite(maximum) || !Number.isFinite(current) || minimum >= maximum || current < minimum || current > maximum)
		return null;
	const step = Number(field.step);
	return { minimum, maximum, step: Number.isFinite(step) && step > 0 ? step : null };
}

function sidePartner(section: string) {
	const pairs: [string, string][] = [
		["_LF", "_RF"],
		["_RF", "_LF"],
		["_LR", "_RR"],
		["_RR", "_LR"],
	];
	const pair = pairs.find(([suffix]) => section.endsWith(suffix));
	return pair ? `${section.slice(0, -pair[0].length)}${pair[1]}` : null;
}

function copyNameFrom(name: string) {
	const base = name.replace(/\.ini$/i, "").trim();
	return `${base || "setup"} copy.ini`;
}
