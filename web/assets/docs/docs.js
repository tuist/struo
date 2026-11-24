import "phoenix_html";
import { Socket } from "phoenix";
import { LiveSocket } from "phoenix_live_view";
import topbar from "../vendor/topbar.js";
import Noora from "noora";
import DocsContentHook from "./hooks/docs-content-hook.js";
import DocsSearchHook from "./hooks/docs-search-hook.js";
import { setupCopyPage } from "./copy-page.js";

import "./docs.css";

const csrfToken = document.querySelector("meta[name='csrf-token']").getAttribute("content");

const liveSocket = new LiveSocket("/live", Socket, {
  longPollFallbackMs: 2500,
  params: { _csrf_token: csrfToken },
  hooks: {
    ...Noora.Hooks,
    DocsContent: DocsContentHook,
    DocsSearch: DocsSearchHook,
  },
});

// Noora's orange 400, the brand accent, in sRGB.
topbar.config({ barColors: { 0: "#e86800" }, shadowColor: "rgba(0, 0, 0, .3)" });

function closeMobileSidebar() {
  document.body.removeAttribute("data-sidebar-open");
  document.getElementById("docs-sidebar")?.removeAttribute("data-mobile-open");
}

window.addEventListener("phx:page-loading-start", () => topbar.show(300));
window.addEventListener("phx:page-loading-stop", (info) => {
  topbar.hide();
  closeMobileSidebar();
  const to = info.detail?.to;
  if (to && info.detail?.kind !== "initial") {
    const destination = new URL(to, window.location.origin);
    if (!destination.hash) requestAnimationFrame(() => window.scrollTo(0, 0));
  }
});

liveSocket.connect();
window.liveSocket = liveSocket;

setupCopyPage();

// The search shortcut is written for the keyboard in use: see search.css.
if (!/Mac|iPhone|iPad/.test(navigator.platform)) document.documentElement.setAttribute("data-control-key", "");

// The stylesheet shows focus rings only after a key press: see docs.css.
// Modifiers don't count, and neither does dismissing something with Escape.
const NOT_NAVIGATION_KEYS = new Set(["Shift", "Control", "Alt", "Meta", "Escape"]);
window.addEventListener("pointerdown", () => document.documentElement.setAttribute("data-pointer", ""), true);
window.addEventListener(
  "keydown",
  ({ key }) => {
    if (!NOT_NAVIGATION_KEYS.has(key)) document.documentElement.removeAttribute("data-pointer");
  },
  true,
);

// Table-of-contents scroll spy: highlights the section being read, which is
// the last heading above a reading line near the top of the window. Close to
// the end of the page the remaining headings can no longer climb that high,
// so the line slides down to the bottom of the window and each of them still
// gets its turn. A heading that was jumped to stays highlighted until the
// reader scrolls by hand. The links and headings are looked up on every pass,
// because a live navigation swaps them without reloading the page.
const TOC_READING_LINE = 0.2;
const TOC_JUMP_SLACK_PX = 8;

function setupTocScrollSpy() {
  const root = document.documentElement;
  const headingId = (hash) => decodeURIComponent(hash.slice(1));
  let pinned = headingId(window.location.hash);
  let frame = 0;

  function reading(headings) {
    // Where a heading comes to rest when it is jumped to, so that it counts
    // as reached once it is there.
    const jump =
      parseFloat(getComputedStyle(root).scrollPaddingTop) +
      parseFloat(getComputedStyle(headings[0]).scrollMarginTop) +
      TOC_JUMP_SLACK_PX;
    const top = Math.max(jump, window.innerHeight * TOC_READING_LINE);
    const left = root.scrollHeight - window.innerHeight - window.scrollY;
    const line = root.scrollHeight > window.innerHeight ? Math.max(top, window.innerHeight - left) : top;
    return headings.findLast((heading) => heading.getBoundingClientRect().top <= line);
  }

  function update() {
    frame = 0;
    const links = [...document.querySelectorAll('#docs-toc [data-part="list"] a')];
    const headings = links.map((link) => document.getElementById(headingId(link.hash))).filter(Boolean);
    if (!headings.length) return;
    const current = headings.find(({ id }) => id === pinned) ?? reading(headings);
    links.forEach((link) => link.toggleAttribute("data-active", headingId(link.hash) === current?.id));
  }

  const request = () => {
    frame ||= requestAnimationFrame(update);
  };
  const pin = (id) => {
    pinned = id;
    request();
  };

  window.addEventListener("scroll", request, { passive: true });
  window.addEventListener("resize", request);
  window.addEventListener("docs:page-shown", () => pin(headingId(window.location.hash)));
  ["wheel", "touchmove", "keydown", "pointerdown"].forEach((type) =>
    window.addEventListener(type, () => pin(""), { capture: true, passive: true }),
  );
  document.addEventListener("click", (event) => {
    const link = event.target.closest?.('a[href^="#"]');
    if (link) pin(headingId(link.hash));
  });
  request();
}

setupTocScrollSpy();
