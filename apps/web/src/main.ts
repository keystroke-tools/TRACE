import "./styles.css";

document.querySelector<HTMLSpanElement>("#year")!.textContent = String(new Date().getFullYear());

const dialog = document.querySelector<HTMLDialogElement>("#screenshot-dialog");
const dialogImage = dialog?.querySelector<HTMLImageElement>("img");

document.querySelectorAll<HTMLButtonElement>("[data-shot]").forEach((button) => {
  button.addEventListener("click", () => {
    if (!dialog || !dialogImage) return;
    dialogImage.src = button.dataset.shot ?? "";
    dialog.showModal();
  });
});

dialog?.querySelector<HTMLButtonElement>("button")?.addEventListener("click", () => dialog.close());
dialog?.addEventListener("click", (event) => {
  if (event.target === dialog) dialog.close();
});

if ("IntersectionObserver" in window && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
  const observer = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.add("is-visible");
      observer.unobserve(entry.target);
    });
  }, { threshold: 0.12 });
  document.querySelectorAll(".reveal").forEach((element) => observer.observe(element));
} else {
  document.querySelectorAll(".reveal").forEach((element) => element.classList.add("is-visible"));
}
