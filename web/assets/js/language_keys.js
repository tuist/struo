import { playKeyPress, warmAudio } from "./sounds.js";

// The longest a key holds its link back for its sound. The audio engine takes
// a moment to come up the first time, and a page that is already leaving
// never plays the sound at all.
const HOLD_MS = 250;

const opensHere = (event, link) =>
  !(event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) && (!link.target || link.target === "_self");

// Gives the keys of the language board the sound of a key going down. A mouse
// or pen sounds the key as it is pressed, the moment the cap sinks. A finger
// may be starting a scroll, and a keyboard has no press of its own, so those
// sound the key when it is activated. A key that opens its page in this window
// waits for the sound to finish first.
export function setupLanguageKeys(root = document) {
  const board = root.querySelector('[data-part="language-grid"]');
  if (!board) return;

  const keyOf = (event) => event.target.closest('[data-part="language-chip"]');
  // The key the pointer is holding down, and the sound it has already made.
  let held = null;
  let sounding = null;

  ["pointerenter", "pointerdown", "keydown"].forEach((type) => board.addEventListener(type, warmAudio));

  board.addEventListener("pointerdown", (event) => {
    const key = keyOf(event);
    if (!key || event.pointerType === "touch" || event.button !== 0) return;
    held = key;
    sounding = playKeyPress();
  });

  board.addEventListener("click", (event) => {
    const key = keyOf(event);
    if (!key) return;
    const sounded = key === held ? sounding : playKeyPress();
    held = null;
    if (event.defaultPrevented || !opensHere(event, key)) return;
    event.preventDefault();
    const timeout = new Promise((resolve) => setTimeout(resolve, HOLD_MS));
    Promise.race([sounded, timeout]).then(() => window.location.assign(key.href));
  });

  ["pointerleave", "pointercancel"].forEach((type) =>
    board.addEventListener(type, () => {
      held = null;
    }),
  );
}
