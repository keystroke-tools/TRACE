import { useId, type ReactNode } from "react";

export interface SwitchProps {
	checked: boolean;
	onCheckedChange: (checked: boolean) => void;
	label: string;
	disabled?: boolean;
	labelledBy?: string;
	describedBy?: string;
	className?: string;
}

export function Switch({ checked, onCheckedChange, label, disabled = false, labelledBy, describedBy, className = "" }: SwitchProps) {
	return (
		<button
			type="button"
			role="switch"
			aria-checked={checked}
			aria-label={labelledBy ? undefined : label}
			aria-labelledby={labelledBy}
			aria-describedby={describedBy}
			disabled={disabled}
			onClick={() => onCheckedChange(!checked)}
			className={`relative h-7 w-12 shrink-0 overflow-hidden border transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-trace-accent disabled:cursor-not-allowed disabled:opacity-50 ${
				checked ? "border-trace-accent bg-trace-accent" : "border-trace-divider bg-trace-deep"
			} ${className}`}
		>
			<span
				className={`pointer-events-none absolute left-[3px] top-[3px] size-5 bg-trace-black transition-transform ${checked ? "translate-x-5" : "translate-x-0"}`}
				aria-hidden="true"
			/>
		</button>
	);
}

export interface SettingSwitchProps extends Omit<SwitchProps, "label" | "labelledBy" | "describedBy"> {
	title: string;
	description: ReactNode;
	className?: string;
	titleClassName?: string;
	descriptionClassName?: string;
}

export function SettingSwitch({
	title,
	description,
	checked,
	onCheckedChange,
	disabled = false,
	className = "",
	titleClassName = "text-[12px]",
	descriptionClassName = "text-[11px]",
}: SettingSwitchProps) {
	const id = useId();
	const titleId = `${id}-title`;
	const descriptionId = `${id}-description`;
	return (
		<label className={`flex items-start justify-between gap-6 ${disabled ? "cursor-not-allowed" : "cursor-pointer"} ${className}`}>
			<span className="min-w-0">
				<strong id={titleId} className={`block text-trace-text ${titleClassName}`}>
					{title}
				</strong>
				<span id={descriptionId} className={`mt-1 block max-w-3xl leading-5 text-trace-dim ${descriptionClassName}`}>
					{description}
				</span>
			</span>
			<Switch
				checked={checked}
				onCheckedChange={onCheckedChange}
				disabled={disabled}
				label={title}
				labelledBy={titleId}
				describedBy={descriptionId}
				className="mt-0.5"
			/>
		</label>
	);
}
