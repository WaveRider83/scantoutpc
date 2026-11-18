const text = document.querySelector<HTMLTextAreaElement>("#ocr-text")!;
const statusEl = document.querySelector<HTMLSpanElement>("#statusEl")!;
document.querySelector("#btn-copy")!.addEventListener("click", async () => {
  await navigator.clipboard.writeText(text.value);
  statusEl.textContent = "copié ✓";
});
document.querySelector("#btn-translate")!.addEventListener("click", () => {
  statusEl.textContent = "traduction pas encore branchée";
});
