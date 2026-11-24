// The sounds of the page's hardware, synthesized so it ships no audio files:
// the click of the rotary selector dropping into a stop, the tube switching
// off and on, a key of the language board going down, and a key of the
// channel bank latching.

const DETENT_GAP_SECONDS = 0.06;
const NOISE_SECONDS = 0.08;
const SILENCE = 0.0001;
// A sound starts this long after it is asked for, so that its attack is never
// in the past by the time the audio thread picks it up.
const LEAD_SECONDS = 0.03;
// A sound still waiting for the audio engine after this long is dropped
// instead of playing far out of step with the picture.
const STALE_MS = 1000;
const KEY_PRESS_MS = 42;
const KEY_LATCH_MS = 60;
// How long after a key of the bank goes down the key it frees springs back.
const KEY_RELEASE_GAP_SECONDS = 0.055;

let context = null;
let noise = null;

function audioContext() {
  const AudioContext = window.AudioContext ?? window.webkitAudioContext;
  if (!AudioContext) return null;
  if (!context) {
    context = new AudioContext();
    noise = context.createBuffer(1, Math.ceil(context.sampleRate * NOISE_SECONDS), context.sampleRate);
    const samples = noise.getChannelData(0);
    for (let index = 0; index < samples.length; index += 1) samples[index] = Math.random() * 2 - 1;
  }
  return context;
}

// A page the back button brings out of the browser's cache comes back with
// its audio engine as it was frozen, which may never run again. The engine is
// let go when the page is left, so a page that returns builds a new one, the
// way a freshly loaded page does.
window.addEventListener("pagehide", () => {
  context?.close().catch(() => {});
  context = null;
  noise = null;
});

// False until the visitor has worked the page, on browsers that say.
const engaged = () => !navigator.userActivation || navigator.userActivation.hasBeenActive;

// An iPhone or iPad set to silent mutes a page's synthesized sound unless the
// page calls it playback, the way a video's sound is. Playback also stops
// music playing from another app, so the page only makes the claim once the
// visitor has worked one of its controls, never by being opened.
function claimSpeaker() {
  const session = navigator.audioSession;
  if (session && session.type !== "playback") session.type = "playback";
}

// Browsers only start audio on a page the visitor has interacted with, and the
// engine then takes a moment to come up, long enough to swallow a click played
// straight away. So the engine is started as early as the browser allows,
// when the pointer reaches a control or presses it, and every sound waits for
// it to be running.
export function warmAudio() {
  if (!engaged()) return;
  claimSpeaker();
  const audio = audioContext();
  if (audio && audio.state !== "running") audio.resume().catch(() => {});
}

// Plays once the engine is running. The promise settles when a sound of the
// given length has reached the speakers, or at once when nothing will play.
function schedule(play, milliseconds = 0) {
  const audio = audioContext();
  if (!audio) return Promise.resolve();
  if (engaged()) claimSpeaker();
  const asked = performance.now();
  const start = () => {
    if (performance.now() - asked >= STALE_MS) return undefined;
    play(audio, audio.currentTime + LEAD_SECONDS);
    const sounded = (LEAD_SECONDS + (audio.outputLatency || 0)) * 1000 + milliseconds;
    return new Promise((resolve) => setTimeout(resolve, sounded));
  };
  return audio.state === "running" ? Promise.resolve(start()) : audio.resume().then(start, () => {});
}

function fade(audio, peak, start, milliseconds) {
  const level = audio.createGain();
  level.gain.setValueAtTime(peak, start);
  level.gain.exponentialRampToValueAtTime(SILENCE, start + milliseconds / 1000);
  level.connect(audio.destination);
  return level;
}

// A burst of band-passed noise: contacts closing.
function burst(audio, start, { hz, gain, ms }) {
  const source = audio.createBufferSource();
  source.buffer = noise;
  const band = audio.createBiquadFilter();
  band.type = "bandpass";
  band.frequency.value = hz;
  band.Q.value = 1.2;
  source.connect(band).connect(fade(audio, gain, start, ms));
  source.start(start);
  source.stop(start + ms / 1000);
}

// A sine sliding from one pitch to another as it dies away.
function tone(audio, start, { from, to, gain, ms }) {
  const oscillator = audio.createOscillator();
  oscillator.frequency.setValueAtTime(from, start);
  oscillator.frequency.exponentialRampToValueAtTime(to, start + ms / 1000);
  oscillator.connect(fade(audio, gain, start, ms));
  oscillator.start(start);
  oscillator.stop(start + ms / 1000);
}

// One click for every stop the dial passes on its way.
export function playDetents(count) {
  schedule((audio, start) => {
    for (let index = 0; index < count; index += 1) {
      const at = start + index * DETENT_GAP_SECONDS;
      burst(audio, at, { hz: 2400, gain: 0.26, ms: 18 });
      tone(audio, at, { from: 170, to: 85, gain: 0.3, ms: 55 });
    }
  });
}

// A key cap bottoming out in its well: softer and duller than a detent. The
// promise settles when the sound is over.
export function playKeyPress() {
  return schedule((audio, start) => {
    burst(audio, start, { hz: 1500, gain: 0.2, ms: 14 });
    tone(audio, start, { from: 230, to: 115, gain: 0.24, ms: KEY_PRESS_MS });
  }, KEY_PRESS_MS);
}

// A key of the channel bank latching down: the cap bottoming out, the latch
// bar snapping over it, and, when the press frees another key, that key
// springing back up a moment later. It is pitched higher than a language key
// so that the small speaker of a phone carries it.
export function playKeyLatch({ releases }) {
  return schedule((audio, start) => {
    burst(audio, start, { hz: 2600, gain: 0.34, ms: 20 });
    burst(audio, start, { hz: 900, gain: 0.26, ms: 32 });
    tone(audio, start, { from: 420, to: 210, gain: 0.3, ms: KEY_LATCH_MS });
    if (!releases) return;
    const up = start + KEY_RELEASE_GAP_SECONDS;
    burst(audio, up, { hz: 3800, gain: 0.2, ms: 12 });
    tone(audio, up, { from: 720, to: 520, gain: 0.1, ms: 28 });
  }, KEY_LATCH_MS);
}

// The set cutting out: a relay dropping, the thump of the collapsing field,
// and the line whistle falling away.
export function playPowerOff() {
  schedule((audio, start) => {
    burst(audio, start, { hz: 1100, gain: 0.3, ms: 30 });
    tone(audio, start, { from: 120, to: 36, gain: 0.5, ms: 260 });
    tone(audio, start, { from: 3400, to: 240, gain: 0.045, ms: 460 });
  });
}

// The set coming back: the relay pulling in and the field building up.
export function playPowerOn() {
  schedule((audio, start) => {
    burst(audio, start, { hz: 1600, gain: 0.22, ms: 24 });
    tone(audio, start, { from: 52, to: 104, gain: 0.32, ms: 300 });
  });
}
