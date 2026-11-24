import { playKeyLatch, playPowerOff, playPowerOn, warmAudio } from "../sounds.js";
import { createRenderer } from "./renderer.js";
import { createScreen, readScreenContent } from "./screen.js";

const POWER_ON_MS = 700;
// An overloaded tube cuts out: the picture collapses, the screen stays dark,
// and the set switches itself back on the way it does on a page load, except
// that the picture is there whole instead of being typed in. The picture it
// was showing stays burnt into the phosphor for a while after.
const POWER_OFF_MS = 560;
const OUTAGE_MS = 2600;
const BURN_FADE_MS = 16000;
// This many channel changes within the window overload the tube.
const OVERLOAD_TURNS = 8;
const OVERLOAD_WINDOW_MS = 2500;
const BOOT_DELAY_MS = 420;
const STATIC_MS = 420;
// Snow and tearing the picture starts with while the tube warms up.
const WARM_UP_STATIC = 0.6;
const SCANLINE_PX = 3;
const MAX_SCALE = 2;
// Widest drawing buffer, in device pixels, for a tube that spans the page.
const MAX_BUFFER_WIDTH = 4096;
const FRAME_MS = 1000 / 60;
// Share of its brightness the phosphor keeps from one 60 Hz frame to the next.
const PERSISTENCE = 0.84;
// How much of a color set shows through the amber picture, from 0 (a
// monochrome monitor) to 1: red, green, and blue phosphor stripes, guns that
// land slightly apart, a cool black level, and colored grain.
const COLOR_REALISM = 0.6;
// The guns land apart by a share of the tube's width. Past this width, in CSS
// pixels, the share shrinks to keep the distance the same: the text stops
// growing with the tube, and fringes wider than its strokes make it hard to
// read.
const FRINGE_FULL_WIDTH = 480;

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
const clamp01 = (value) => clamp(value, 0, 1);

// Temporary, while the color treatment is being chosen: "?rgb=0.3" in the
// page address overrides COLOR_REALISM.
function colorRealism() {
  const override = Number.parseFloat(new URLSearchParams(window.location.search).get("rgb"));
  return Number.isFinite(override) ? clamp01(override) : COLOR_REALISM;
}

// Resolves a CSS color to the 0..1 RGB triple the shader works in.
function readColor(color) {
  const context = document.createElement("canvas").getContext("2d");
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  const [red, green, blue] = context.getImageData(0, 0, 1, 1).data;
  return [red / 255, green / 255, blue / 255];
}

// Runs the WebGL tube over the hero markup. The tube switches on once, when
// the page loads. Returns null when WebGL is unavailable, which leaves the
// markup itself on screen.
function startTube(crt, { onSwitchOn }) {
  const surface = crt.querySelector('[data-part="crt-screen"]');
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const observers = [];

  let alive = true;
  let ready = false;
  let visible = true;
  let frameId = null;
  let last = null;

  // Milliseconds of animation shown so far. It advances only while frames are
  // drawn, so the switch-on plays when the tube is first seen.
  let clock = 0;
  let channel = 0;
  let lines = 1;
  let fringe = 1;
  let power = 0;
  let staticAmount = WARM_UP_STATIC;
  // How long after the boot starts the channel's lines begin to type.
  let channelDelay = 0;
  // Clock reading at the last channel change. Null until the dial is turned.
  let tunedAt = null;
  // Clock reading when the picture starts to appear.
  let bootAt = BOOT_DELAY_MS;
  // Set while the tube is cut out: when it happened and how bright it was.
  let outage = null;
  let burn = 0;
  // The picture that is burnt into the tube, kept to draw again at a new size.
  let burnt = null;
  // The surface measurements the buffers were last built for.
  let sized = null;

  const renderer = createRenderer(crt.querySelector('[data-part="crt-canvas"]'), { onContextLost: shutDown });
  if (!renderer) return null;

  const content = readScreenContent(crt);
  const screen = createScreen(content);
  // The phosphor's color, the paler one it burns at where the beam is
  // strongest, and the page's ground. All come from the stylesheet.
  const phosphor = getComputedStyle(crt.querySelector('[data-part="crt-content"]'));
  const tint = readColor(phosphor.color);
  const hot = readColor(phosphor.getPropertyValue("--struo-phosphor-bright"));
  const ground = readColor(getComputedStyle(document.documentElement).backgroundColor);
  const color = colorRealism();

  function shutDown() {
    alive = false;
    cancelAnimationFrame(frameId);
    observers.forEach((observer) => observer.disconnect());
    crt.dataset.crt = "off";
  }

  function request() {
    if (alive && frameId === null) frameId = requestAnimationFrame(step);
  }

  function step(now) {
    frameId = null;
    const elapsed = last === null ? 0 : clamp(now - last, 0, 100);
    last = now;
    const still = reducedMotion.matches;

    clock += elapsed;
    if (outage && (still || clock - outage.at >= POWER_OFF_MS + OUTAGE_MS)) switchBackOn();

    if (still) {
      power = 1;
      staticAmount = 0;
      burn = 0;
    } else if (outage) {
      const collapse = clamp01((clock - outage.at) / POWER_OFF_MS);
      // The curve flattens toward the end, so the dot lingers before it dies.
      power = outage.from * (1 - collapse) ** 2.2;
      burn = collapse;
    } else {
      power = clamp01(power + elapsed / POWER_ON_MS);
      staticAmount = Math.max(0, staticAmount - elapsed / STATIC_MS);
      burn = Math.max(0, burn - elapsed / BURN_FADE_MS);
    }

    const boot = still ? Infinity : clock - bootAt;
    const channelClock = tunedAt === null || still ? boot - channelDelay : clock - tunedAt;
    if (screen.draw({ boot, channel: channelClock })) renderer.uploadText(screen.canvas);

    renderer.render({
      tint,
      hot,
      ground,
      color,
      fringe,
      lines,
      power,
      staticAmount,
      burn,
      time: (now / 1000) % 600,
      roll: 0.3 * staticAmount ** 2,
      decay: still ? 0 : PERSISTENCE ** (elapsed / FRAME_MS),
      motion: still ? 0 : 1,
    });

    if (still || !visible) last = null;
    else request();
  }

  // The tube switches on again as it does on a page load, the raster opening
  // through snow, but with the whole picture already on it, on whichever
  // channel the dial was left at.
  function switchBackOn() {
    outage = null;
    power = 0;
    staticAmount = WARM_UP_STATIC;
    bootAt = clock;
    tunedAt = null;
    ({ channelDelay } = screen.restore(channel));
    crt.dataset.power = "on";
    if (!reducedMotion.matches) onSwitchOn();
  }

  function resize() {
    const width = surface.clientWidth;
    const height = surface.clientHeight;
    if (!width || !height) return;
    const scale = Math.min(window.devicePixelRatio || 1, MAX_SCALE, MAX_BUFFER_WIDTH / width);
    // The tube breaks out of the page column, and its text lines up with it.
    const gutter = crt.parentElement.getBoundingClientRect().left - surface.getBoundingClientRect().left;
    const footer = parseFloat(getComputedStyle(surface).getPropertyValue("--crt-fade")) || 0;
    const measured = [width, height, scale, gutter, footer].join(" ");
    if (measured === sized) return;
    sized = measured;

    renderer.resize(Math.round(width * scale), Math.round(height * scale));
    ({ channelDelay } = screen.resize({ width, height, scale, gutter, footer }, channel));
    // The burn belongs to the picture, so it follows the picture's new layout.
    if (burnt && (outage || burn > 0)) renderer.uploadBurn(screen.redraw(burnt));
    lines = Math.round(height / SCANLINE_PX);
    fringe = Math.min(1, FRINGE_FULL_WIDTH / width);
    // Resizing empties the drawing buffers. Drawing again before the browser
    // paints keeps the picture from blinking while the window is dragged.
    cancelAnimationFrame(frameId);
    step(performance.now());
  }

  const copy = [...content.headline, ...content.channels.flatMap((entry) => Object.values(entry)), "CH 0123456789"].join(" ");
  const sample = copy + copy.toUpperCase();
  Promise.allSettled([
    document.fonts.load(`500 48px ${content.fonts.display}`, sample),
    document.fonts.load(`400 16px ${content.fonts.mono}`, sample),
    document.fonts.load(`500 16px ${content.fonts.mono}`, sample),
  ]).then(() => {
    if (!alive) return;
    ready = true;
    resize();

    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(surface);
    const visibilityObserver = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      if (visible) request();
    });
    visibilityObserver.observe(surface);
    observers.push(resizeObserver, visibilityObserver);
    reducedMotion.addEventListener("change", request);
  });

  crt.dataset.crt = "on";

  return {
    // Retypes the channel label, the platform, and its command. The rest of
    // the picture stays as it is. A tube that is cut out shows the channel
    // when it comes back.
    tune(index) {
      channel = index;
      if (!alive || !ready || outage) return;
      screen.tune(channel);
      tunedAt = clock;
      request();
    },

    // Cuts the tube out and reports whether it did. A tube that is still
    // warming up, already out, or held still for reduced motion stays as it is.
    overload() {
      if (!alive || !ready || outage || power < 1 || reducedMotion.matches) return false;
      burnt = screen.picture;
      renderer.uploadBurn(screen.canvas);
      outage = { at: clock, from: power };
      crt.dataset.power = "off";
      request();
      return true;
    },
  };
}

export function setupHeroCrt(root = document) {
  const crt = root.querySelector('[data-part="crt"]');
  if (!crt) return;

  const tube = startTube(crt, { onSwitchOn: playPowerOn });
  if (!tube) crt.dataset.crt = "off";

  const board = crt.querySelector('[data-part="channel-keys"]');
  const keys = [...board.querySelectorAll('[data-part="channel-key"]')];
  const panels = [...crt.querySelectorAll('[data-part="crt-channel"]')];
  const keyOf = (event) => event.target.closest('[data-part="channel-key"]');
  let channel = 0;
  let changes = [];
  // The key the pointer is holding down, which has already made its sound.
  let held = null;

  // True once the channel has been changed too many times in too short a while.
  function overworked() {
    const now = performance.now();
    changes = [...changes.filter((at) => now - at < OVERLOAD_WINDOW_MS), now];
    if (changes.length < OVERLOAD_TURNS) return false;
    changes = [];
    return true;
  }

  function show(index) {
    channel = index;
    keys.forEach((key, at) => key.setAttribute("aria-pressed", String(at === index)));
    panels.forEach((panel, at) => panel.setAttribute("data-active", String(at === index)));
    tube?.tune(index);
  }

  function switchTo(index) {
    if (index === channel) return;
    const cutOut = overworked() && tube?.overload();
    show(index);
    if (cutOut) playPowerOff();
  }

  ["pointerenter", "pointerdown", "keydown"].forEach((type) => board.addEventListener(type, warmAudio));

  // A key that is already down has no other key to free.
  const sound = (key) => playKeyLatch({ releases: keys.indexOf(key) !== channel });

  // A mouse or pen sounds the key as it goes down. A finger may be starting a
  // scroll, and a keyboard has no press of its own, so those sound it when it
  // is activated.
  board.addEventListener("pointerdown", (event) => {
    const key = keyOf(event);
    if (!key || event.pointerType === "touch" || event.button !== 0) return;
    held = key;
    sound(key);
  });

  board.addEventListener("click", (event) => {
    const key = keyOf(event);
    if (!key) return;
    if (key !== held) sound(key);
    held = null;
    switchTo(keys.indexOf(key));
  });

  ["pointerleave", "pointercancel"].forEach((type) =>
    board.addEventListener(type, () => {
      held = null;
    }),
  );

  // The rotary channel dial, switched off for now. It needs playDetents from
  // ../sounds.js.
  // const dial = crt.querySelector('[data-part="channel-dial"]');
  // const stops = [...dial.querySelectorAll('[data-part="dial-stop"]')];
  // const inputs = stops.map((stop) => stop.querySelector("input"));
  // const panels = [...crt.querySelectorAll('[data-part="crt-channel"]')];
  // let channel = 0;
  // let turns = [];
  //
  // // True once the dial has been turned too many times in too short a while.
  // function overworked() {
  //   const now = performance.now();
  //   turns = [...turns.filter((at) => now - at < OVERLOAD_WINDOW_MS), now];
  //   if (turns.length < OVERLOAD_TURNS) return false;
  //   turns = [];
  //   return true;
  // }
  //
  // function show(index) {
  //   channel = index;
  //   inputs[index].checked = true;
  //   panels.forEach((panel, at) => panel.setAttribute("data-active", String(at === index)));
  //   dial.style.setProperty("--dial-angle", stops[index].style.getPropertyValue("--stop-angle"));
  //   tube?.tune(index);
  // }
  //
  // function turnTo(index) {
  //   if (index === channel) return;
  //   playDetents(Math.abs(index - channel));
  //   const cutOut = overworked() && tube?.overload();
  //   show(index);
  //   if (cutOut) playPowerOff();
  // }
  //
  // // A browser that restores form state on reload may bring the page back with
  // // another stop already chosen.
  // const restored = inputs.findIndex((input) => input.checked);
  // if (restored > 0) show(restored);
  //
  // ["pointerenter", "pointerdown", "pointerup", "keydown"].forEach((type) => dial.addEventListener(type, warmAudio));
  // dial.addEventListener("change", (event) => turnTo(inputs.indexOf(event.target)));
  // dial.querySelector('[data-part="dial-knob"]').addEventListener("click", () => turnTo((channel + 1) % stops.length));
}
