import { EVENTS } from "./scenario.js";

const TOTAL_TICK_MINUTES = 10;
// The pace is the point of the demo: a lookup passes in a blink, so a run of
// restored actions streams by, while a build holds the result for a time that
// grows with how long the action really took.
const STEP_MS = 70;
const HIT_HOLD_MS = 220;
const MISS_HOLD_MS = 600;
const BUILD_BASE_MS = 500;
const BUILD_MS_PER_SECOND = 32;
// A build's timer is redrawn in steps, the way an old display refreshes.
const BUILD_TICK_MS = 260;
const RESTART_MS = 5000;
const KEY_PLACEHOLDER = "--------";
// A key resolves out of a few frames of noise.
const HEX = "0123456789abcdef";
const DECODE_FRAMES = 3;
const DECODE_FRAME_MS = 30;
const LIT = ["box-action", "box-key", "box-result", "wire-key", "wire-result"];

const seconds = (value) => `${value.toFixed(1)}s`;

function minutesAndSeconds(value) {
  const whole = Math.round(value);
  return `${Math.floor(whole / 60)}m ${String(whole % 60).padStart(2, "0")}s`;
}

// Writes a readout with each digit in a cell of its own. The display face has
// no tabular figures, and the cells keep a total from shifting as it counts.
function setFigures(readout, text) {
  readout.replaceChildren(
    ...[...text].map((character) => {
      if (character < "0" || character > "9") return character;
      const cell = document.createElement("span");
      cell.dataset.part = "demo-digit";
      cell.textContent = character;
      return cell;
    }),
  );
}

// Replays the scenario on a loop: each action is hashed into its key, then
// either restored from the cache at once or built and stored. The loop waits
// while the demo is off screen. With reduced motion it shows the finished run
// and stops.
function startDemo(root) {
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const nodes = new Map();
  const node = (name) => {
    if (!nodes.has(name)) nodes.set(name, root.querySelector(`[data-node="${name}"]`));
    return nodes.get(name);
  };
  const activate = (name, active = true) => node(name).setAttribute("data-active", String(active));
  root.querySelectorAll("[data-figures]").forEach((readout) => setFigures(readout, readout.textContent));

  // Writes a readout and replays the flicker its styles give a fresh value.
  // A value that did not change holds steady.
  function refresh(name, text) {
    const readout = node(name);
    if (readout.textContent === text) return;
    if (readout.hasAttribute("data-figures")) setFigures(readout, text);
    else readout.textContent = text;
    for (const animation of readout.getAnimations()) {
      animation.currentTime = 0;
      animation.play();
    }
  }

  const uncachedMinutes = EVENTS.reduce((sum, event) => sum + event.uncached, 0) / 60;
  const totalMax = Math.ceil(uncachedMinutes / 5) * 5;
  for (let minutes = 0; minutes <= totalMax; minutes += TOTAL_TICK_MINUTES) {
    const mark = document.createElement("span");
    mark.dataset.part = "demo-tick";
    mark.style.bottom = `${(minutes / totalMax) * 100}%`;
    mark.textContent = minutes;
    node("total-ticks").appendChild(mark);
  }

  // What the run has done so far: which machine first built each key, and
  // after each action the count restored and the running seconds with and
  // without the cache. The chart has marked the first "plotted" of those
  // actions.
  let builders = new Map();
  let restored = [0];
  let uncached = [0];
  let cached = [0];
  let plotted = 0;

  let visible = false;
  let wake = null;
  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    root.dataset.running = String(visible);
    if (visible) wake?.();
  }).observe(root);
  const whenVisible = () =>
    visible || still
      ? Promise.resolve()
      : new Promise((resolve) => {
          wake = resolve;
        });

  const sleep = (milliseconds) => (still ? Promise.resolve() : new Promise((resolve) => setTimeout(resolve, milliseconds)));

  // Lights a wire from its near end to its far end over the given time.
  function travel(name, milliseconds) {
    node(name).style.setProperty("--wire-ms", `${milliseconds}ms`);
    activate(name);
    return sleep(milliseconds);
  }

  async function decode(name, text) {
    for (let frame = 0; frame < DECODE_FRAMES && !still; frame += 1) {
      node(name).textContent = Array.from(text, () => HEX[Math.floor(Math.random() * HEX.length)]).join("");
      await sleep(DECODE_FRAME_MS);
    }
    refresh(name, text);
  }

  async function build(event) {
    const ticks = Math.max(1, Math.round((BUILD_BASE_MS + event.seconds * BUILD_MS_PER_SECOND) / BUILD_TICK_MS));
    for (let tick = 0; tick < ticks && !still; tick += 1) {
      refresh("result", `building ${seconds((event.seconds * tick) / ticks)}`);
      node("progress").style.width = `${(tick / ticks) * 100}%`;
      await sleep(BUILD_TICK_MS);
    }
  }

  // Marks an action on the chart where it left the total: a blip that fades
  // for one restored, a contact that stays for one built.
  function mark(index) {
    const event = EVENTS[index - 1];
    if (event.hit && still) return;
    const contact = document.createElement("span");
    contact.dataset.part = "demo-contact";
    contact.dataset.result = event.hit ? "hit" : "miss";
    contact.style.left = `${(index / EVENTS.length) * 100}%`;
    contact.style.bottom = `${(cached[index] / 60 / totalMax) * 100}%`;
    if (event.hit) contact.addEventListener("animationend", () => contact.remove());
    node("contacts").appendChild(contact);
  }

  // Draws the chart and the totals as they stood after "count" actions.
  function plot(count) {
    const trace = (series) =>
      series
        .slice(0, count + 1)
        .map((total, index) => {
          const x = (index / EVENTS.length) * 100;
          const y = 100 - (total / 60 / totalMax) * 100;
          return `${index ? "L" : "M"}${x.toFixed(2)} ${y.toFixed(2)}`;
        })
        .join("");
    node("line-none").setAttribute("d", trace(uncached));
    node("line-once").setAttribute("d", trace(cached));
    for (let index = plotted + 1; index <= count; index += 1) mark(index);
    plotted = count;

    refresh("restored", `${restored[count]} / ${count}`);
    refresh("saved", minutesAndSeconds(uncached[count] - cached[count]));
    refresh("speedup", `${(count ? uncached[count] / cached[count] : 1).toFixed(1)}x`);
  }

  function reset() {
    builders = new Map();
    restored = [0];
    uncached = [0];
    cached = [0];
    plotted = 0;
    LIT.forEach((name) => activate(name, false));
    node("contacts").textContent = "";
    node("key").textContent = KEY_PLACEHOLDER;
    node("key-state").textContent = "";
    node("result-note").textContent = "waiting";
    node("result").textContent = "--";
    node("progress").style.width = "0";
    plot(0);
  }

  // The last action's key and result stay on screen, dimmed, until the new
  // action reaches them.
  async function play(event) {
    LIT.forEach((name) => activate(name, false));
    node("progress").style.width = "0";
    refresh("machine", `on ${event.machine}`);
    refresh("command", `struo build ${event.target.path}`);
    refresh("inputs", `sources${event.edited ? " edited" : ""}, rev ${event.revision}`);
    refresh("environment", event.target.tool);
    activate("box-action");

    await travel("wire-key", STEP_MS);
    activate("box-key");
    await decode("key", event.key);
    node("key-state").dataset.result = event.hit ? "hit" : "miss";
    refresh("key-state", event.hit ? "in cache" : "new");

    await travel("wire-result", STEP_MS);
    activate("box-result");
    if (event.hit) {
      refresh("result-note", `from cache, built on ${builders.get(event.key)}`);
      refresh("result", `restored in ${seconds(event.seconds)}`);
    } else {
      refresh("result-note", "not in cache, building");
      await build(event);
      refresh("result-note", "built and stored");
      refresh("result", `built in ${seconds(event.seconds)}`);
      builders.set(event.key, event.machine);
    }
    node("progress").style.width = "100%";

    restored.push(restored.at(-1) + (event.hit ? 1 : 0));
    uncached.push(uncached.at(-1) + event.uncached);
    cached.push(cached.at(-1) + event.seconds);
    plot(cached.length - 1);
    await sleep(event.hit ? HIT_HOLD_MS : MISS_HOLD_MS);
  }

  async function run() {
    for (;;) {
      reset();
      for (const event of EVENTS) {
        await whenVisible();
        await play(event);
      }
      if (still) return;
      await sleep(RESTART_MS);
    }
  }

  run();
}

export function setupCacheDemo(root = document) {
  const demo = root.querySelector('[data-part="demo-frame"]');
  if (demo) startDemo(demo);
}
