import { copyTextToClipboard } from "./shared/clipboard.js";

const FEEDBACK_DURATION_MS = 3000;
const timers = new WeakMap();

// The split button's label reads "Copied" for a moment after a copy, and its
// icon turns into a check: see toc.css. The button keeps its width meanwhile,
// so the chevron stays under the pointer.
function flashCopied(menu) {
  const button = menu?.querySelector('[data-part="main-button"]');
  const label = button?.querySelector('[data-part="head"]');
  if (!label) return;

  if (!timers.has(label)) {
    label.dataset.defaultLabel = label.textContent;
    button.style.minWidth = `${button.getBoundingClientRect().width}px`;
  }
  label.textContent = "Copied";
  button.setAttribute("data-copied", "");

  clearTimeout(timers.get(label));
  timers.set(
    label,
    setTimeout(() => {
      label.textContent = label.dataset.defaultLabel;
      button.removeAttribute("data-copied");
      button.style.minWidth = "";
      timers.delete(label);
    }, FEEDBACK_DURATION_MS),
  );
}

// The page's markdown is already in the document, so the copy happens inside
// the click and the browser allows it.
export function setupCopyPage() {
  window.addEventListener("docs:copy-page", ({ target }) => {
    const markdown = document.getElementById("docs-page-markdown")?.value;
    if (!markdown) return;

    copyTextToClipboard(markdown)
      .then(() => flashCopied(target.closest(".noora-menu")))
      .catch((error) => console.error("Failed to copy page:", error));
  });
}
