import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ToastProvider } from "./Toast";
import { PedalOverlay } from "./features/telemetry/PedalOverlay";
import { UpdateProvider } from "./features/update/UpdateContext";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("TRACE root element is unavailable");

const pedalOverlay = new URLSearchParams(window.location.search).get("view") === "pedals";
if (pedalOverlay) document.documentElement.classList.add("pedal-overlay-root");
else document.addEventListener("contextmenu", (event) => event.preventDefault());

createRoot(root).render(
	<StrictMode>
		{pedalOverlay ? (
			<PedalOverlay />
		) : (
			<ToastProvider>
				<UpdateProvider>
					<App />
				</UpdateProvider>
			</ToastProvider>
		)}
	</StrictMode>,
);
