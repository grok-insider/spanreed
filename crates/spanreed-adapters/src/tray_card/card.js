const post = (msg) => {
  try { window.ipc.postMessage(msg); } catch (e) {}
};
document.querySelectorAll("[data-tab]").forEach((button) => {
  button.addEventListener("click", () => {
    const id = button.dataset.tab;
    document.querySelectorAll("[data-tab]").forEach((node) => node.classList.toggle("on", node === button));
    document.querySelectorAll("[data-panel]").forEach((node) => node.classList.toggle("on", node.dataset.panel === id));
  });
});
const applyRange = (hours) => {
  const max = Number(hours) * 3600000;
  let shown = 0;
  document.querySelectorAll("[data-reset-ms]").forEach((node) => {
    const ms = Number(node.dataset.resetMs);
    const visible = ms > 0 && ms <= max;
    node.hidden = !visible;
    if (visible) {
      shown += 1;
      node.style.left = Math.min(96, Math.max(4, ms / max * 100)) + "%";
    }
  });
  const empty = document.querySelector(".none");
  if (empty) empty.hidden = shown > 0;
};
document.querySelectorAll("[data-range]").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll("[data-range]").forEach((node) => node.classList.toggle("on", node === button));
    applyRange(button.dataset.range);
  });
});
applyRange(168);
document.querySelectorAll("[data-act]").forEach((button) => {
  button.addEventListener("click", () => post(button.dataset.act));
});
document.querySelectorAll("[data-buy]").forEach((button) => {
  button.addEventListener("click", () => post("buy " + button.dataset.buy));
});
const resetDialog = document.querySelector(".dialog");
const resetBody = document.querySelector("#reset-body");
const resetCancel = document.querySelector("[data-reset-cancel]");
const resetConfirm = document.querySelector("[data-reset-confirm]");
let resetPending = false;
document.querySelectorAll("[data-reset]").forEach((button) => {
  button.addEventListener("click", () => {
    if (resetPending) return;
    const account = button.dataset.reset || "this account";
    resetBody.textContent = "This spends one limit reset credit for " + account + " and asks Codex to clear the current rate-limit windows. This cannot be undone.";
    resetDialog.hidden = false;
  });
});
resetCancel.addEventListener("click", () => {
  if (resetPending) return;
  resetDialog.hidden = true;
});
resetConfirm.addEventListener("click", () => {
  if (resetPending) return;
  resetPending = true;
  resetConfirm.textContent = "Using reset…";
  resetConfirm.disabled = true;
  resetCancel.disabled = true;
  post("reset");
});
document.querySelectorAll(".bars i").forEach((bar) => {
  bar.addEventListener("mouseenter", () => {
    const note = bar.closest(".panel").querySelector(".hover");
    if (note) note.textContent = bar.dataset.detail || "";
  });
});
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!resetDialog.hidden && !resetPending) {
    resetDialog.hidden = true;
    return;
  }
  post("close");
});
