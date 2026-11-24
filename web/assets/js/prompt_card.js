import { copyTextToClipboard } from "../docs/shared/clipboard.js";
import { sweepCopyScan } from "../docs/shared/hooks/code-copy.js";

// How long the copy button shows that the prompt was copied.
const COPIED_MS = 2000;

function setupCopy(card, button) {
  const source = card.querySelector('[data-part="copy-source"]');
  if (!button || !source) return;

  const idleLabel = button.getAttribute("aria-label");
  let timer = null;

  button.addEventListener("click", () => {
    copyTextToClipboard(source.content.textContent)
      .then(() => {
        sweepCopyScan(card);
        button.setAttribute("data-copied", "");
        button.setAttribute("aria-label", "Copied");
        clearTimeout(timer);
        timer = setTimeout(() => {
          button.removeAttribute("data-copied");
          button.setAttribute("aria-label", idleLabel);
        }, COPIED_MS);
      })
      .catch((error) => console.error("Failed to copy the prompt:", error));
  });
}

function setupToggle(card, button) {
  const label = button?.querySelector('[data-part="label"]');
  if (!button || !label) return;

  button.addEventListener("click", () => {
    const expanded = card.toggleAttribute("data-expanded");
    button.setAttribute("aria-expanded", String(expanded));
    label.textContent = expanded ? "Show less" : "Show more";
  });
}

// Wires each prompt card: the button that copies the whole prompt, and the
// toggle that unfolds the part of it the card keeps folded.
export function setupPromptCard(root = document) {
  root.querySelectorAll('[data-part="prompt-card"]').forEach((card) => {
    setupCopy(card, card.querySelector('[data-part="copy"]'));
    setupToggle(card, card.querySelector('[data-part="prompt-toggle"]'));
  });
}
