import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const assetsDirectory = fileURLToPath(new URL("../dist/assets/", import.meta.url));
const stylesheet = (await readdir(assetsDirectory)).find((entry) => entry.startsWith("index-") && entry.endsWith(".css"));

if (!stylesheet) throw new Error("Desktop build did not produce an application stylesheet.");

const css = await readFile(join(assetsDirectory, stylesheet), "utf8");
const requiredSharedUtilities = [
	["comparison HUD height", "height:204px"],
	["comparison HUD columns", "grid-template-columns:120px 72px minmax(180px,320px) minmax(120px,1fr) minmax(180px,320px) 72px 120px"],
	["desktop HUD anchor", "left:calc(var(--trace-sidebar) + 24px)"],
];

const missing = requiredSharedUtilities.filter(([, declaration]) => !css.includes(declaration)).map(([name]) => name);
if (missing.length > 0) {
	throw new Error(`Desktop CSS is missing shared UI utilities (${missing.join(", ")}). Ensure src/styles.css scans packages/trace-ui before releasing.`);
}

console.log("Verified shared UI utilities in the desktop stylesheet.");
