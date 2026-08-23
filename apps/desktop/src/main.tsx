import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ToastProvider } from "./Toast";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("TRACE root element is unavailable");

createRoot(root).render(
	<StrictMode>
		<ToastProvider>
			<App />
		</ToastProvider>
	</StrictMode>,
);
