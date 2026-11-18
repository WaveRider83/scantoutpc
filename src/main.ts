import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const label = getCurrentWindow().label;

if (label === "overlay") {
  initOverlay();
} else {
  initMain();
}

function initMain() {
  const text = document.querySelector<HTMLTextAreaElement>("#ocr-text")!;
  const statusEl = document.querySelector<HTMLSpanElement>("#status")!;
  document.querySelector("#btn-copy")!.addEventListener("click", async () => {
    await navigator.clipboard.writeText(text.value);
    statusEl.textContent = "copié ✓";
  });
  document.querySelector("#btn-translate")!.addEventListener("click", () => {
    statusEl.textContent = "traduction pas encore branchée";
  });
  listen<{ x: number; y: number; width: number; height: number }>("zone-set", (e) => {
    const z = e.payload;
    statusEl.textContent = `zone : ${z.width}×${z.height} @ ${z.x},${z.y}`;
  });
}

function initOverlay() {
  document.body.innerHTML = `<div id="overlay-root"><div id="rubber"></div><div id="hint">Glissez pour définir la zone — Échap pour annuler</div></div>`;
  const root = document.getElementById("overlay-root")!;
  const rubber = document.getElementById("rubber")!;
  let startX = 0, startY = 0, dragging = false;

  root.addEventListener("mousedown", (e) => {
    dragging = true; startX = e.clientX; startY = e.clientY;
    rubber.style.display = "block";
    moveRubber(e);
  });
  root.addEventListener("mousemove", (e) => { if (dragging) moveRubber(e); });
  root.addEventListener("mouseup", async (e) => {
    if (!dragging) return;
    dragging = false;
    const x = Math.round(Math.min(startX, e.clientX));
    const y = Math.round(Math.min(startY, e.clientY));
    const width = Math.round(Math.abs(e.clientX - startX));
    const height = Math.round(Math.abs(e.clientY - startY));
    if (width >= 20 && height >= 20) {
      await invoke("set_zone", { x, y, width, height });
    } else {
      await getCurrentWindow().hide();
    }
  });
  window.addEventListener("keydown", async (e) => {
    if (e.key === "Escape") await getCurrentWindow().hide();
  });

  function moveRubber(e: MouseEvent) {
    const x = Math.min(startX, e.clientX), y = Math.min(startY, e.clientY);
    rubber.style.left = x + "px";
    rubber.style.top = y + "px";
    rubber.style.width = Math.abs(e.clientX - startX) + "px";
    rubber.style.height = Math.abs(e.clientY - startY) + "px";
  }
}
