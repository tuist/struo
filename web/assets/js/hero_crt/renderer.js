import { BLUR, COMPOSITE, PERSIST, VERTEX } from "./shaders.js";

const BLOOM_SCALE = 0.25;
// Horizontal and vertical blur, run twice with a wider step for a soft halo.
const BLUR_PASSES = [
  [1, 0, 1],
  [0, 1, 1],
  [1, 0, 2.4],
  [0, 1, 2.4],
];

function compile(gl, type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(gl.getShaderInfoLog(shader));
  }
  return shader;
}

function link(gl, fragment) {
  const handle = gl.createProgram();
  gl.attachShader(handle, compile(gl, gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(handle, compile(gl, gl.FRAGMENT_SHADER, fragment));
  gl.bindAttribLocation(handle, 0, "aPos");
  gl.linkProgram(handle);
  if (!gl.getProgramParameter(handle, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(handle));
  }
  const locations = new Map();
  const uniform = (name) => {
    if (!locations.has(name)) locations.set(name, gl.getUniformLocation(handle, name));
    return locations.get(name);
  };
  return { handle, uniform };
}

function createTexture(gl) {
  const texture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return texture;
}

function createTarget(gl, width, height) {
  const texture = createTexture(gl);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  const framebuffer = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0);
  gl.clearColor(0, 0, 0, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  return { texture, framebuffer, width, height };
}

function deleteTarget(gl, target) {
  gl.deleteTexture(target.texture);
  gl.deleteFramebuffer(target.framebuffer);
}

// Owns the WebGL side of the tube. Returns null when WebGL is unavailable, so
// the caller can leave the plain markup on screen.
export function createRenderer(canvas, { onContextLost }) {
  const gl = canvas.getContext("webgl", { antialias: false, depth: false, stencil: false });
  if (!gl) return null;

  let programs;
  try {
    programs = { persist: link(gl, PERSIST), blur: link(gl, BLUR), composite: link(gl, COMPOSITE) };
  } catch (error) {
    console.error("Hero tube shaders failed to build:", error);
    return null;
  }

  canvas.addEventListener("webglcontextlost", (event) => {
    event.preventDefault();
    onContextLost();
  });

  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

  const maxSize = gl.getParameter(gl.MAX_TEXTURE_SIZE);
  const text = createTexture(gl);
  // The picture the tube was showing when it last cut out. Black until then.
  const burn = createTexture(gl);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0, 0, 0, 255]));
  let persistence = [];
  let bloom = [];

  function bind(unit, texture) {
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, texture);
  }

  function draw(program, target) {
    gl.bindFramebuffer(gl.FRAMEBUFFER, target ? target.framebuffer : null);
    gl.viewport(0, 0, target ? target.width : canvas.width, target ? target.height : canvas.height);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  function upload(texture, source) {
    bind(0, texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
  }

  return {
    resize(width, height) {
      width = Math.min(width, maxSize);
      height = Math.min(height, maxSize);
      canvas.width = width;
      canvas.height = height;
      [...persistence, ...bloom].forEach((target) => deleteTarget(gl, target));
      persistence = [createTarget(gl, width, height), createTarget(gl, width, height)];
      const bloomWidth = Math.max(1, Math.round(width * BLOOM_SCALE));
      const bloomHeight = Math.max(1, Math.round(height * BLOOM_SCALE));
      bloom = [createTarget(gl, bloomWidth, bloomHeight), createTarget(gl, bloomWidth, bloomHeight)];
    },

    uploadText(source) {
      upload(text, source);
    },

    uploadBurn(source) {
      upload(burn, source);
    },

    render(frame) {
      const [previous, current] = persistence;
      gl.useProgram(programs.persist.handle);
      bind(0, text);
      bind(1, previous.texture);
      gl.uniform1i(programs.persist.uniform("uText"), 0);
      gl.uniform1i(programs.persist.uniform("uPrev"), 1);
      gl.uniform1f(programs.persist.uniform("uDecay"), frame.decay);
      draw(programs.persist, current);
      persistence = [current, previous];

      gl.useProgram(programs.blur.handle);
      gl.uniform1i(programs.blur.uniform("uImage"), 0);
      let source = current;
      BLUR_PASSES.forEach(([x, y, reach], index) => {
        const target = bloom[index % 2];
        bind(0, source.texture);
        gl.uniform2f(programs.blur.uniform("uStep"), (x * reach) / target.width, (y * reach) / target.height);
        draw(programs.blur, target);
        source = target;
      });

      const composite = programs.composite;
      gl.useProgram(composite.handle);
      bind(0, current.texture);
      bind(1, source.texture);
      bind(2, burn);
      gl.uniform1i(composite.uniform("uImage"), 0);
      gl.uniform1i(composite.uniform("uBloom"), 1);
      gl.uniform1i(composite.uniform("uBurnImage"), 2);
      gl.uniform1f(composite.uniform("uBurn"), frame.burn);
      gl.uniform2f(composite.uniform("uResolution"), canvas.width, canvas.height);
      gl.uniform3fv(composite.uniform("uTint"), frame.tint);
      gl.uniform3fv(composite.uniform("uHot"), frame.hot);
      gl.uniform3fv(composite.uniform("uGround"), frame.ground);
      gl.uniform1f(composite.uniform("uColor"), frame.color);
      gl.uniform1f(composite.uniform("uFringe"), frame.fringe);
      gl.uniform1f(composite.uniform("uTime"), frame.time);
      gl.uniform1f(composite.uniform("uPower"), frame.power);
      gl.uniform1f(composite.uniform("uStatic"), frame.staticAmount);
      gl.uniform1f(composite.uniform("uRoll"), frame.roll);
      gl.uniform1f(composite.uniform("uLines"), frame.lines);
      gl.uniform1f(composite.uniform("uMotion"), frame.motion);
      draw(composite, null);
    },
  };
}
