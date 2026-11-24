const TRIGGER = '[data-part="search-trigger"], [data-part="search-button"]';
const OPTION = '[role="option"]';

// Opens and closes the search dialog and walks its results with the
// keyboard. The server renders the results as the query changes.
const DocsSearchHook = {
  mounted() {
    this.dialog = this.el.querySelector("dialog");
    this.selected = 0;

    this.open = () => {
      if (this.dialog.open) return;
      this.dialog.showModal();
      const input = this.input();
      input.focus();
      input.select();
      this.select(0);
    };

    this.close = () => {
      if (this.dialog.open) this.dialog.close();
    };

    this.onShortcut = (event) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      if (event.key.toLowerCase() !== "k") return;

      event.preventDefault();
      if (this.dialog.open) this.close();
      else this.open();
    };

    this.onTriggerClick = (event) => {
      if (event.target.closest(TRIGGER)) this.open();
    };

    // The trigger is a read-only input: a key that would act on a button
    // opens the dialog, and a typed character starts the query with it.
    this.onTriggerKey = (event) => {
      if (!event.target.closest(TRIGGER) || event.metaKey || event.ctrlKey || event.altKey) return;

      const typed = event.key.length === 1 && event.key !== " ";
      if (!typed && !["Enter", " ", "ArrowDown"].includes(event.key)) return;

      event.preventDefault();
      this.open();
      if (typed && event.target.matches("input")) {
        const input = this.input();
        input.value = event.key;
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }
    };

    this.onDialogKey = (event) => {
      if (event.isComposing) return;

      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const count = this.options().length;
        if (count) this.select((this.selected + (event.key === "ArrowDown" ? 1 : -1) + count) % count);
      } else if (event.key === "Enter") {
        event.preventDefault();
        this.options()[this.selected]?.click();
      }
    };

    this.onDialogClick = (event) => {
      // The dialog has no padding, so a click on it is a click on the backdrop.
      if (event.target === this.dialog || event.target.closest(OPTION)) this.close();
    };

    this.onPointerMove = (event) => {
      const index = this.options().indexOf(event.target.closest(OPTION));
      if (index >= 0 && index !== this.selected) this.select(index, { scroll: false });
    };

    window.addEventListener("keydown", this.onShortcut);
    document.addEventListener("click", this.onTriggerClick);
    document.addEventListener("keydown", this.onTriggerKey);
    window.addEventListener("phx:page-loading-start", this.close);
    this.dialog.addEventListener("keydown", this.onDialogKey);
    this.dialog.addEventListener("click", this.onDialogClick);
    this.dialog.addEventListener("pointermove", this.onPointerMove);
  },

  // New results: the first one is selected again.
  updated() {
    this.select(0);
  },

  destroyed() {
    window.removeEventListener("keydown", this.onShortcut);
    document.removeEventListener("click", this.onTriggerClick);
    document.removeEventListener("keydown", this.onTriggerKey);
    window.removeEventListener("phx:page-loading-start", this.close);
  },

  input() {
    return this.dialog.querySelector('input[name="q"]');
  },

  options() {
    return [...this.dialog.querySelectorAll(OPTION)];
  },

  select(index, { scroll = true } = {}) {
    const options = this.options();
    this.selected = index;

    options.forEach((option, position) => option.setAttribute("aria-selected", String(position === index)));

    const selected = options[index];
    if (selected) this.input().setAttribute("aria-activedescendant", selected.id);
    else this.input().removeAttribute("aria-activedescendant");

    if (selected && scroll) {
      if (index === 0) selected.closest('[role="listbox"]').scrollTop = 0;
      else selected.scrollIntoView({ block: "nearest" });
    }
  },
};

export default DocsSearchHook;
