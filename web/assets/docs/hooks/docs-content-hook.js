import { setupCodeCopy } from "../shared/hooks/code-copy.js";
import { setupCodeGroups } from "../shared/hooks/code-group.js";

function initializeCodeBlocks(container) {
  setupCodeCopy(container);
  setupCodeGroups(container);
}

// Each page has its own element, so one mounting means a page was shown,
// whether by a link or by the browser's back and forward buttons.
const DocsContentHook = {
  mounted() {
    initializeCodeBlocks(this.el);
    window.dispatchEvent(new Event("docs:page-shown"));
  },
  updated() {
    initializeCodeBlocks(this.el);
  },
};

export default DocsContentHook;
