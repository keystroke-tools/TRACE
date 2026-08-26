import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import desktopPackage from "../../../package.json";
import { useToast } from "../../Toast";

export type UpdatePhase = "idle" | "checking" | "upToDate" | "available" | "downloading" | "installing" | "restarting" | "failed";

interface UpdateContextValue {
	availableVersion: string | null;
	checkForUpdates: (announceResult?: boolean) => Promise<void>;
	currentVersion: string;
	dismissNotice: () => void;
	downloadProgress: number | null;
	error: string | null;
	installUpdate: () => Promise<void>;
	noticeVisible: boolean;
	phase: UpdatePhase;
	supported: boolean;
}

const UpdateContext = createContext<UpdateContextValue | null>(null);

function errorMessage(error: unknown) {
	return error instanceof Error ? error.message : String(error);
}

function displayVersion(version: string) {
	return `v${version.replace(/^v/i, "")}`;
}

export function UpdateProvider({ children }: { children: ReactNode }) {
	const showToast = useToast();
	const supported = isTauri();
	const [update, setUpdate] = useState<Update | null>(null);
	const [currentVersion, setCurrentVersion] = useState(displayVersion(desktopPackage.version));
	const updateRef = useRef<Update | null>(null);
	const checkRef = useRef<Promise<void> | null>(null);
	const [phase, setPhase] = useState<UpdatePhase>("idle");
	const [downloadedBytes, setDownloadedBytes] = useState(0);
	const [totalBytes, setTotalBytes] = useState<number | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [noticeVisible, setNoticeVisible] = useState(false);

	const checkForUpdates = useCallback(
		(announceResult = false) => {
			if (checkRef.current) return checkRef.current;
			const request = (async () => {
				if (!supported) {
					if (announceResult) {
						showToast({
							kind: "info",
							title: "Desktop app required",
							message: "Update checks are available in an installed TRACE desktop build.",
						});
					}
					return;
				}
				setPhase("checking");
				setError(null);
				try {
					const available = await check({ timeout: 15_000 });
					if (updateRef.current && updateRef.current !== available) await updateRef.current.close();
					updateRef.current = available;
					setUpdate(available);
					if (available) {
						setPhase("available");
						setNoticeVisible(true);
						if (announceResult) {
							showToast({
								kind: "info",
								title: `${displayVersion(available.version)} is available`,
								message: "Install it from Settings or the update card in the sidebar.",
							});
						}
					} else {
						setPhase("upToDate");
						setNoticeVisible(false);
						if (announceResult) {
							showToast({ kind: "success", title: "TRACE is up to date", message: "You already have the latest available version." });
						}
					}
				} catch (caught) {
					const message = errorMessage(caught);
					setError(message);
					setPhase("failed");
					if (announceResult) showToast({ kind: "error", title: "Update check failed", message, timeoutMs: 8_000 });
					else console.warn("TRACE update check failed", caught);
				}
			})();
			checkRef.current = request;
			void request.finally(() => {
				checkRef.current = null;
			});
			return request;
		},
		[showToast, supported],
	);

	useEffect(() => {
		if (!supported) return;
		void getVersion()
			.then((version) => setCurrentVersion(displayVersion(version)))
			.catch((caught: unknown) => console.warn("TRACE could not read the installed version", caught));
	}, [supported]);

	useEffect(() => {
		if (!supported) return;
		const timer = window.setTimeout(() => void checkForUpdates(false), 2_500);
		return () => window.clearTimeout(timer);
	}, [checkForUpdates, supported]);

	const installUpdate = useCallback(async () => {
		const available = updateRef.current;
		if (!available || phase === "downloading" || phase === "installing" || phase === "restarting") return;
		setPhase("downloading");
		setError(null);
		setDownloadedBytes(0);
		setTotalBytes(null);
		try {
			await available.downloadAndInstall((event: DownloadEvent) => {
				switch (event.event) {
					case "Started":
						setTotalBytes(event.data.contentLength ?? null);
						break;
					case "Progress":
						setDownloadedBytes((current) => current + event.data.chunkLength);
						break;
					case "Finished":
						setPhase("installing");
						break;
				}
			});
			setPhase("restarting");
			await relaunch();
		} catch (caught) {
			const message = errorMessage(caught);
			setError(message);
			setPhase("failed");
			setNoticeVisible(true);
			showToast({ kind: "error", title: "Update installation failed", message, timeoutMs: 8_000 });
		}
	}, [phase, showToast]);

	const downloadProgress = totalBytes && totalBytes > 0 ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : null;

	return (
		<UpdateContext.Provider
			value={{
				availableVersion: update ? displayVersion(update.version) : null,
				checkForUpdates,
				currentVersion,
				dismissNotice: () => setNoticeVisible(false),
				downloadProgress,
				error,
				installUpdate,
				noticeVisible,
				phase,
				supported,
			}}
		>
			{children}
		</UpdateContext.Provider>
	);
}

export function useUpdater() {
	const updater = useContext(UpdateContext);
	if (!updater) throw new Error("useUpdater must be used inside UpdateProvider");
	return updater;
}
