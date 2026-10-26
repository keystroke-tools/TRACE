import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const output = resolve(import.meta.dirname, "../../../crates/trace-server/assets/library/library-admin.css");
const css = await readFile(output, "utf8");
const required = [
	".trace-seek::-webkit-slider-runnable-track",
	".trace-seek::-webkit-slider-thumb",
	".trace-seek::-moz-range-track",
	".trace-seek::-moz-range-thumb",
	".trace-chart__header",
	".trace-chart__plot",
	".trace-chart__tooltip",
	".trace-session-title",
];
const missing = required.filter((selector) => !css.includes(selector));

if (missing.length) {
	throw new Error(`The web library stylesheet is missing shared UI selectors: ${missing.join(", ")}`);
}

console.log("Verified shared UI theme selectors in the web library stylesheet.");
