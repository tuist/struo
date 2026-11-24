// Draws the hero copy onto a 2D canvas as a grayscale image. The shader maps
// that brightness to phosphor color, so nothing here knows about the palette.

const LEVELS = { dim: 0.7, bright: 1 };
const HEADLINE_CPS = 42;
const HEADLINE_LEADING = 1.04;
const HEADLINE_MAX_PX = 124;
// A headline line that would have to be set smaller than this to fit the
// screen's width is broken in two instead.
const HEADLINE_MIN_PX = 28;
const LABEL_CPS = 40;
const PLATFORM_CPS = 22;
const COMMAND_CPS = 26;
const CURSOR_BLINK_MS = 530;
const STATION = "STRUO BY TUIST";
// The "Works with" line, relative to the command under it.
const LEAD_SCALE = 1.5;
const LEAD_LEADING = 1.75;
// Heights in command lines: the strip at the top with its rule, and the gap
// between the headline and the "Works with" line.
const STRIP_LINES = 2.4;
const GAP_LINES = 1.2;

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

// Reads what the tube shows from the markup it replaces, so the page copy
// stays the single source of the words on screen.
export function readScreenContent(crt) {
  const heading = crt.querySelector("h1");
  const headingStyle = getComputedStyle(heading);
  const upper = headingStyle.textTransform === "uppercase";
  const panels = [...crt.querySelectorAll('[data-part="crt-channel"]')];
  const text = (panel, part) => panel.querySelector(`[data-part="${part}"]`).textContent.trim();

  return {
    headline: [...heading.childNodes]
      .filter((node) => node.nodeType === Node.TEXT_NODE)
      .map((node) => node.textContent.trim())
      .filter(Boolean)
      .map((line) => (upper ? line.toUpperCase() : line)),
    channels: panels.map((panel) => {
      const prompt = text(panel, "prompt");
      return {
        lead: text(panel, "works-with-lead"),
        platform: text(panel, "platform"),
        prompt,
        command: text(panel, "crt-command").slice(prompt.length).trim(),
      };
    }),
    fonts: { display: headingStyle.fontFamily, mono: getComputedStyle(panels[0]).fontFamily },
  };
}

const rowLength = (row) => row.segments.reduce((sum, segment) => sum + segment.text.length, 0);

export function createScreen(content) {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d", { alpha: false });
  let size = { width: 0, height: 0, scale: 1, gutter: 0, footer: 0 };
  let rows = [];
  // Set once the picture has come up whole, with nothing typed in.
  let whole = false;
  let lastSignature = null;
  // The channel the rows are laid out for, and what the last draw showed.
  let shown = 0;
  let picture = null;

  const monoFont = (px, weight = 400) => `${weight} ${px}px ${content.fonts.mono}`;
  const displayFont = (px) => `500 ${px}px ${content.fonts.display}`;

  function measure(font, text) {
    context.font = font;
    return context.measureText(text).width;
  }

  // Breaks a line at the space that leaves its two halves closest in width.
  function halve(line) {
    const words = line.split(" ");
    const splits = words.slice(1).map((_, at) => [words.slice(0, at + 1).join(" "), words.slice(at + 1).join(" ")]);
    const wider = (halves) => Math.max(...halves.map((half) => measure(displayFont(100), half)));
    return splits.reduce((best, halves) => (wider(halves) < wider(best) ? halves : best), [line]);
  }

  // Picks type sizes that fit the tube, shrinking everything together when the
  // screen is short. The text keeps to the page column and clear of the footer,
  // the stretch at the bottom where the picture fades out. On a tall screen the
  // copy sits in the middle of the room under the strip.
  function metrics() {
    const { width, height, gutter, footer } = size;
    const padX = Math.max(gutter, clamp(width * 0.06, 22, 68));
    const padY = clamp(height * 0.09, 22, 48);
    const room = width - padX * 2;
    const headlineLines = content.headline.flatMap((line) =>
      (room / measure(displayFont(100), line)) * 100 < HEADLINE_MIN_PX ? halve(line) : [line],
    );
    const widest = Math.max(...headlineLines.map((line) => measure(displayFont(100), line)));

    for (let shrink = 1; ; shrink -= 0.06) {
      const mono = clamp(width / 62, 12, 17.5) * shrink;
      const lineHeight = Math.round(mono * 1.5);
      const lead = mono * LEAD_SCALE;
      const headline = Math.min((room / widest) * 100, height * 0.22 * shrink, HEADLINE_MAX_PX);
      const copy = lineHeight * (GAP_LINES + 1) + headlineLines.length * headline * HEADLINE_LEADING + lead * LEAD_LEADING;
      const spare = height - padY - Math.max(padY, footer) - lineHeight * STRIP_LINES - copy;
      if (spare >= 0 || shrink < 0.6) {
        return { padX, padY, mono, lineHeight, lead, headline, headlineLines, inset: Math.max(0, spare / 2) };
      }
    }
  }

  // Lays the screen out for a channel. The channel's own lines run on the
  // "channel" clock, so turning the dial retypes them and nothing else. A
  // channel tuned by hand also retypes its label in the strip; at switch-on
  // the label is simply there.
  function layout(channelIndex, { tuned }) {
    const { width } = size;
    const { padX, padY, mono, lineHeight, lead, headline, headlineLines, inset } = metrics();
    const channel = content.channels[channelIndex];
    shown = channelIndex;
    const next = [];
    let y = padY;

    const label = `CH ${String(channelIndex + 1).padStart(2, "0")} ${channel.platform.toUpperCase()}`;
    const font = monoFont(mono);
    next.push({ x: padX, y, font, size: mono, clock: "boot", at: 0, segments: [{ text: STATION, level: "dim" }] });
    next.push({
      x: width - padX - measure(font, label),
      y,
      font,
      size: mono,
      clock: tuned ? "channel" : "boot",
      at: 0,
      cps: tuned ? LABEL_CPS : null,
      segments: [{ text: label, level: "dim" }],
    });
    next.push({ rule: true, x: padX, y: y + lineHeight * 1.15, width: width - padX * 2 });
    y += lineHeight * STRIP_LINES + inset;

    let at = 120;
    headlineLines.forEach((text) => {
      next.push({
        x: padX,
        y,
        font: displayFont(headline),
        size: headline,
        clock: "boot",
        at,
        cps: HEADLINE_CPS,
        segments: [{ text, level: "bright" }],
      });
      at += (text.length / HEADLINE_CPS) * 1000 + 90;
      y += headline * HEADLINE_LEADING;
    });
    const bootEnd = at;
    y += lineHeight * GAP_LINES;

    // "Works with" and the prompt are there at once; the platform and the
    // command type in after them.
    const leadText = `${channel.lead} `;
    next.push({
      x: padX,
      y,
      font: monoFont(lead, 500),
      size: lead,
      clock: "channel",
      at: 0,
      cps: PLATFORM_CPS,
      instant: leadText.length,
      segments: [
        { text: leadText, level: "dim" },
        { text: channel.platform, level: "bright", invert: true },
      ],
    });
    y += lead * LEAD_LEADING;

    const promptText = `${channel.prompt} `;
    next.push({
      x: padX,
      y,
      font: monoFont(mono, 500),
      size: mono,
      clock: "channel",
      at: (channel.platform.length / PLATFORM_CPS) * 1000 + 180,
      cps: COMMAND_CPS,
      instant: promptText.length,
      cursor: true,
      segments: [
        { text: promptText, level: "dim" },
        { text: channel.command, level: "bright" },
      ],
    });

    // In a picture that came up whole, only what the dial retunes is typed.
    rows = whole ? next.map((row) => (tuned && row.clock === "channel" ? row : { ...row, at: 0, cps: null })) : next;
    lastSignature = null;
    return { channelDelay: whole ? 0 : bootEnd + 250 };
  }

  // How many characters of a row the clocks have revealed so far.
  function revealed(row, clocks) {
    const elapsed = clocks[row.clock] - row.at;
    if (elapsed < 0) return -1;
    const length = rowLength(row);
    return row.cps ? Math.min(length, (row.instant ?? 0) + Math.floor((elapsed / 1000) * row.cps)) : length;
  }

  function fill(level) {
    const value = Math.round(LEVELS[level] * 255);
    context.fillStyle = `rgb(${value} ${value} ${value})`;
  }

  function drawRow(row, count) {
    context.font = row.font;
    let x = row.x;
    let left = count;
    for (const segment of row.segments) {
      const text = segment.text.slice(0, Math.max(0, left));
      left -= segment.text.length;
      const width = context.measureText(text).width;
      fill(segment.level);
      if (segment.invert && text) {
        const pad = row.size * 0.3;
        context.fillRect(x - pad, row.y - row.size * 0.14, width + pad * 2, row.size * 1.22);
        context.fillStyle = "#000";
      }
      context.fillText(text, x, row.y);
      x += width;
    }
    return x;
  }

  function paint({ counts, booted, cursorOn }) {
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.fillStyle = "#000";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.setTransform(size.scale, 0, 0, size.scale, 0, 0);
    context.textBaseline = "top";

    let cursor = null;
    rows.forEach((row, index) => {
      if (row.rule) {
        if (booted) {
          fill("dim");
          context.globalAlpha = 0.55;
          context.fillRect(row.x, row.y, row.width, 1);
          context.globalAlpha = 1;
        }
        return;
      }
      if (counts[index] < 0) return;
      const end = drawRow(row, counts[index]);
      if (row.cursor) cursor = { x: end, y: row.y, size: row.size };
    });

    if (cursor && cursorOn) {
      fill("bright");
      context.fillRect(cursor.x + 1, cursor.y - cursor.size * 0.1, cursor.size * 0.6, cursor.size * 1.15);
    }
  }

  return {
    canvas,

    // What the canvas shows now, in a form redraw() can put up at another size.
    get picture() {
      return picture;
    },

    // Draws a picture taken earlier at the size the screen has now, and returns
    // the canvas holding it. The next draw() puts the live picture back.
    redraw(taken) {
      const live = rows;
      const channelIndex = shown;
      layout(taken.channel, { tuned: false });
      paint(taken);
      rows = live;
      shown = channelIndex;
      return canvas;
    },

    resize({ width, height, scale, gutter, footer }, channelIndex) {
      size = { width, height, scale, gutter, footer };
      canvas.width = Math.round(width * scale);
      canvas.height = Math.round(height * scale);
      return layout(channelIndex, { tuned: false });
    },

    // Puts the channel's whole picture up at once, with nothing typed in.
    restore(channelIndex) {
      whole = true;
      return layout(channelIndex, { tuned: false });
    },

    tune(channelIndex) {
      return layout(channelIndex, { tuned: true });
    },

    // Redraws when the visible text changed and reports whether it did.
    draw(clocks) {
      const counts = rows.map((row) => (row.rule ? 0 : revealed(row, clocks)));
      const typing = rows.some((row, index) => row.cps && counts[index] >= 0 && counts[index] < rowLength(row));
      const steady = typing || !Number.isFinite(clocks.channel);
      const cursorOn = steady || Math.floor(Math.max(0, clocks.channel) / CURSOR_BLINK_MS) % 2 === 0;
      const signature = `${counts.join(",")}|${clocks.boot >= 0}|${cursorOn}`;
      if (signature === lastSignature) return false;
      lastSignature = signature;

      picture = { channel: shown, counts, booted: clocks.boot >= 0, cursorOn };
      paint(picture);
      return true;
    },
  };
}
