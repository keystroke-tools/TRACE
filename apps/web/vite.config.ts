import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
	plugins: [tailwindcss()],
	build: {
		rollupOptions: {
			input: {
				main: "index.html",
				docs: "docs/index.html",
				faq: "faq/index.html",
			},
		},
	},
	server: {
		port: 1421,
		strictPort: true,
	},
});
