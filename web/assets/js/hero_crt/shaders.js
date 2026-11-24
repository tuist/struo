// GLSL for the hero tube. Three passes: phosphor persistence, a separable blur
// for bloom, and the composite that bends, scans, and lights the picture.

const PRECISION = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
`;

export const VERTEX = `
attribute vec2 aPos;
varying vec2 vUv;

void main() {
  vUv = aPos * 0.5 + 0.5;
  gl_Position = vec4(aPos, 0.0, 1.0);
}
`;

// Phosphor keeps glowing after the beam leaves, so each frame holds on to a
// decayed copy of the previous one.
export const PERSIST = `${PRECISION}
uniform sampler2D uText;
uniform sampler2D uPrev;
uniform float uDecay;
varying vec2 vUv;

void main() {
  float now = texture2D(uText, vUv).r;
  float before = texture2D(uPrev, vUv).r * uDecay - 0.004;
  gl_FragColor = vec4(vec3(max(now, before)), 1.0);
}
`;

export const BLUR = `${PRECISION}
uniform sampler2D uImage;
uniform vec2 uStep;
varying vec2 vUv;

void main() {
  float sum = texture2D(uImage, vUv).r * 0.227027;
  sum += (texture2D(uImage, vUv + uStep * 1.384615).r + texture2D(uImage, vUv - uStep * 1.384615).r) * 0.316216;
  sum += (texture2D(uImage, vUv + uStep * 3.230769).r + texture2D(uImage, vUv - uStep * 3.230769).r) * 0.070270;
  gl_FragColor = vec4(vec3(sum), 1.0);
}
`;

export const COMPOSITE = `${PRECISION}
uniform sampler2D uImage;
uniform sampler2D uBloom;
uniform sampler2D uBurnImage;
uniform vec2 uResolution;
uniform vec3 uTint;
uniform vec3 uHot;
uniform vec3 uGround;
uniform float uColor;
uniform float uFringe;
uniform float uTime;
uniform float uPower;
uniform float uStatic;
uniform float uRoll;
uniform float uLines;
uniform float uMotion;
uniform float uBurn;
varying vec2 vUv;

const float TAU = 6.2831853;

float hash(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

// Bulge of the glass: the picture pulls in at the corners.
vec2 curve(vec2 uv) {
  uv = (uv * 2.0 - 1.0) * 1.012;
  vec2 bend = abs(uv.yx) / vec2(6.5, 4.6);
  uv += uv * bend * bend;
  return uv * 0.5 + 0.5;
}

// Signed distance to a rounded box filling [0, 1], negative inside.
float roundedBox(vec2 p, float radius, float aspect) {
  vec2 half_size = vec2(aspect, 1.0);
  vec2 d = abs(p * 2.0 - 1.0) * half_size - half_size + radius;
  return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - radius;
}

float easeOutBack(float x) {
  float y = x - 1.0;
  return 1.0 + 2.70158 * y * y * y + 1.70158 * y * y;
}

void main() {
  float aspect = uResolution.x / uResolution.y;
  vec2 tube = curve(vUv);
  // The glass runs off the bottom of the picture, so only its top corners and
  // its sides show. It dissolves into the page over a band inside its rim, kept
  // narrower than the picture's margin on a narrow screen.
  float rim = roundedBox(vec2(tube.x, max(tube.y, 0.5)), 0.085, aspect);
  float glass = 1.0 - smoothstep(-min(0.06, 0.03 * aspect), 0.0, rim);

  // Power: the raster opens from a dot to a line, then springs to full height.
  float sx = max(smoothstep(0.0, 0.42, uPower), 0.0005);
  float sy = mix(0.004, 1.0, easeOutBack(clamp((uPower - 0.34) / 0.66, 0.0, 1.0)));
  float squash = 1.0 - clamp(sy, 0.0, 1.0);
  vec2 p = (tube - 0.5) / vec2(sx, sy) + 0.5;
  float raster = (1.0 - smoothstep(-0.012, 0.004, roundedBox(p, 0.085, aspect))) * step(0.001, uPower);

  // Signal trouble: per-line jitter, plus tearing and vertical roll while the
  // tube warms up.
  float tick = floor(uTime * 30.0);
  float jitter = hash(vec2(floor(p.y * uLines), tick)) - 0.5;
  vec2 s = p;
  s.x += jitter * (0.0006 * uMotion + 0.03 * uStatic);
  s.x += uStatic * 0.035 * sin(p.y * 31.0 + uTime * 40.0);
  s.y = fract(p.y + uRoll);

  // The three guns never land on quite the same spot, least of all toward the
  // corners. On a color set that leaves a red fringe on one side of an edge and
  // a green one on the other.
  vec2 centered = tube - 0.5;
  float offCenter = dot(centered, centered);
  float spread = (0.00025 + 0.0035 * offCenter + 0.0003 * uColor) * uFringe + 0.008 * uStatic;
  vec3 lum = vec3(
    texture2D(uImage, s + vec2(spread, 0.0)).r,
    texture2D(uImage, s).r,
    texture2D(uImage, s - vec2(spread, 0.0)).r
  );
  float bloom = texture2D(uBloom, s).r;

  // On a color set the amber is red and green light mixed, with a trace of
  // blue, and the unlit screen glows a cool gray.
  vec3 guns = uTint + vec3(0.0, 0.0, 0.08) * uColor;
  vec3 color = guns * lum * 1.65;
  color += uHot * pow(lum.g, 2.0) * 0.26;
  // The halo levels off in the thick of a lit area, so that dark marks inside
  // one, such as the name on the lit block, stay dark enough to read.
  color += guns * 0.32 * (1.0 - exp(-bloom * 4.4));
  color += mix(uTint * 0.02, vec3(0.012, 0.015, 0.022), 0.85 * uColor);

  // Scanlines thin out where the beam is bright.
  float beam = 0.5 + 0.5 * cos(p.y * uLines * TAU);
  float depth = mix(0.4, 0.1, clamp(lum.g + bloom, 0.0, 1.0));
  color *= 1.0 - depth * (1.0 - beam);

  // Phosphor stripes: each device pixel column lights one of red, green, blue.
  // Bright areas spill across the stripes and wash them out.
  float stripe = mod(gl_FragCoord.x, 3.0);
  vec3 triad = vec3(step(stripe, 1.0), step(1.0, stripe) * step(stripe, 2.0), step(2.0, stripe));
  float washout = 1.0 - 0.6 * smoothstep(0.35, 1.0, lum.g + bloom);
  color *= mix(vec3(1.0 - 0.07 * triad.b), triad * 1.5 + 0.5, 0.55 * uColor * washout);

  vec2 cell = gl_FragCoord.xy + tick * 17.0;
  vec3 grain = vec3(hash(cell), hash(cell + 41.0), hash(cell + 83.0)) - 0.5;
  color += mix(uTint * grain.r, grain * 0.7, uColor) * 0.05;
  float snow = hash(floor(gl_FragCoord.xy * 0.5) + tick * 31.0);
  color = mix(color, uTint * snow * 1.2 + vec3(snow * 0.25), uStatic * 0.75);

  // Mains hum, a faint sync band drifting down the picture, and flicker.
  color *= 1.0 + 0.045 * uMotion * sin((p.y + uTime * 0.07) * TAU);
  float sweep = p.y - (1.0 - fract(uTime * 0.085));
  color += uTint * uMotion * 0.022 * smoothstep(0.012, 0.0, abs(sweep));
  color += uTint * uMotion * 0.014 * smoothstep(0.12, 0.0, sweep) * step(0.0, sweep);
  color *= 1.0 - uMotion * (0.02 * (0.5 + 0.5 * sin(uTime * 75.0)) + 0.02 * hash(vec2(tick, 7.0)));

  // The collapsed raster burns far brighter than the open one. The boost falls
  // off steeply so the opening picture never floods the screen with light.
  float burn = pow(squash, 8.0);
  color = color * (1.0 + 3.0 * burn) + (uTint * 1.6 + uHot * 0.8) * burn;

  float vignette = tube.x * tube.y * (1.0 - tube.x) * (1.0 - tube.y);
  float shade = mix(0.42, 1.0, pow(clamp(16.0 * vignette, 0.0, 1.0), 0.3));
  color *= shade;

  // A spot driven past full keeps its hue and pales toward the hot color the
  // harder it is driven. Left to clip, the red would stall while the green
  // went on rising, and the orange would turn yellow.
  float peak = max(max(color.r, color.g), max(color.b, 1.0));
  color = mix(color / peak, uHot, 0.6 * (1.0 - 1.0 / peak));

  // The unlit glass is the page's ground, less the faint glow the tube adds
  // over it, so the dark tube and the page around it read as one surface.
  vec3 result = max(uGround - 0.01, 0.0) + color * raster;

  // Burn-in: a picture that sat on the tube stays in the phosphor as a pale
  // mark, which shows best once the screen has gone dark.
  vec2 soft = 1.5 / uResolution;
  float ghost = 0.25 * (
    texture2D(uBurnImage, tube + vec2(soft.x, 0.0)).r + texture2D(uBurnImage, tube - vec2(soft.x, 0.0)).r +
    texture2D(uBurnImage, tube + vec2(0.0, soft.y)).r + texture2D(uBurnImage, tube - vec2(0.0, soft.y)).r
  );
  result += mix(uTint, vec3(0.62, 0.52, 0.44), 0.4) * ghost * 0.075 * uBurn * shade;

  vec2 highlight = (vUv - vec2(0.2, 0.88)) * vec2(aspect * 0.5, 1.7);
  result += vec3(1.0, 0.97, 0.92) * 0.035 * exp(-dot(highlight, highlight) * 3.0);

  // Past the rim the canvas is clear, so the page shows around the tube.
  gl_FragColor = vec4(result * glass, glass);
}
`;
