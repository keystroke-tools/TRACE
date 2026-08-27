import "./styles.css";

document.querySelector<HTMLSpanElement>("#year")!.textContent = String(new Date().getFullYear());

const dialog = document.querySelector<HTMLDialogElement>("#screenshot-dialog");
const dialogImage = dialog?.querySelector<HTMLImageElement>("img");
let screenshotTrigger: HTMLButtonElement | null = null;

document.querySelectorAll<HTMLButtonElement>("[data-shot]").forEach((button) => {
	button.addEventListener("click", () => {
		if (!dialog || !dialogImage) return;
		screenshotTrigger = button;
		dialogImage.src = button.dataset.shot ?? "";
		dialog.showModal();
	});
});

dialog?.querySelector<HTMLButtonElement>("button")?.addEventListener("click", () => dialog.close());
dialog?.addEventListener("click", (event) => {
	if (event.target === dialog) dialog.close();
});
dialog?.addEventListener("close", () => {
	if (dialogImage) dialogImage.removeAttribute("src");
	screenshotTrigger?.focus();
});

document.querySelectorAll<HTMLButtonElement>("[data-copy-target]").forEach((copyButton) => {
	const target = document.querySelector<HTMLElement>(copyButton.dataset.copyTarget ?? "")?.textContent?.trim();
	copyButton.addEventListener("click", async () => {
		if (!target) return;
		try {
			await navigator.clipboard.writeText(target);
			copyButton.textContent = "COPIED";
			copyButton.dataset.state = "success";
		} catch {
			copyButton.textContent = "SELECT TO COPY";
			copyButton.dataset.state = "error";
		}
		window.setTimeout(() => {
			copyButton.textContent = "COPY";
			delete copyButton.dataset.state;
		}, 2_400);
	});
});

if ("IntersectionObserver" in window && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
	const observer = new IntersectionObserver(
		(entries) => {
			entries.forEach((entry) => {
				if (!entry.isIntersecting) return;
				entry.target.classList.add("is-visible");
				observer.unobserve(entry.target);
			});
		},
		{ threshold: 0.12 },
	);
	document.querySelectorAll(".reveal").forEach((element) => observer.observe(element));
} else {
	document.querySelectorAll(".reveal").forEach((element) => element.classList.add("is-visible"));
}
