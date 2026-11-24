import { copyTextToClipboard } from "../clipboard.js";

// Sweeps one scanline down the code in the box it was copied from, below the
// box's bar. Removing the attribute and reading the layout first restarts a
// sweep still in flight.
function sweepCopyScan(box) {
  if (!box) return;
  const bar = box.querySelector(':scope > :is([data-part="bar"], [data-part="header"], [data-part="prompt-bar"])');
  box.removeAttribute("data-copy-scan");
  box.style.setProperty("--scan-top", `${bar?.offsetHeight ?? 0}px`);
  void box.offsetWidth;
  box.setAttribute("data-copy-scan", "");
  box.onanimationend = (event) => {
    if (event.animationName === "copy-scan") box.removeAttribute("data-copy-scan");
  };
}

function flashCopyCheck(button) {
  sweepCopyScan(button.closest('[data-part="code-window"], .code-group'));
  button.setAttribute("data-copied", "");
  button.setAttribute("aria-label", "Copied");
  // A second click restarts the confirmation instead of letting the first
  // click's timer end it early.
  clearTimeout(button.copyResetTimer);
  button.copyResetTimer = setTimeout(() => {
    button.removeAttribute("data-copied");
    button.setAttribute("aria-label", "Copy code");
  }, 2000);
}

function copySourceText(container) {
  return container?.querySelector('[data-part="copy-source"]')?.content?.textContent;
}

function setupCodeCopy(el) {
  el.querySelectorAll('[data-part="code-window"] [data-part="copy"]').forEach((button) => {
    button.setAttribute("role", "button");
    button.setAttribute("tabindex", "0");
    button.setAttribute("aria-label", "Copy code");

    button.addEventListener("click", () => {
      const codeWindow = button.closest('[data-part="code-window"]');
      const codeBlock = codeWindow?.querySelector('[data-part="code"]');
      if (!codeBlock) return;
      copyTextToClipboard(copySourceText(codeWindow) ?? codeBlock.textContent.trim())
        .then(() => flashCopyCheck(button))
        .catch((err) => console.error("Failed to copy code:", err));
    });

    button.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        button.click();
      }
    });
  });
}

const CodeCopy = {
  mounted() {
    setupCodeCopy(this.el);
  },
  updated() {
    setupCodeCopy(this.el);
  },
};

export { CodeCopy, setupCodeCopy, flashCopyCheck, copySourceText, sweepCopyScan };
