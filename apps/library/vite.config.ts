import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
	plugins: [react(), tailwindcss()],
	resolve: {
		alias: [
			{ find: "react-dom/client", replacement: fileURLToPath(import.meta.resolve("preact/compat/client")) },
			{ find: "react-dom/test-utils", replacement: fileURLToPath(import.meta.resolve("preact/test-utils")) },
			{ find: "react-dom", replacement: fileURLToPath(import.meta.resolve("preact/compat")) },
			{ find: "react/jsx-dev-runtime", replacement: fileURLToPath(import.meta.resolve("preact/jsx-dev-runtime")) },
			{ find: "react/jsx-runtime", replacement: fileURLToPath(import.meta.resolve("preact/jsx-runtime")) },
			{ find: "react", replacement: fileURLToPath(import.meta.resolve("preact/compat")) },
		],
	},
	build: {
		lib: {
			entry: resolve(import.meta.dirname, "src/admin.tsx"),
			formats: ["iife"],
			name: "TraceLibraryAdmin",
			fileName: () => "library-admin.js",
		},
		outDir: resolve(import.meta.dirname, "../../crates/trace-server/assets/library"),
		emptyOutDir: false,
		rollupOptions: {
			output: {
				assetFileNames: "library-admin.[ext]",
			},
		},
	},
});
