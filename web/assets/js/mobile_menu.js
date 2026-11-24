// The phone menu of the site header, after the one on the Tuist marketing
// site: the menu button opens a panel that covers the page under the bar.

// The width from which the header shows its links in the bar instead. Matches
// the header's breakpoint in the stylesheet.
const DESKTOP = "(min-width: 681px)";

export function setupMobileMenu(root = document) {
  const header = root.querySelector('[data-part="site-header"]');
  const button = header?.querySelector('[data-part="mobile-menu-button"]');
  const panel = header?.querySelector('[data-part="mobile-menu"]');
  if (!button || !panel) return;

  let open = false;
  let prefetched = false;

  // Fetches the menu's own pages the first time it opens, so a tap lands on a
  // page that is already there. A phone has no hover to start that from.
  // Browsers without speculation rules ignore it.
  function prefetchLinks() {
    if (prefetched) return;
    prefetched = true;
    if (!HTMLScriptElement.supports?.("speculationrules")) return;
    if (navigator.connection?.saveData) return;
    const urls = new Set();
    for (const link of panel.querySelectorAll("a[href]")) {
      if (link.origin !== location.origin) continue;
      if (link.target && link.target !== "_self") continue;
      if (link.pathname === location.pathname) continue;
      urls.add(link.pathname + link.search);
    }
    if (!urls.size) return;
    const script = document.createElement("script");
    script.type = "speculationrules";
    script.textContent = JSON.stringify({ prefetch: [{ urls: [...urls] }] });
    document.head.appendChild(script);
  }

  function setOpen(state) {
    open = state;
    if (state) {
      prefetchLinks();
      header.style.setProperty("--site-bar-height", `${header.offsetHeight}px`);
      // The panel keeps its scroll offset while it is closed.
      panel.scrollTop = 0;
    }
    header.dataset.mobileMenuOpen = String(state);
    button.setAttribute("aria-expanded", String(state));
    button.dataset.state = state ? "open" : "closed";
    // The page under the menu must not scroll. The lock goes on the root
    // element, where it leaves the sticky bar in place. iOS Safari pans the
    // document through it all the same, which the touch action stops.
    document.documentElement.style.overflow = state ? "hidden" : "";
    document.body.style.touchAction = state ? "none" : "";
  }

  // Closes the menu at once, with its fade switched off.
  function closeInstantly() {
    if (!open) return;
    header.dataset.instant = "";
    setOpen(false);
    // Styles are flushed while transitions are off, so restoring them does not
    // start one from the state that is already applied.
    void header.offsetWidth;
    delete header.dataset.instant;
  }

  button.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    setOpen(!open);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && open) setOpen(false);
  });

  // Following a link in the header closes the menu and releases the page
  // before the page changes. Links that open elsewhere leave it as it is.
  header.addEventListener("click", (event) => {
    const link = event.target.closest("a[href]");
    if (!link || event.defaultPrevented || event.button !== 0) return;
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if (link.target && link.target !== "_self") return;
    closeInstantly();
  });
  window.addEventListener("pageswap", closeInstantly);

  // A window that grows past the phone layout must not keep the menu open or
  // the page locked.
  window.matchMedia(DESKTOP).addEventListener("change", (event) => {
    if (event.matches && open) setOpen(false);
  });
}
