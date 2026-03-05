const canvas = document.getElementById("app");
const gl = canvas.getContext("webgl2", {
  antialias: false,
  alpha: false,
  depth: false,
  stencil: false,
  premultipliedAlpha: false,
  powerPreference: "high-performance",
});

if (!gl) {
  document.body.innerHTML =
    "<p style='padding:16px;color:#fff'>WebGL2 is required for this demo.</p>";
  throw new Error("WebGL2 unavailable");
}

const colorBufferFloatExt = gl.getExtension("EXT_color_buffer_float");
const maxTextureSize = gl.getParameter(gl.MAX_TEXTURE_SIZE) || 2048;
const PARTICLE_COUNT_MULTIPLIER = 3;
const PARTICLE_RESOLUTION_SCALE = Math.sqrt(PARTICLE_COUNT_MULTIPLIER);

const QUALITY_PRESETS = {
  auto: {
    maxDesktopTex: 576,
    maxMobileTex: 416,
    trailScale: 0.62,
    pointScaleMul: 1.0,
  },
  ultra: {
    maxDesktopTex: 768,
    maxMobileTex: 512,
    trailScale: 0.74,
    pointScaleMul: 0.94,
  },
  insane: {
    maxDesktopTex: 960,
    maxMobileTex: 640,
    trailScale: 0.84,
    pointScaleMul: 0.88,
  },
};

const FX_PRESETS = {
  neon: {
    mode: 0,
    trailWarp: 0.04,
    trailGhost: 0.12,
    trailSparkle: 0.35,
    trailDecayLift: 0.0,
    bloom: 1.0,
    chroma: 0.0,
    grain: 0.08,
    bgPulse: 0.18,
    particleSpark: 0.38,
    ringGain: 0.35,
    flare: 0.28,
    alphaGain: 0.92,
  },
  prism: {
    mode: 1,
    trailWarp: 0.12,
    trailGhost: 0.35,
    trailSparkle: 0.8,
    trailDecayLift: -0.004,
    bloom: 1.22,
    chroma: 0.55,
    grain: 0.22,
    bgPulse: 0.32,
    particleSpark: 0.82,
    ringGain: 0.72,
    flare: 0.58,
    alphaGain: 1.05,
  },
  plasma: {
    mode: 2,
    trailWarp: 0.18,
    trailGhost: 0.5,
    trailSparkle: 1.0,
    trailDecayLift: -0.008,
    bloom: 1.34,
    chroma: 0.24,
    grain: 0.3,
    bgPulse: 0.52,
    particleSpark: 1.0,
    ringGain: 0.88,
    flare: 0.82,
    alphaGain: 1.12,
  },
};

const state = {
  time: 0,
  lastTime: 0,
  dpr: 1,
  width: 1,
  height: 1,
  mood: "fluid",
  brush: "vortex",
  particleTexSize: 256,
  particleAmount: 50,
  particleCount: 256 * 256,
  pointer: {
    x: 0,
    y: 0,
    nx: 0,
    ny: 0,
    vx: 0,
    vy: 0,
    active: false,
    down: false,
    lastSeen: 0,
    radius: 0.18,
    strength: 1.0,
  },
  attractor: {
    enabled: false,
    touchPinned: false,
    x: 0,
    y: 0,
    mass: 1.8,
    spin: 0.45,
  },
  wormhole: {
    active: false,
    ax: -0.4,
    ay: 0.0,
    bx: 0.4,
    by: 0.0,
  },
  shape: {
    text: "TOUCH!",
    layout: "single",
    mix: 1,
    targetMix: 1,
    releaseAt: 0,
    duration: 2.3,
    tapHoldDuration: 0.9,
    dirty: true,
  },
  color: {
    hex: "#9bffb3",
    rgb: [0.6078, 1.0, 0.702],
  },
  colorMode: "auto",  // "auto" or a specific color
  colorCycle: {
    enabled: true,
    colors: [
      [0.6078, 1.0, 0.702],    // Mint
      [0.4, 0.8274, 1.0],      // Ice blue
      [1.0, 0.8274, 0.416],    // Gold
      [1.0, 0.557, 0.659],     // Rose
      [0.6, 0.4, 1.0],         // Purple
      [1.0, 0.6, 0.2],         // Orange
    ],
    currentIndex: 0,
    nextIndex: 1,
    mix: 0,
    rgb: [0.6078, 1.0, 0.702],
  },
  // Static color presets for manual mode
  colorPresets: {
    mint: [0.6078, 1.0, 0.702],
    ice: [0.4, 0.8274, 1.0],
    gold: [1.0, 0.8274, 0.416],
    rose: [1.0, 0.557, 0.659],
    purple: [0.6, 0.4, 1.0],
    orange: [1.0, 0.6, 0.2],
  },
  charging: false,
  chargeLevel: 0,
  perf: {
    quality: "auto",
  },
  fx: {
    mode: "neon",
  },
  stats: {
    fps: 60,
    sampleAccum: 0,
    frameAccum: 0,
    samples: 0,
    lastUiUpdate: 0,
  },
  rustUi: {
    enabled: false,
    ready: false,
    loading: false,
    error: "",
    initPromise: null,
  },
};

function clamp(v, a, b) {
  return Math.max(a, Math.min(b, v));
}

function hexToRgb01(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex || "");
  if (!m) return [0.6078, 1.0, 0.702];
  const n = parseInt(m[1], 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

function isLikelyMobile() {
  return window.matchMedia?.("(max-width: 820px), (pointer: coarse)")?.matches ?? window.innerWidth <= 820;
}

function getQualityPreset() {
  return QUALITY_PRESETS[state.perf.quality] || QUALITY_PRESETS.auto;
}

function getFxPreset() {
  return FX_PRESETS[state.fx.mode] || FX_PRESETS.neon;
}

function formatParticleCount(count) {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(2)}M`;
  if (count >= 1000) return `${(count / 1000).toFixed(count >= 100_000 ? 0 : 1)}k`;
  return `${count}`;
}

function chooseParticleTexSize() {
  const area = window.innerWidth * window.innerHeight * Math.min(window.devicePixelRatio || 1, 2);
  const q = getQualityPreset();
  const baseCap = Math.max(128, Math.min(maxTextureSize, isLikelyMobile() ? q.maxMobileTex : q.maxDesktopTex));
  const cap = Math.max(128, Math.min(maxTextureSize, Math.floor(baseCap * PARTICLE_RESOLUTION_SCALE)));
  const ladder = [128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640, 704, 768, 832, 896, 960];
  
  // Use particle slider value to determine base target size
  // slider 1-100 maps to texture size range
  const sliderFraction = Math.max(0, Math.min(1, state.particleAmount / 100));
  const minSize = 128;
  const maxSize = 960;
  // Quadratic curve for more natural feel - low values give fewer particles, high values give more
  const sliderBasedSize = Math.round(minSize + (maxSize - minSize) * sliderFraction * sliderFraction);
  
  let target = sliderBasedSize;
  
  // Also consider screen area as a minimum baseline
  const areaBasedTarget = (() => {
    if (area < 320_000) return 160;
    if (area < 650_000) return 224;
    if (area < 1_150_000) return 320;
    if (area < 1_900_000) return 384;
    if (area < 2_800_000) return 448;
    if (area < 3_800_000) return 512;
    if (area < 5_200_000) return 576;
    if (area < 6_800_000) return 640;
    if (area < 8_500_000) return 704;
    return 768;
  })();
  
  // Take the larger of slider-based or area-based size, capped by quality
  target = Math.max(target, areaBasedTarget);

  target = Math.round(target * PARTICLE_RESOLUTION_SCALE);

  if (state.perf.quality === "auto") target = Math.min(target, Math.round(576 * PARTICLE_RESOLUTION_SCALE));
  if (state.perf.quality === "ultra") target = Math.min(target, Math.round(768 * PARTICLE_RESOLUTION_SCALE));
  if (state.perf.quality === "insane" && area > 4_200_000) target = 896;
  if (state.perf.quality === "insane" && area > 7_000_000) target = 960;

  for (let i = ladder.length - 1; i >= 0; i--) {
    if (ladder[i] <= cap && ladder[i] <= target) return ladder[i];
  }
  return Math.min(cap, 128);
}

function createShader(type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const info = gl.getShaderInfoLog(shader);
    gl.deleteShader(shader);
    throw new Error(info || "Shader compile failed");
  }
  return shader;
}

function createProgram(vsSource, fsSource) {
  const program = gl.createProgram();
  const vs = createShader(gl.VERTEX_SHADER, vsSource);
  const fs = createShader(gl.FRAGMENT_SHADER, fsSource);
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const info = gl.getProgramInfoLog(program);
    gl.deleteProgram(program);
    throw new Error(info || "Program link failed");
  }
  return program;
}

const uniformCache = new WeakMap();
function getUniform(program, name) {
  let map = uniformCache.get(program);
  if (!map) {
    map = new Map();
    uniformCache.set(program, map);
  }
  if (!map.has(name)) {
    map.set(name, gl.getUniformLocation(program, name));
  }
  return map.get(name);
}

function createTexture(w, h, {
  internalFormat,
  format,
  type,
  min = gl.NEAREST,
  mag = gl.NEAREST,
  wrap = gl.CLAMP_TO_EDGE,
} = {}) {
  const tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, min);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, mag);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
  gl.texImage2D(gl.TEXTURE_2D, 0, internalFormat, w, h, 0, format, type, null);
  gl.bindTexture(gl.TEXTURE_2D, null);
  return tex;
}

function createFramebuffer(colorTextures) {
  const fbo = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
  const attachments = [];
  colorTextures.forEach((tex, i) => {
    const attachment = gl.COLOR_ATTACHMENT0 + i;
    gl.framebufferTexture2D(gl.FRAMEBUFFER, attachment, gl.TEXTURE_2D, tex, 0);
    attachments.push(attachment);
  });
  if (attachments.length > 1) gl.drawBuffers(attachments);
  const status = gl.checkFramebufferStatus(gl.FRAMEBUFFER);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  if (status !== gl.FRAMEBUFFER_COMPLETE) {
    throw new Error(`Framebuffer incomplete: ${status}`);
  }
  return fbo;
}

const quadVAO = gl.createVertexArray();
gl.bindVertexArray(quadVAO);
gl.bindVertexArray(null);

const fullScreenVS = `#version 300 es
precision highp float;
const vec2 POS[3] = vec2[3](
  vec2(-1.0, -1.0),
  vec2( 3.0, -1.0),
  vec2(-1.0,  3.0)
);
out vec2 vUv;
void main() {
  vec2 p = POS[gl_VertexID];
  vUv = p * 0.5 + 0.5;
  gl_Position = vec4(p, 0.0, 1.0);
}`;

const initFS = `#version 300 es
precision highp float;
layout(location = 0) out vec4 outPos;
layout(location = 1) out vec4 outVel;
uniform vec2 uResolution;
uniform float uSeed;

float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

void main() {
  vec2 id = floor(gl_FragCoord.xy - 0.5);
  vec2 uv = (id + 0.5) / uResolution;
  float a = hash12(uv + uSeed);
  float b = hash12(uv.yx + uSeed * 1.37);
  float c = hash12(uv + 0.71 + uSeed * 0.17);
  float angle = a * 6.2831853;
  float radius = pow(b, 0.75) * 0.82;
  vec2 pos = vec2(cos(angle), sin(angle)) * radius;

  // Elastic anchors use a softly warped grid rather than pure randomness.
  vec2 g = uv * 2.0 - 1.0;
  g += 0.12 * vec2(sin(uv.y * 18.0 + a * 4.0), cos(uv.x * 17.0 + b * 4.0));

  vec2 vel = 0.02 * vec2(cos(angle + 1.2), sin(angle - 0.7));
  outPos = vec4(pos, g);
  outVel = vec4(vel, a, b);
}`;

const simFS = `#version 300 es
precision highp float;
layout(location = 0) out vec4 outPos;
layout(location = 1) out vec4 outVel;
in vec2 vUv;

uniform sampler2D uPosTex;
uniform sampler2D uVelTex;
uniform sampler2D uShapeTargetTex;
uniform vec2 uStateResolution;
uniform float uTime;
uniform float uDt;
uniform int uMood;
uniform int uBrushMode;
uniform vec4 uPointer;   // xy pos, z radius, w active
uniform vec4 uPointerV;  // xy velocity, z strength, w down
uniform vec4 uAttractor; // xy pos, z mass, w enabled
uniform vec2 uAttractorSpin; // x spin, y reserved
uniform vec4 uWormA;     // xy pos, z active, w radius
uniform vec4 uWormB;     // xy pos, z active, w radius
uniform vec2 uViewport;
uniform vec4 uShape;     // x mix, y pull, z orbit, w active

float saturate(float x) { return clamp(x, 0.0, 1.0); }

float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float a = hash12(i);
  float b = hash12(i + vec2(1.0, 0.0));
  float c = hash12(i + vec2(0.0, 1.0));
  float d = hash12(i + vec2(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

vec2 fluidField(vec2 p, float t, float seed) {
  vec2 q = p * 2.4 + vec2(t * 0.08, -t * 0.06) + seed;
  float n1 = noise(q + vec2(0.0, 1.7));
  float n2 = noise(q + vec2(1.9, 0.0));
  vec2 grad = vec2(n1 - 0.5, n2 - 0.5);
  vec2 curlish = vec2(-grad.y, grad.x);
  curlish += 0.35 * vec2(sin(p.y * 6.0 + t * 0.9), cos(p.x * 5.0 - t * 1.1));
  return curlish;
}

void applyMagnet(inout vec2 acc, vec2 p, vec2 center, float mass, float spin, float falloffBias) {
  vec2 d = center - p;
  float r2 = dot(d, d) + falloffBias;
  float inv = 1.0 / r2;
  vec2 t = vec2(-d.y, d.x);
  acc += d * (mass * inv);
  acc += t * (spin * mass * inv * 0.9);
}

void main() {
  vec4 posData = texture(uPosTex, vUv);
  vec4 velData = texture(uVelTex, vUv);
  vec2 p = posData.xy;
  vec2 home = posData.zw;
  vec2 v = velData.xy;
  float seed = velData.z;
  float phase = velData.w;
  float dt = min(uDt, 0.033);
  float aspect = uViewport.x / max(uViewport.y, 1.0);
  vec2 acc = vec2(0.0);
  float shapeMix = saturate(uShape.x);
  float magneticShapeDampen = mix(1.0, 0.62, shapeMix);

  if (uMood == 0) {
    // Fluid: divergence-light curl-ish field with gentle center bias.
    acc += fluidField(p, uTime, seed) * 0.55;
    acc += -p * 0.08;
  } else if (uMood == 1) {
    // Magnetic: global orbit traps + optional user attractor.
    applyMagnet(acc, p, vec2(0.0), 0.12 * magneticShapeDampen, 0.54, 0.06);
    vec2 c2 = vec2(0.45 * sin(uTime * 0.31), 0.35 * cos(uTime * 0.27));
    applyMagnet(acc, p, c2, 0.075 * magneticShapeDampen, -0.72, 0.04);
  } else {
    // Elastic: spring to anchors plus traveling wave.
    vec2 target = home;
    target += 0.08 * vec2(
      sin(uTime * 0.8 + home.y * 7.0 + seed * 6.2831),
      cos(uTime * 0.7 + home.x * 8.0 + phase * 6.2831)
    );
    target.x *= aspect;
    vec2 d = target - p;
    acc += d * 2.3;
    acc += fluidField(p * 0.8, uTime * 0.5, phase) * 0.12;
  }

  if (uAttractor.w > 0.5) {
    applyMagnet(acc, p, uAttractor.xy, uAttractor.z, uAttractorSpin.x, 0.025);
  }

  if (uPointer.w > 0.5) {
    vec2 d = p - uPointer.xy;
    float r = max(uPointer.z, 0.001);
    float dist = length(d);
    float fall = exp(-pow(dist / r, 2.0) * 2.5);
    vec2 dir = dist > 0.0001 ? d / dist : vec2(1.0, 0.0);
    float brushStrength = (0.65 + length(uPointerV.xy) * 0.8) * uPointerV.z;
    if (uBrushMode == 0) {
      acc += dir * brushStrength * fall * 1.4;
    } else if (uBrushMode == 1) {
      acc += -dir * brushStrength * fall * 1.4;
    } else {
      acc += vec2(-dir.y, dir.x) * brushStrength * fall * 1.7;
      acc += uPointerV.xy * fall * 0.5;
    }
  }

  if (uWormA.z > 0.5 && uWormB.z > 0.5) {
    vec2 da = p - uWormA.xy;
    vec2 db = p - uWormB.xy;
    float ra = uWormA.w;
    float rb = uWormB.w;
    float fa = exp(-dot(da, da) / max(ra * ra, 1e-4) * 2.2);
    float fb = exp(-dot(db, db) / max(rb * rb, 1e-4) * 2.2);
    acc += (uWormB.xy - p) * fa * 1.8;
    acc += (uWormA.xy - p) * fb * 1.8;
    acc += vec2(-da.y, da.x) * fa * 0.9;
    acc += vec2(db.y, -db.x) * fb * 0.9;
  }

  if (uShape.w > 0.5) {
    vec2 target = texture(uShapeTargetTex, vUv).xy;
    target.x *= aspect;
    vec2 d = target - p;
    float dist2 = dot(d, d);
    float fall = exp(-dist2 * 4.0);
    acc += d * (uShape.x * uShape.y);
    acc += vec2(-d.y, d.x) * (uShape.x * uShape.z * (0.2 + 0.8 * fall));
    v *= mix(1.0, 0.965, saturate(uShape.x) * fall);
  }

  // Subtle damping and speed clamp keep the field readable and stable.
  v += acc * dt;
  float moodDamping = (uMood == 2) ? 0.92 : 0.975;
  v *= pow(moodDamping, dt * 60.0);
  float speed = length(v);
  float maxSpeed = (uMood == 1) ? 1.35 : 1.05;
  if (speed > maxSpeed) v *= maxSpeed / speed;
  p += v * dt;

  // Containment with soft bounce to keep particles on-screen.
  vec2 bounds = vec2(1.03 * aspect, 1.03);
  for (int i = 0; i < 2; i++) {
    if (p[i] > bounds[i]) { p[i] = bounds[i]; v[i] *= -0.68; }
    if (p[i] < -bounds[i]) { p[i] = -bounds[i]; v[i] *= -0.68; }
  }

  outPos = vec4(p, home);
  outVel = vec4(v, seed, phase);
}`;

const trailDecayFS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uTrail;
uniform vec2 uInvResolution;
uniform float uDecay;
uniform float uTime;
uniform vec4 uFx; // x warp, y ghost, z sparkle, w decay lift
void main() {
  vec2 px = uInvResolution;
  vec2 drift = vec2(
    sin(uTime * 0.67 + vUv.y * 13.0),
    cos(uTime * 0.73 + vUv.x * 11.0)
  ) * (px * (1.0 + 8.0 * uFx.x));
  vec2 uv = clamp(vUv + drift * uFx.x, px * 1.5, vec2(1.0) - px * 1.5);
  vec4 c = texture(uTrail, uv) * 0.55;
  c += texture(uTrail, uv + vec2(px.x, 0.0)) * 0.12;
  c += texture(uTrail, uv - vec2(px.x, 0.0)) * 0.12;
  c += texture(uTrail, uv + vec2(0.0, px.y)) * 0.10;
  c += texture(uTrail, uv - vec2(0.0, px.y)) * 0.10;
  if (uFx.y > 0.001) {
    vec2 ghostOffset = vec2(px.x * (2.0 + 8.0 * uFx.y), -px.y * (1.0 + 5.0 * uFx.y));
    c += texture(uTrail, clamp(uv + ghostOffset, px * 1.5, vec2(1.0) - px * 1.5)) * (0.04 + 0.10 * uFx.y);
  }
  c.rgb *= clamp(uDecay + uFx.w, 0.88, 0.995);
  c.rgb += (0.0018 + 0.0032 * uFx.z) * vec3(
    sin(uTime * 0.7 + uv.x * 11.0),
    sin(uTime * 0.9 + uv.y * 13.0 + 1.4),
    sin(uTime * 1.1 + (uv.x + uv.y) * 9.0 + 2.3)
  );
  outColor = vec4(max(c.rgb, 0.0), 1.0);
}`;

const compositeFS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uTrail;
uniform vec2 uResolution;
uniform float uTime;
uniform vec3 uTint;
uniform vec4 uFx; // x bloom, y chroma, z grain, w bg pulse
uniform int uFxMode;

float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

void main() {
  vec2 px = 1.0 / max(uResolution, vec2(1.0));
  vec3 trail = texture(uTrail, vUv).rgb;
  if (uFx.y > 0.001) {
    float shift = uFx.y * (0.75 + 0.25 * sin(uTime * 0.7 + vUv.y * 20.0));
    trail.r = texture(uTrail, clamp(vUv + vec2(px.x * (2.0 + 4.0 * shift), 0.0), px, vec2(1.0) - px)).r;
    trail.b = texture(uTrail, clamp(vUv - vec2(px.x * (1.5 + 3.5 * shift), 0.0), px, vec2(1.0) - px)).b;
  }
  // Soft-knee rolloff preserves color detail in dense areas instead of clipping to white.
  vec3 trailSoft = trail / (1.0 + trail * (0.55 + 0.45 * uFx.x));
  vec3 bloomish = trailSoft * (0.56 + 0.14 * smoothstep(0.08, 0.8, max(max(trailSoft.r, trailSoft.g), trailSoft.b))) * uFx.x;
  bloomish += pow(max(trailSoft - 0.08, 0.0), vec3(0.85)) * (0.10 + 0.16 * uFx.x);
  vec3 color = bloomish;
  color = mix(color, color * (0.75 + 0.55 * uTint), 0.10 + 0.14 * uFx.w);
  if (uFx.z > 0.001) {
    float grain = hash12(vUv * uResolution + fract(uTime * 60.0));
    color += (grain - 0.5) * (0.006 + 0.02 * uFx.z);
  }
  color = color / (1.0 + color * 0.75);
  color = pow(color, vec3(0.95));
  outColor = vec4(color, 1.0);
}`;

const particleVS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aUv;
uniform sampler2D uPosTex;
uniform sampler2D uVelTex;
uniform vec2 uResolution;
uniform float uPointScale;
out vec4 vData;
void main() {
  vec4 p = texture(uPosTex, aUv);
  vec4 v = texture(uVelTex, aUv);
  vec2 pos = p.xy;
  pos.x *= uResolution.y / max(uResolution.x, 1.0);
  gl_Position = vec4(pos, 0.0, 1.0);
  float speed = length(v.xy);
  gl_PointSize = uPointScale * (1.0 + speed * 3.4);
  vData = vec4(v.xy, v.z, speed);
}`;

const particleFS = `#version 300 es
precision highp float;
in vec4 vData;
out vec4 outColor;
uniform float uTime;
uniform int uMood;
uniform vec3 uTint;
uniform vec3 uTint2;
uniform float uTintMix;
uniform vec4 uFx; // x spark, y ring, z flare, w alpha
uniform int uFxMode;
void main() {
  vec2 p = gl_PointCoord * 2.0 - 1.0;
  float r2 = dot(p, p);
  if (r2 > 1.0) discard;
  float core = exp(-r2 * 5.0);
  float ring = exp(-abs(sqrt(r2) - 0.45) * 10.0);
  float flare = pow(max(0.0, 1.0 - abs(p.x * p.y) * 18.0), 3.0) * uFx.z;
  flare += pow(max(0.0, 1.0 - abs(p.x) * 2.8), 8.0) * uFx.z * 0.18;
  flare += pow(max(0.0, 1.0 - abs(p.y) * 2.8), 8.0) * uFx.z * 0.18;
  float speed = vData.w;
  float hueShift = fract(vData.z + speed * 0.22 + uTime * 0.02);
  vec3 baseA = vec3(0.08, 0.7, 1.0);
  vec3 baseB = vec3(0.12, 1.0, 0.65);
  vec3 baseC = vec3(1.0, 0.4, 0.18);
  vec3 col = mix(baseA, baseB, smoothstep(0.1, 0.8, hueShift));
  col = mix(col, baseC, smoothstep(0.7, 1.0, hueShift));
  if (uMood == 1) col = mix(col, vec3(1.0, 0.85, 0.35), 0.3);
  if (uMood == 2) col = mix(col, vec3(0.7, 0.75, 1.0), 0.25);
  if (uFxMode == 1) col = mix(col, vec3(0.8, 0.6, 1.0), 0.18 * (0.5 + 0.5 * sin(uTime + vData.z * 6.2831)));
  if (uFxMode == 2) col = mix(col, vec3(1.0, 0.45, 0.16), 0.16 + 0.14 * smoothstep(0.0, 1.2, speed));
  // Blend between two tint colors based on time
  vec3 tint = mix(uTint, uTint2, uTintMix);
  col = mix(col, tint, 0.72);
  float alpha = core * (0.48 + 0.12 * uFx.w) + ring * (0.08 + 0.14 * uFx.y);
  alpha += flare * (0.06 + 0.12 * uFx.x);
  float outAlpha = clamp(alpha * (0.86 + 0.18 * uFx.w), 0.0, 0.88);
  float drive = 0.44 + speed * (0.58 + 0.28 * uFx.x);
  drive = drive / (1.0 + drive * (0.45 + 0.25 * uFx.x));
  float glow = 0.75 + 0.65 * drive;
  outColor = vec4(col * outAlpha * glow, outAlpha);
}`;

const copyFS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uTex;
void main() { outColor = texture(uTex, vUv); }`;

const programs = {
  init: createProgram(fullScreenVS, initFS),
  sim: createProgram(fullScreenVS, simFS),
  trailDecay: createProgram(fullScreenVS, trailDecayFS),
  composite: createProgram(fullScreenVS, compositeFS),
  particle: createProgram(particleVS, particleFS),
  copy: createProgram(fullScreenVS, copyFS),
};

let particleResources = null;
let trailResources = null;
let shapeTargetTex = null;
const shapeCanvas = document.createElement("canvas");
shapeCanvas.width = 768;
shapeCanvas.height = 384;
const shapeCtx = shapeCanvas.getContext("2d", { willReadFrequently: true });

function buildParticleVertexData(size) {
  const count = size * size;
  const uv = new Float32Array(count * 2);
  let k = 0;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      uv[k++] = (x + 0.5) / size;
      uv[k++] = (y + 0.5) / size;
    }
  }
  return uv;
}

function createParticleResources(size) {
  const posA = createTexture(size, size, {
    internalFormat: gl.RGBA32F,
    format: gl.RGBA,
    type: gl.FLOAT,
  });
  const velA = createTexture(size, size, {
    internalFormat: gl.RGBA32F,
    format: gl.RGBA,
    type: gl.FLOAT,
  });
  const posB = createTexture(size, size, {
    internalFormat: gl.RGBA32F,
    format: gl.RGBA,
    type: gl.FLOAT,
  });
  const velB = createTexture(size, size, {
    internalFormat: gl.RGBA32F,
    format: gl.RGBA,
    type: gl.FLOAT,
  });

  const fboA = createFramebuffer([posA, velA]);
  const fboB = createFramebuffer([posB, velB]);

  const vao = gl.createVertexArray();
  const vbo = gl.createBuffer();
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
  gl.bufferData(gl.ARRAY_BUFFER, buildParticleVertexData(size), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);

  return {
    size,
    count: size * size,
    buffers: [
      { pos: posA, vel: velA, fbo: fboA },
      { pos: posB, vel: velB, fbo: fboB },
    ],
    readIndex: 0,
    vao,
    vbo,
  };
}

function destroyParticleResources(res) {
  if (!res) return;
  for (const b of res.buffers) {
    gl.deleteTexture(b.pos);
    gl.deleteTexture(b.vel);
    gl.deleteFramebuffer(b.fbo);
  }
  gl.deleteBuffer(res.vbo);
  gl.deleteVertexArray(res.vao);
}

function ensureShapeTargetTexture(size) {
  if (shapeTargetTex && shapeTargetTex.size === size) return;
  if (shapeTargetTex) gl.deleteTexture(shapeTargetTex.tex);
  shapeTargetTex = {
    tex: createTexture(size, size, {
      internalFormat: gl.RGBA32F,
      format: gl.RGBA,
      type: gl.FLOAT,
    }),
    size,
  };
  state.shape.dirty = true;
}

function rebuildShapeTargetTexture() {
  if (!particleResources || !shapeTargetTex || !shapeCtx) return;
  if (!state.shape.dirty) return;

  const text = ((state.shape.text || "RUSTY PARTS").trim() || "RUSTY PARTS").toUpperCase();
  const cw = shapeCanvas.width;
  const ch = shapeCanvas.height;
  shapeCtx.clearRect(0, 0, cw, ch);
  shapeCtx.fillStyle = "#fff";
  shapeCtx.textAlign = "center";
  shapeCtx.textBaseline = "middle";
  const fontFamily = '"Arial Black", "Segoe UI", sans-serif';

  if (state.shape.layout === "multi") {
    const words = text.split(/\s+/).filter(Boolean);
    if (!words.length) words.push("RUSTY", "PARTS");
    const count = clamp(words.length * 2 + 1, 6, 12);
    const maxLen = Math.max(...words.map((w) => w.length), 1);
    const base = Math.floor(Math.min(cw, ch) * (maxLen > 9 ? 0.11 : 0.14));
    const placements = [];

    for (let i = 0; i < count; i++) {
      const word = words[(i + ((Math.random() * words.length) | 0)) % words.length];
      for (let attempt = 0; attempt < 16; attempt++) {
        const fontSize = clamp(Math.round(base * (0.62 + Math.random() * 0.95)), 24, 128);
        shapeCtx.font = `800 ${fontSize}px ${fontFamily}`;
        const m = shapeCtx.measureText(word);
        const textW = Math.max(40, m.width);
        const textH = Math.max(
          fontSize,
          (m.actualBoundingBoxAscent || fontSize * 0.8) + (m.actualBoundingBoxDescent || fontSize * 0.2),
        );
        const padX = Math.min(cw * 0.45, textW * 0.62 + 20);
        const padY = Math.min(ch * 0.4, textH * 0.72 + 16);
        const x = clamp(padX + Math.random() * Math.max(1, cw - padX * 2), 0, cw);
        const y = clamp(padY + Math.random() * Math.max(1, ch - padY * 2), 0, ch);
        const radius = Math.max(textW * 0.5, textH * 0.95);

        let overlaps = false;
        for (const p of placements) {
          const dx = x - p.x;
          const dy = y - p.y;
          if (dx * dx + dy * dy < (radius + p.radius) * (radius + p.radius) * 1.05) {
            overlaps = true;
            break;
          }
        }
        if (overlaps && attempt < 15) continue;

        placements.push({
          x,
          y,
          word,
          radius,
          fontSize,
          angle: (Math.random() - 0.5) * 1.2,
        });
        break;
      }
    }

    shapeCtx.strokeStyle = "rgba(255,255,255,0.95)";
    shapeCtx.fillStyle = "#fff";
    shapeCtx.lineJoin = "round";
    shapeCtx.lineCap = "round";
    for (const p of placements) {
      shapeCtx.save();
      shapeCtx.translate(p.x, p.y);
      shapeCtx.rotate(p.angle);
      shapeCtx.font = `800 ${p.fontSize}px ${fontFamily}`;
      shapeCtx.lineWidth = Math.max(2, p.fontSize * 0.07);
      shapeCtx.strokeText(p.word, 0, 0);
      shapeCtx.fillText(p.word, 0, 0);
      shapeCtx.restore();
    }
  } else {
    const bigText = (text.split(/\s+/).filter(Boolean).join(" ") || "RUSTY PARTS").slice(0, 18);
    const maxW = cw * 0.95;
    const maxH = ch * 0.8;
    let lo = 32;
    let hi = Math.floor(ch * 0.9);
    let best = lo;

    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      shapeCtx.font = `800 ${mid}px ${fontFamily}`;
      const m = shapeCtx.measureText(bigText);
      const w = m.width;
      const h = (m.actualBoundingBoxAscent || mid * 0.8) + (m.actualBoundingBoxDescent || mid * 0.2);
      if (w <= maxW && h <= maxH) {
        best = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }

    shapeCtx.font = `800 ${best}px ${fontFamily}`;
    shapeCtx.fillText(bigText, cw * 0.5, ch * 0.54);
  }

  const img = shapeCtx.getImageData(0, 0, cw, ch).data;
  const samples = [];
  for (let y = 0; y < ch; y += 2) {
    for (let x = 0; x < cw; x += 2) {
      if (img[(y * cw + x) * 4 + 3] > 8) samples.push([x, y]);
    }
  }
  if (!samples.length) samples.push([cw * 0.5, ch * 0.5]);

  const size = particleResources.size;
  const count = size * size;
  const data = new Float32Array(count * 4);
  for (let i = 0; i < count; i++) {
    const s = samples[(i * 131 + ((i / 7) | 0) * 17) % samples.length];
    const x = (s[0] / (cw - 1)) * 2 - 1;
    const y = -((s[1] / (ch - 1)) * 2 - 1);
    const jx = (Math.sin(i * 12.9898) * 43758.5453 % 1) * 0.004;
    const jy = (Math.sin((i + 9) * 78.233) * 12345.6789 % 1) * 0.004;
    const sx = state.shape.layout === "single" ? 0.9 : 0.82;
    const sy = state.shape.layout === "single" ? 0.62 : 0.55;
    data[i * 4 + 0] = x * sx + jx;
    data[i * 4 + 1] = y * sy + jy;
    data[i * 4 + 2] = 0;
    data[i * 4 + 3] = 1;
  }

  gl.bindTexture(gl.TEXTURE_2D, shapeTargetTex.tex);
  gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, size, size, gl.RGBA, gl.FLOAT, data);
  gl.bindTexture(gl.TEXTURE_2D, null);
  state.shape.dirty = false;
}

function createTrailResources(w, h) {
  const supportsHalfFloat = !!colorBufferFloatExt;
  const internal = supportsHalfFloat ? gl.RGBA16F : gl.RGBA8;
  const type = supportsHalfFloat ? gl.HALF_FLOAT : gl.UNSIGNED_BYTE;
  const texA = createTexture(w, h, {
    internalFormat: internal,
    format: gl.RGBA,
    type,
    min: gl.LINEAR,
    mag: gl.LINEAR,
  });
  const texB = createTexture(w, h, {
    internalFormat: internal,
    format: gl.RGBA,
    type,
    min: gl.LINEAR,
    mag: gl.LINEAR,
  });
  const fboA = createFramebuffer([texA]);
  const fboB = createFramebuffer([texB]);
  return {
    width: w,
    height: h,
    buffers: [
      { tex: texA, fbo: fboA },
      { tex: texB, fbo: fboB },
    ],
    readIndex: 0,
  };
}

function destroyTrailResources(res) {
  if (!res) return;
  for (const b of res.buffers) {
    gl.deleteTexture(b.tex);
    gl.deleteFramebuffer(b.fbo);
  }
}

function runFullscreen(program) {
  gl.useProgram(program);
  gl.bindVertexArray(quadVAO);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  gl.bindVertexArray(null);
}

function initParticles() {
  const nextSize = chooseParticleTexSize();
  if (particleResources && particleResources.size === nextSize) return;
  destroyParticleResources(particleResources);
  particleResources = createParticleResources(nextSize);
  ensureShapeTargetTexture(nextSize);
  state.particleTexSize = nextSize;
  state.particleCount = particleResources.count;

  const seed = Math.random() * 1000;
  gl.viewport(0, 0, nextSize, nextSize);
  const initProgram = programs.init;
  for (const pass of particleResources.buffers) {
    gl.bindFramebuffer(gl.FRAMEBUFFER, pass.fbo);
    gl.useProgram(initProgram);
    gl.uniform2f(getUniform(initProgram, "uResolution"), nextSize, nextSize);
    gl.uniform1f(getUniform(initProgram, "uSeed"), seed);
    runFullscreen(initProgram);
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  rebuildShapeTargetTexture();
  updateParticleCountUi({ includeFps: false });
}

function initTrail() {
  const q = getQualityPreset();
  const w = Math.max(2, Math.floor(state.width * q.trailScale));
  const h = Math.max(2, Math.floor(state.height * q.trailScale));
  if (trailResources && trailResources.width === w && trailResources.height === h) return;
  destroyTrailResources(trailResources);
  trailResources = createTrailResources(w, h);

  gl.disable(gl.BLEND);
  for (const b of trailResources.buffers) {
    gl.bindFramebuffer(gl.FRAMEBUFFER, b.fbo);
    gl.viewport(0, 0, w, h);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
}

function resize() {
  state.dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.floor(window.innerWidth * state.dpr));
  const h = Math.max(1, Math.floor(window.innerHeight * state.dpr));
  state.width = w;
  state.height = h;
  canvas.width = w;
  canvas.height = h;
  canvas.style.width = `${window.innerWidth}px`;
  canvas.style.height = `${window.innerHeight}px`;
  initParticles();
  initTrail();
}

function moodIndex() {
  if (state.mood === "magnetic") return 1;
  return 0;
}

function brushIndex() {
  if (state.brush === "pull") return 1;
  if (state.brush === "vortex") return 2;
  return 0;
}

function normFromClient(clientX, clientY) {
  const rect = canvas.getBoundingClientRect();
  const x = (clientX - rect.left) / rect.width;
  const y = (clientY - rect.top) / rect.height;
  return {
    x,
    y,
    nx: (x * 2 - 1) * (rect.width / rect.height),
    ny: -(y * 2 - 1),
  };
}

function setPointerFromClient(clientX, clientY, now) {
  const prevX = state.pointer.nx;
  const prevY = state.pointer.ny;
  const p = normFromClient(clientX, clientY);
  state.pointer.x = p.x;
  state.pointer.y = p.y;
  state.pointer.nx = p.nx;
  state.pointer.ny = p.ny;
  if (state.pointer.lastSeen > 0) {
    const dt = Math.max((now - state.pointer.lastSeen) / 1000, 1 / 120);
    state.pointer.vx = (p.nx - prevX) / dt;
    state.pointer.vy = (p.ny - prevY) / dt;
  }
  state.pointer.lastSeen = now;
  state.pointer.active = true;
}

const TAP_TRIGGER_MAX_MS = 260;
const TAP_TRIGGER_MAX_MOVE_PX = 16;
const TOUCH_HOLD_FORM_MS = 280;
const mouseTap = { active: false, x: 0, y: 0, t: 0, moved: false };
const touchTapStarts = new Map();
let touchHoldFormTimer = 0;
let touchHoldFormTouchId = null;
let touchHoldFormActive = false;

function triggerPointerWordForm() {
  toggleShapeWord();
}

function setTouchAttractorPinned(enabled, clientX, clientY, now = performance.now()) {
  state.attractor.touchPinned = !!enabled;
  if (state.attractor.touchPinned && Number.isFinite(clientX) && Number.isFinite(clientY)) {
    const p = normFromClient(clientX, clientY);
    state.attractor.x = p.nx;
    state.attractor.y = p.ny;
  }
  if (!state.pointer.down && !state.wormhole.active && !touchHoldFormActive) {
    state.attractor.enabled = state.attractor.touchPinned;
  }
}

function clearTouchHoldFormTimer() {
  if (!touchHoldFormTimer) return;
  clearTimeout(touchHoldFormTimer);
  touchHoldFormTimer = 0;
  touchHoldFormTouchId = null;
}

function releaseTouchHoldForm({ melt = false } = {}) {
  clearTouchHoldFormTimer();
  if (touchHoldFormActive && melt) triggerShapeMelt();
  touchHoldFormActive = false;
  if (!state.pointer.down && !state.wormhole.active) {
    state.attractor.enabled = state.attractor.touchPinned;
  }
}

function armTouchHoldFormGesture() {
  if (touches.size !== 1 || touchHoldFormTimer || touchHoldFormActive) return;
  const firstTouch = touches.entries().next();
  if (firstTouch.done) return;
  const [touchId] = firstTouch.value;
  const start = touchTapStarts.get(touchId);
  if (!start || start.moved || start.multi) return;
  touchHoldFormTouchId = touchId;
  touchHoldFormTimer = window.setTimeout(() => {
    touchHoldFormTimer = 0;
    touchHoldFormTouchId = null;
    const activeTouch = touches.get(touchId);
    const activeStart = touchTapStarts.get(touchId);
    if (!activeTouch || touches.size !== 1) return;
    if (!activeStart || activeStart.moved || activeStart.multi) return;
    touchHoldFormActive = true;
    state.attractor.enabled = false;
    triggerShapeForm(0);
  }, TOUCH_HOLD_FORM_MS);
}

canvas.addEventListener("pointermove", (e) => {
  const now = performance.now();
  setPointerFromClient(e.clientX, e.clientY, now);
  if (mouseTap.active) {
    const dx = e.clientX - mouseTap.x;
    const dy = e.clientY - mouseTap.y;
    if (dx * dx + dy * dy > TAP_TRIGGER_MAX_MOVE_PX * TAP_TRIGGER_MAX_MOVE_PX) mouseTap.moved = true;
  }
  if (state.pointer.down) {
    state.attractor.x = state.pointer.nx;
    state.attractor.y = state.pointer.ny;
  }
});

canvas.addEventListener("pointerleave", () => {
  state.pointer.active = false;
  state.pointer.vx = 0;
  state.pointer.vy = 0;
  mouseTap.active = false;
});

canvas.addEventListener("pointerdown", (e) => {
  if (e.pointerType === "touch" || e.button !== 0) return;
  state.attractor.touchPinned = false;
  canvas.setPointerCapture(e.pointerId);
  const now = performance.now();
  setPointerFromClient(e.clientX, e.clientY, now);
  state.pointer.down = true;
  state.attractor.enabled = true;
  state.attractor.x = state.pointer.nx;
  state.attractor.y = state.pointer.ny;
  mouseTap.active = true;
  mouseTap.x = e.clientX;
  mouseTap.y = e.clientY;
  mouseTap.t = now;
  mouseTap.moved = false;
});

canvas.addEventListener("pointerup", (e) => {
  if (e.pointerType === "touch" || e.button !== 0) return;
  if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
  state.pointer.down = false;
  state.attractor.enabled = false;
  const dt = performance.now() - mouseTap.t;
  if (mouseTap.active && !mouseTap.moved && dt <= TAP_TRIGGER_MAX_MS) {
    triggerPointerWordForm();
  }
  mouseTap.active = false;
});

canvas.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  state.attractor.enabled = false;
});

canvas.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    const factor = Math.exp(-e.deltaY * 0.0012);
    state.attractor.mass = clamp(state.attractor.mass * factor, 0.08, 2.2);
  },
  { passive: false },
);

const touches = new Map();
canvas.addEventListener(
  "touchstart",
  (e) => {
    e.preventDefault();
    const now = performance.now();
    for (const t of e.changedTouches) {
      touches.set(t.identifier, { x: t.clientX, y: t.clientY, t: now });
      touchTapStarts.set(t.identifier, { x: t.clientX, y: t.clientY, t: now, moved: false, multi: false });
    }
    if (touches.size > 1) {
      for (const start of touchTapStarts.values()) start.multi = true;
    }
    updateTouchModes(now);
  },
  { passive: false },
);

canvas.addEventListener(
  "touchmove",
  (e) => {
    e.preventDefault();
    const now = performance.now();
    for (const t of e.changedTouches) {
      touches.set(t.identifier, { x: t.clientX, y: t.clientY, t: now });
      const start = touchTapStarts.get(t.identifier);
      if (start) {
        const dx = t.clientX - start.x;
        const dy = t.clientY - start.y;
        if (dx * dx + dy * dy > TAP_TRIGGER_MAX_MOVE_PX * TAP_TRIGGER_MAX_MOVE_PX) start.moved = true;
      }
    }
    if (touches.size > 1) {
      for (const start of touchTapStarts.values()) start.multi = true;
    }
    updateTouchModes(now);
  },
  { passive: false },
);

canvas.addEventListener(
  "touchend",
  (e) => {
    e.preventDefault();
    const now = performance.now();
    let tapToggle = null;
    for (const t of e.changedTouches) {
      if (touchHoldFormTouchId === t.identifier) clearTouchHoldFormTimer();
      const start = touchTapStarts.get(t.identifier);
      if (start) {
        const dx = t.clientX - start.x;
        const dy = t.clientY - start.y;
        const moved = start.moved || dx * dx + dy * dy > TAP_TRIGGER_MAX_MOVE_PX * TAP_TRIGGER_MAX_MOVE_PX;
        const dt = now - start.t;
        if (!start.multi && !moved && dt <= TAP_TRIGGER_MAX_MS && !touchHoldFormActive) {
          tapToggle = { x: t.clientX, y: t.clientY };
        }
        touchTapStarts.delete(t.identifier);
      }
      touches.delete(t.identifier);
    }
    updateTouchModes(now);
    if (tapToggle && touches.size === 0 && !state.wormhole.active) {
      toggleShapeWord();
    }
  },
  { passive: false },
);

canvas.addEventListener(
  "touchcancel",
  (e) => {
    e.preventDefault();
    const now = performance.now();
    for (const t of e.changedTouches) {
      if (touchHoldFormTouchId === t.identifier) clearTouchHoldFormTimer();
      touches.delete(t.identifier);
      touchTapStarts.delete(t.identifier);
    }
    updateTouchModes(now);
  },
  { passive: false },
);

function updateTouchModes(now) {
  const values = [...touches.values()];
  if (values.length >= 2) {
    releaseTouchHoldForm({ melt: true });
    const [a, b] = values;
    const an = normFromClient(a.x, a.y);
    const bn = normFromClient(b.x, b.y);
    state.wormhole.active = true;
    state.wormhole.ax = an.nx;
    state.wormhole.ay = an.ny;
    state.wormhole.bx = bn.nx;
    state.wormhole.by = bn.ny;
    state.pointer.active = false;
    state.attractor.enabled = false;
  } else if (values.length === 1) {
    const [a] = values;
    if (touchHoldFormTimer && !touches.has(touchHoldFormTouchId)) clearTouchHoldFormTimer();
    armTouchHoldFormGesture();
    state.wormhole.active = false;
    setPointerFromClient(a.x, a.y, now);
    state.pointer.active = !state.attractor.touchPinned && !touchHoldFormActive;
    if (state.attractor.touchPinned && !touchHoldFormActive) {
      state.attractor.x = state.pointer.nx;
      state.attractor.y = state.pointer.ny;
      state.attractor.enabled = true;
    } else {
      state.attractor.enabled = false;
    }
  } else {
    releaseTouchHoldForm({ melt: true });
    state.wormhole.active = false;
    state.pointer.active = false;
    state.attractor.enabled = state.attractor.touchPinned;
  }
}

function setActiveButton(seg, key, value) {
  if (!seg) return;
  for (const btn of seg.querySelectorAll("button")) {
    btn.classList.toggle("is-active", btn.dataset[key] === value);
  }
}

function updateShapeActionButtons() {
  const shaped = isShapeWordVisible();
  formBtn.classList.toggle("is-active", shaped);
  meltBtn.classList.toggle("is-active", !shaped);
}

function isTypingTarget(el) {
  if (!el) return false;
  if (el === shapeInput) return true;
  return (
    el instanceof HTMLElement &&
    (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/i.test(el.tagName))
  );
}

const brushSeg = document.getElementById("brushSeg");
const particleSlider = document.getElementById("particleSlider");
const particleCountOut = document.getElementById("particleCountOut");
const fxSeg = document.getElementById("fxSeg");
const shapeInput = document.getElementById("shapeInput");
const layoutSeg = document.getElementById("layoutSeg");
const formBtn = document.getElementById("formBtn");
const meltBtn = document.getElementById("meltBtn");
const controlPanel = document.getElementById("controlPanel");
const panelToggle = document.getElementById("panelToggle");
const rustUiBtn = document.getElementById("rustUiBtn");
const rustUiInlineStatus = document.getElementById("rustUiInlineStatus");
const rustUiDock = document.getElementById("rustUiDock");
const rustUiDockToggle = document.getElementById("rustUiDockToggle");
const rustUiDockNote = document.getElementById("rustUiDockNote");

function setRustUiStatus(text, status = "idle") {
  if (!rustUiInlineStatus) return;
  rustUiInlineStatus.textContent = text;
  rustUiInlineStatus.dataset.state = status;
}

function setRustUiNote(text) {
  if (!rustUiDockNote) return;
  rustUiDockNote.textContent = text;
}

function updateParticleCountUi({ includeFps = true } = {}) {
  if (!particleCountOut) return;
  const base = `${formatParticleCount(state.particleCount)} pts`;
  const fps = Math.max(0, Math.round(state.stats.fps || 0));
  const text = includeFps ? `${base} @ ${fps}fps` : base;
  particleCountOut.textContent = text;
  particleCountOut.value = text;
}

function setQualityMode(mode) {
  if (!QUALITY_PRESETS[mode]) return;
  state.perf.quality = mode;
  setActiveButton(qualitySeg, "quality", state.perf.quality);
  initParticles();
  initTrail();
  rebuildShapeTargetTexture();
  updateParticleCountUi({ includeFps: false });
}

function setFxMode(mode) {
  if (!FX_PRESETS[mode]) return;
  state.fx.mode = mode;
  setActiveButton(fxSeg, "fx", state.fx.mode);
}

async function ensureRustUiLoaded() {
  if (state.rustUi.ready) return true;
  if (state.rustUi.initPromise) return state.rustUi.initPromise;

  state.rustUi.loading = true;
  setRustUiStatus("Loading...", "idle");
  setRustUiNote("Loading Rust UI Micro Engine WebAssembly bundle...");

  state.rustUi.initPromise = (async () => {
    try {
      const mod = await import("./rust-ui-micro-engine/web/ui_micro_app.js");
      if (typeof mod.default === "function") {
        await mod.default();
      }
      state.rustUi.ready = true;
      state.rustUi.error = "";
      setRustUiStatus("Live", "ready");
      setRustUiNote("Rust UI Micro Engine is running in the dock canvas.");
      return true;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      state.rustUi.error = msg;
      state.rustUi.ready = false;
      setRustUiStatus("Build wasm", "error");
      setRustUiNote("Rust UI wasm bundle not found. Run scripts/build-rust-ui-web.sh, then reload.");
      console.warn("Rust UI Micro Engine load failed:", err);
      state.rustUi.initPromise = null;
      return false;
    } finally {
      state.rustUi.loading = false;
    }
  })();

  return state.rustUi.initPromise;
}

function setRustUiVisible(visible) {
  if (!rustUiBtn && !rustUiDock) return;
  state.rustUi.enabled = !!visible;
  if (rustUiDock) rustUiDock.hidden = !state.rustUi.enabled;
  if (rustUiBtn) {
    rustUiBtn.textContent = state.rustUi.enabled ? "Hide Micro UI" : "Open Micro UI";
    rustUiBtn.setAttribute("aria-pressed", state.rustUi.enabled ? "true" : "false");
  }
  if (state.rustUi.enabled) {
    void ensureRustUiLoaded();
  }
}

function setPanelCollapsed(collapsed) {
  if (!controlPanel || !panelToggle) return;
  controlPanel.classList.toggle("is-collapsed", collapsed);
  panelToggle.setAttribute("aria-expanded", collapsed ? "false" : "true");
  panelToggle.textContent = collapsed ? "Expand" : "Collapse";
}

if (panelToggle && controlPanel) {
  panelToggle.addEventListener("click", () => {
    setPanelCollapsed(!controlPanel.classList.contains("is-collapsed"));
  });
}

brushSeg.addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-brush]");
  if (!btn) return;
  state.brush = btn.dataset.brush;
  setActiveButton(brushSeg, "brush", state.brush);
});

if (qualitySeg) {
  qualitySeg.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-quality]");
    if (!btn) return;
    setQualityMode(btn.dataset.quality);
  });
}

if (fxSeg) {
  fxSeg.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-fx]");
    if (!btn) return;
    setFxMode(btn.dataset.fx);
  });
}

// Color mode toggle
if (colorSeg) {
  colorSeg.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-color]");
    if (!btn) return;
    const colorMode = btn.dataset.color;
    state.colorMode = colorMode;
    setActiveButton(colorSeg, "color", colorMode);
  });
}

if (rustUiBtn) {
  rustUiBtn.addEventListener("click", () => {
    setRustUiVisible(!state.rustUi.enabled);
  });
}

if (rustUiDockToggle) {
  rustUiDockToggle.addEventListener("click", () => {
    setRustUiVisible(false);
  });
}

layoutSeg.addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-layout]");
  if (!btn) return;
  state.shape.layout = btn.dataset.layout;
  state.shape.dirty = true;
  setActiveButton(layoutSeg, "layout", state.shape.layout);
});

function triggerShapeForm(holdDuration = state.shape.duration) {
  state.shape.text = ((shapeInput.value || "RUSTY PARTS").trim() || "RUSTY PARTS").slice(0, 18);
  shapeInput.value = state.shape.text;
  state.shape.dirty = true;
  rebuildShapeTargetTexture();
  state.mood = "magnetic";
  state.shape.targetMix = 1;
  state.shape.releaseAt = Number.isFinite(holdDuration) && holdDuration > 0 ? state.time + holdDuration : 0;
  updateShapeActionButtons();
}

function triggerShapeMelt() {
  state.mood = "fluid";
  state.shape.targetMix = 0;
  state.shape.releaseAt = 0;
  updateShapeActionButtons();
}

function isShapeWordVisible() {
  return state.shape.mix > 0.04 || state.shape.targetMix > 0.04;
}

function toggleShapeWord() {
  if (isShapeWordVisible()) {
    triggerShapeMelt();
    return;
  }
  triggerShapeForm(0);
}

function syncShapeTextFromInput({
  refreshTarget = false,
  sustainIfShaped = false,
  fallbackOnEmpty = true,
} = {}) {
  const normalized = ((shapeInput.value || "").trim()).slice(0, 18);
  state.shape.text = normalized || (fallbackOnEmpty ? "RUSTY PARTS" : "");
  if (fallbackOnEmpty || normalized) {
    shapeInput.value = state.shape.text;
  }
  state.shape.dirty = true;
  if (refreshTarget && (state.shape.mix > 0.01 || state.shape.targetMix > 0.01)) {
    rebuildShapeTargetTexture();
  }
  if (sustainIfShaped && (state.shape.mix > 0.04 || state.shape.targetMix > 0.04)) {
    state.shape.targetMix = 1;
    state.shape.releaseAt = state.time + state.shape.duration;
  }
  updateShapeActionButtons();
}

shapeInput.addEventListener("input", () => {
  if (isShapeWordVisible()) {
    triggerShapeMelt();
  }
  syncShapeTextFromInput({ fallbackOnEmpty: false });
});

shapeInput.addEventListener("change", () => {
  if (isShapeWordVisible()) {
    triggerShapeMelt();
  }
  syncShapeTextFromInput();
});

// Particle slider event - adjust particle amount
if (particleSlider) {
  particleSlider.addEventListener("input", () => {
    const val = parseInt(particleSlider.value, 10);
    state.particleAmount = val;
    // Map slider 1-100 to texture size
    // Higher slider = more particles = larger texture
    const baseSize = 128;
    const maxSize = 768;
    const fraction = val / 100;
    const newSize = Math.round(baseSize + (maxSize - baseSize) * fraction * fraction);
    state.perf.quality = "auto";
    // Force reinit with new size
    destroyParticleResources(particleResources);
    particleResources = null;
    initParticles();
    initTrail();
    rebuildShapeTargetTexture();
    updateParticleCountUi({ includeFps: false });
  });
}

shapeInput.addEventListener("keydown", (e) => {
  if (e.key !== "Enter") return;
  e.preventDefault();
  triggerShapeForm();
});

formBtn.addEventListener("click", triggerShapeForm);
meltBtn.addEventListener("click", triggerShapeMelt);

window.addEventListener("keydown", (e) => {
  if (isTypingTarget(e.target)) return;
  // Removed brush key handlers (Q, W, E) - brush is now vortex only
  if (e.key === "1") setFxMode("neon");
  if (e.key === "2") setFxMode("prism");
  if (e.key === "3") setFxMode("plasma");
  if (e.key.toLowerCase() === "z") setQualityMode("auto");
  if (e.key.toLowerCase() === "x") setQualityMode("ultra");
  if (e.key.toLowerCase() === "c") setQualityMode("insane");
  if (e.key.toLowerCase() === "m") triggerShapeMelt();
  if (e.key.toLowerCase() === "t") {
    state.shape.layout = state.shape.layout === "single" ? "multi" : "single";
    state.shape.dirty = true;
  }
  if (e.key === "Enter" && document.activeElement !== shapeInput) triggerShapeForm();
  // Removed brush-related setActiveButton calls
  setActiveButton(fxSeg, "fx", state.fx.mode);
  setActiveButton(layoutSeg, "layout", state.shape.layout);
  updateShapeActionButtons();
});

function simStep(dt) {
  const res = particleResources;
  const read = res.buffers[res.readIndex];
  const write = res.buffers[1 - res.readIndex];
  const simProgram = programs.sim;
  const fx = getFxPreset();

  gl.bindFramebuffer(gl.FRAMEBUFFER, write.fbo);
  gl.viewport(0, 0, res.size, res.size);
  gl.disable(gl.BLEND);

  gl.useProgram(simProgram);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, read.pos);
  gl.uniform1i(getUniform(simProgram, "uPosTex"), 0);
  gl.activeTexture(gl.TEXTURE1);
  gl.bindTexture(gl.TEXTURE_2D, read.vel);
  gl.uniform1i(getUniform(simProgram, "uVelTex"), 1);
  gl.activeTexture(gl.TEXTURE2);
  gl.bindTexture(gl.TEXTURE_2D, shapeTargetTex ? shapeTargetTex.tex : null);
  gl.uniform1i(getUniform(simProgram, "uShapeTargetTex"), 2);

  gl.uniform2f(getUniform(simProgram, "uStateResolution"), res.size, res.size);
  gl.uniform1f(getUniform(simProgram, "uTime"), state.time);
  gl.uniform1f(getUniform(simProgram, "uDt"), dt);
  gl.uniform1i(getUniform(simProgram, "uMood"), moodIndex());
  gl.uniform1i(getUniform(simProgram, "uBrushMode"), brushIndex());

  const pointerActive = state.pointer.active && state.time - state.pointer.lastSeen / 1000 < 0.15 && !state.wormhole.active;
  gl.uniform4f(
    getUniform(simProgram, "uPointer"),
    state.pointer.nx,
    state.pointer.ny,
    state.pointer.radius,
    pointerActive ? 1 : 0,
  );
  gl.uniform4f(
    getUniform(simProgram, "uPointerV"),
    state.pointer.vx * 0.02,
    state.pointer.vy * 0.02,
    state.pointer.strength,
    state.pointer.down ? 1 : 0,
  );
  gl.uniform4f(
    getUniform(simProgram, "uAttractor"),
    state.attractor.x,
    state.attractor.y,
    state.attractor.mass,
    state.attractor.enabled ? 1 : 0,
  );
  gl.uniform2f(getUniform(simProgram, "uAttractorSpin"), state.attractor.spin * (1 + fx.trailWarp * 0.2), 0);
  gl.uniform4f(
    getUniform(simProgram, "uWormA"),
    state.wormhole.ax,
    state.wormhole.ay,
    state.wormhole.active ? 1 : 0,
    0.18,
  );
  gl.uniform4f(
    getUniform(simProgram, "uWormB"),
    state.wormhole.bx,
    state.wormhole.by,
    state.wormhole.active ? 1 : 0,
    0.18,
  );
  gl.uniform2f(getUniform(simProgram, "uViewport"), state.width, state.height);
  gl.uniform4f(
    getUniform(simProgram, "uShape"),
    state.shape.mix,
    2.8 + fx.particleSpark * 0.35,
    0.42 + fx.flare * 0.2,
    state.shape.mix > 0.001 ? 1 : 0,
  );

  runFullscreen(simProgram);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  res.readIndex = 1 - res.readIndex;
}

function trailStep() {
  const res = trailResources;
  const read = res.buffers[res.readIndex];
  const write = res.buffers[1 - res.readIndex];
  const pRes = particleResources;
  const particles = pRes.buffers[pRes.readIndex];
  const fx = getFxPreset();
  const q = getQualityPreset();
  const trailProgram = programs.trailDecay;
  const particleProgram = programs.particle;

  gl.bindFramebuffer(gl.FRAMEBUFFER, write.fbo);
  gl.viewport(0, 0, res.width, res.height);
  gl.disable(gl.BLEND);
  gl.useProgram(trailProgram);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, read.tex);
  gl.uniform1i(getUniform(trailProgram, "uTrail"), 0);
  gl.uniform2f(getUniform(trailProgram, "uInvResolution"), 1 / res.width, 1 / res.height);
  const decay = state.mood === "magnetic" ? 0.945 : 0.952;
  gl.uniform1f(getUniform(trailProgram, "uDecay"), decay);
  gl.uniform1f(getUniform(trailProgram, "uTime"), state.time);
  gl.uniform4f(
    getUniform(trailProgram, "uFx"),
    fx.trailWarp,
    fx.trailGhost,
    fx.trailSparkle,
    fx.trailDecayLift,
  );
  runFullscreen(trailProgram);

  gl.enable(gl.BLEND);
  // Alpha-weighted additive keeps local glow while avoiding runaway white overdraw.
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  gl.useProgram(particleProgram);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, particles.pos);
  gl.uniform1i(getUniform(particleProgram, "uPosTex"), 0);
  gl.activeTexture(gl.TEXTURE1);
  gl.bindTexture(gl.TEXTURE_2D, particles.vel);
  gl.uniform1i(getUniform(particleProgram, "uVelTex"), 1);
  gl.uniform2f(getUniform(particleProgram, "uResolution"), res.width, res.height);
  // Make particles bigger for better visibility
  const pointScale = Math.max(2.5, Math.min(5.5, state.dpr * 2.8 * q.pointScaleMul));
  gl.uniform1f(getUniform(particleProgram, "uPointScale"), pointScale);
  gl.uniform1f(getUniform(particleProgram, "uTime"), state.time);
  gl.uniform1i(getUniform(particleProgram, "uMood"), moodIndex());
  
  // Get current cycling colors
  const colors = state.colorCycle.colors;
  const currentIdx = state.colorCycle.currentIndex;
  const nextIdx = state.colorCycle.nextIndex;
  const currentColor = colors[currentIdx];
  const nextColor = colors[nextIdx];
  const tintMix = state.colorCycle.mix;
  
  gl.uniform3f(
    getUniform(particleProgram, "uTint"),
    currentColor[0],
    currentColor[1],
    currentColor[2],
  );
  gl.uniform3f(
    getUniform(particleProgram, "uTint2"),
    nextColor[0],
    nextColor[1],
    nextColor[2],
  );
  gl.uniform1f(getUniform(particleProgram, "uTintMix"), tintMix);
  gl.uniform4f(
    getUniform(particleProgram, "uFx"),
    fx.particleSpark,
    fx.ringGain,
    fx.flare,
    fx.alphaGain,
  );
  gl.uniform1i(getUniform(particleProgram, "uFxMode"), fx.mode);
  gl.bindVertexArray(pRes.vao);
  gl.drawArrays(gl.POINTS, 0, pRes.count);
  gl.bindVertexArray(null);
  gl.disable(gl.BLEND);

  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  res.readIndex = 1 - res.readIndex;
}

function compositeToScreen() {
  const trail = trailResources.buffers[trailResources.readIndex];
  const fx = getFxPreset();
  const compositeProgram = programs.composite;
  
  // Get current cycling colors for composite tint
  const colors = state.colorCycle.colors;
  const currentIdx = state.colorCycle.currentIndex;
  const nextIdx = state.colorCycle.nextIndex;
  const currentColor = colors[currentIdx];
  const nextColor = colors[nextIdx];
  const tintMix = state.colorCycle.mix;
  
  // Blend between current and next color
  const tintR = currentColor[0] * (1 - tintMix) + nextColor[0] * tintMix;
  const tintG = currentColor[1] * (1 - tintMix) + nextColor[1] * tintMix;
  const tintB = currentColor[2] * (1 - tintMix) + nextColor[2] * tintMix;
  
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  gl.viewport(0, 0, state.width, state.height);
  gl.disable(gl.BLEND);
  gl.useProgram(compositeProgram);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, trail.tex);
  gl.uniform1i(getUniform(compositeProgram, "uTrail"), 0);
  gl.uniform2f(getUniform(compositeProgram, "uResolution"), state.width, state.height);
  gl.uniform1f(getUniform(compositeProgram, "uTime"), state.time);
  gl.uniform3f(
    getUniform(compositeProgram, "uTint"),
    tintR,
    tintG,
    tintB,
  );
  gl.uniform4f(getUniform(compositeProgram, "uFx"), fx.bloom, fx.chroma, fx.grain, fx.bgPulse);
  gl.uniform1i(getUniform(compositeProgram, "uFxMode"), fx.mode);
  runFullscreen(compositeProgram);
}

function frame(nowMs) {
  const now = nowMs * 0.001;
  if (!state.lastTime) state.lastTime = now;
  let dt = now - state.lastTime;
  state.lastTime = now;
  state.time = now;
  dt = Math.min(dt, 0.05);
  state.stats.sampleAccum += dt;
  state.stats.frameAccum += dt;
  state.stats.samples += 1;
  if (state.stats.sampleAccum >= 0.35) {
    state.stats.fps = state.stats.samples / Math.max(state.stats.frameAccum, 1e-3);
    state.stats.sampleAccum = 0;
    state.stats.frameAccum = 0;
    state.stats.samples = 0;
    updateParticleCountUi();
  }

  // Smooth pointer velocity down when idle so brush force fades naturally.
  state.pointer.vx *= 0.9;
  state.pointer.vy *= 0.9;
  if (state.shape.releaseAt > 0 && state.time >= state.shape.releaseAt) {
    state.shape.targetMix = 0;
    state.shape.releaseAt = 0;
    updateShapeActionButtons();
  }
  const shapeRate = state.shape.targetMix > state.shape.mix ? 6.5 : 2.2;
  state.shape.mix += (state.shape.targetMix - state.shape.mix) * (1 - Math.exp(-dt * shapeRate));
  updateShapeActionButtons();

  // Update color cycling - fast rotation through color combinations (only in auto mode)
  // Full cycle takes about 3 seconds for vibrant color shifting
  if (state.colorMode === "auto") {
    const cycleSpeed = 0.35; 
    state.colorCycle.mix += dt * cycleSpeed;
    if (state.colorCycle.mix >= 1.0) {
      state.colorCycle.mix = 0;
      state.colorCycle.currentIndex = state.colorCycle.nextIndex;
      state.colorCycle.nextIndex = (state.colorCycle.currentIndex + 1) % state.colorCycle.colors.length;
    }
  } else {
    // In static color mode, smoothly interpolate to the selected color
    state.colorCycle.mix = 0;
    const staticColor = state.colorPresets[state.colorMode];
    if (staticColor) {
      // Find the index of the static color in the colors array
      let colorIdx = -1;
      for (let i = 0; i < state.colorCycle.colors.length; i++) {
        const c = state.colorCycle.colors[i];
        if (Math.abs(c[0] - staticColor[0]) < 0.01 && 
            Math.abs(c[1] - staticColor[1]) < 0.01 && 
            Math.abs(c[2] - staticColor[2]) < 0.01) {
          colorIdx = i;
          break;
        }
      }
      if (colorIdx !== -1) {
        state.colorCycle.currentIndex = colorIdx;
        state.colorCycle.nextIndex = (colorIdx + 1) % state.colorCycle.colors.length;
      }
    }
  }

  // Update CSS accent color
  updateAccentColor();

  simStep(dt);
  trailStep();
  compositeToScreen();
  requestAnimationFrame(frame);
}

// Update CSS accent color based on current cycling color
function updateAccentColor() {
  const colors = state.colorCycle.colors;
  const currentIdx = state.colorCycle.currentIndex;
  const nextIdx = state.colorCycle.nextIndex;
  const currentColor = colors[currentIdx];
  const nextColor = colors[nextIdx];
  const mix = state.colorCycle.mix;
  
  const r = Math.round((currentColor[0] * (1 - mix) + nextColor[0] * mix) * 255);
  const g = Math.round((currentColor[1] * (1 - mix) + nextColor[1] * mix) * 255);
  const b = Math.round((currentColor[2] * (1 - mix) + nextColor[2] * mix) * 255);
  
  const hex = `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
  document.documentElement.style.setProperty("--accent", hex);
}

resize();
window.addEventListener("resize", resize);
rebuildShapeTargetTexture();
updateAccentColor();
setActiveButton(fxSeg, "fx", state.fx.mode);
setActiveButton(colorSeg, "color", state.colorMode);
setRustUiVisible(false);
updateParticleCountUi({ includeFps: false });
updateShapeActionButtons();

requestAnimationFrame(frame);
