import { UpdateNotice } from "../features/update/UpdateNotice";

const navigation = ["LIVE", "SESSIONS", "COMPARE", "OVERLAYS", "SETUPS", "SETTINGS"] as const;
export type Section = (typeof navigation)[number];

export function Navigation({ active, onChange }: { active: Section; onChange: (section: Section) => void }) {
	return (
		<aside className="flex min-h-0 flex-col overflow-hidden border-r border-trace-divider bg-trace-surface" aria-label="Primary navigation">
			<nav className="min-h-0 flex-1 overflow-y-auto pt-4">
				{navigation.map((item, index) => (
					<button
						key={item}
						type="button"
						onClick={() => onChange(item)}
						className={`flex h-[52px] w-full items-center gap-3 border-0 border-l-[3px] px-4 text-left text-xs font-bold tracking-[.1em] transition-colors ${
							item === active
								? "border-trace-accent bg-trace-accent-wash text-white"
								: "border-transparent bg-transparent text-trace-muted hover:bg-trace-raised hover:text-trace-text"
						}`}
					>
						<span className="font-mono text-[12px] text-trace-dim">0{index + 1}</span>
						<span className="min-w-0 truncate">{item}</span>
					</button>
				))}
			</nav>
			<UpdateNotice />
		</aside>
	);
}
