import desktopPackage from "../../package.json";
import type { TelemetryStatus } from "../data-source";

export function Footer({ status }: { status: TelemetryStatus | null }) {
	return (
		<footer className="col-span-full flex items-center gap-6 border-t border-trace-divider bg-trace-black px-[14px] font-mono text-[12px] tracking-[.06em] text-trace-dim">
			<span>
				TRACE ENGINE <b className="ml-1 text-trace-accent">READY</b>
			</span>
			<span>
				{status?.simulatorShortName ?? "SIM"} MODULE <b className="ml-1 text-trace-accent">LIFECYCLE</b>
			</span>
			<span>
				STORAGE <b className="ml-1 text-trace-accent">LOCAL</b>
			</span>
			<span className="ml-auto">V{desktopPackage.version}</span>
		</footer>
	);
}
