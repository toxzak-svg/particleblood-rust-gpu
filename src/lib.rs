use std::cell::RefCell;
use std::f32::consts::TAU;
use std::rc::Rc;

use js_sys::{Float32Array, Math, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    CanvasRenderingContext2d, Document, Element, Event, EventTarget,
    HtmlCanvasElement, HtmlElement, HtmlInputElement, KeyboardEvent, MouseEvent,
    PointerEvent, WebGl2RenderingContext as GL, WebGlBuffer, WebGlFramebuffer, WebGlProgram,
    WebGlShader, WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject, WheelEvent, Window,
};

thread_local! {
    static APP_HOLDER: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

/// Set before start() to run in story mode with a custom message. JS should call this after parsing URL (e.g. ?m=1&t=Hello).
#[wasm_bindgen(js_name = initStoryMode)]
pub fn init_story_mode(message: &str, story_mode: bool) {
    STORY_INIT.with(|cell| {
        *cell.borrow_mut() = Some(StoryInit {
            message: message.to_string(),
            story_mode,
        });
    });
}

struct StoryInit {
    message: String,
    story_mode: bool,
}

thread_local! {
    static STORY_INIT: RefCell<Option<StoryInit>> = const { RefCell::new(None) };
}

const TAP_TRIGGER_MAX_MS: f64 = 260.0;
const TAP_TRIGGER_MAX_MOVE_PX: f32 = 16.0;
const SHAPE_FORM_DURATION_S: f64 = 2.3;
const INTRO_TITLE: &str = "RUSTY PARTS";
const DEFAULT_SHAPE_TEXT: &str = "Touch!";
const INTRO_FADE_IN_S: f64 = 0.0;
const INTRO_HOLD_S: f64 = 1.4;
const INTRO_MELT_S: f64 = 0.7;
const INTRO_BURST_STRENGTH: f32 = 4.2;
const INTRO_BURST_COUNT: usize = 20;
const PARTICLE_RESOLUTION_SCALE: f64 = 1.30;
const COLOR_SHIFT_MIN_S: f64 = 1.4;
const COLOR_SHIFT_MAX_S: f64 = 3.0;
const COLOR_HOLD_MIN_S: f64 = 0.4;
const COLOR_HOLD_MAX_S: f64 = 1.2;
const TOUCH_ATTRACT_RAMP_S: f64 = 2.4;
const TOUCH_ATTRACT_MAX_GAIN: f32 = 5.6;
const TOUCH_ATTRACT_WAVE_A_HZ: f32 = 1.7;
const TOUCH_ATTRACT_WAVE_B_HZ: f32 = 4.2;
const TOUCH_ATTRACT_WAVE_BLEND: f32 = 0.36;
const TOUCH_BURST_MIN_HOLD_S: f64 = 0.55;
/// Gravity scale: map device g (m/s²) to NDC acceleration. ~0.06 gives subtle tilt. (Unused when devicemotion is disabled.)
#[allow(dead_code)]
const TILT_GRAVITY_SCALE: f32 = 0.06;
#[allow(dead_code)]
const TILT_SMOOTH: f32 = 0.14;
const TOUCH_BURST_FULL_HOLD_S: f64 = 3.2;
const TOUCH_BURST_DURATION_S: f64 = 1.15;
const TOUCH_BURST_MIN_STRENGTH: f32 = 1.0;
const TOUCH_BURST_MAX_STRENGTH: f32 = 4.2;
const PARTICLE_TEX_LADDER: &[i32] = &[
    128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640, 704, 768, 896, 1024, 1280, 1536, 1792,
    2048, 2304,
];

const FULLSCREEN_VS: &str = r#"#version 300 es
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
}
"#;

const BACKGROUND_FS: &str = r#"#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform vec2 uResolution;
uniform float uTime;
uniform vec3 uTint;
uniform vec4 uFx; // x bloom-ish, y chroma-ish, z grain, w pulse
uniform int uFxMode;

float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

float hash21(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Approximate complementary: darken and shift hue for a background that complements the particle tint
vec3 tintToComplement(vec3 c) {
  float mx = max(max(c.r, c.g), c.b);
  float mn = min(min(c.r, c.g), c.b);
  float luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
  vec3 comp = vec3(1.0 - c.r * 0.7, 1.0 - c.g * 0.5, 1.0 - c.b * 0.6);
  comp = mix(comp, vec3(0.08, 0.06, 0.14), 0.92);
  return comp;
}

void main() {
  vec2 uv = vUv - 0.5;
  float aspect = uResolution.x / max(uResolution.y, 1.0);
  uv.x *= aspect;
  float r = length(uv) * 1.4;

  vec3 base = tintToComplement(uTint);
  float pulse = 0.5 + 0.5 * sin(uTime * 0.4);
  base += (uTint * 0.06 + 0.02) * (0.3 + 0.7 * uFx.w * pulse);

  float vignette = 1.0 - smoothstep(0.5, 1.2, r);
  vignette = 0.4 + 0.6 * vignette;
  base *= vignette;

  float n = hash21(floor(uv * 80.0 + uTime * 2.0));
  float wave = sin(uv.x * 6.0 + uTime * 0.8) * sin(uv.y * 5.0 - uTime * 0.6) * 0.5 + 0.5;
  float soft = mix(n, wave, 0.3) * 0.04 * (1.0 + uFx.w * 0.5);
  base += soft * (uTint + 0.1);

  if (uFxMode == 1) {
    float prism = sin(uv.x * 3.0 + uTime * 0.5) * sin(uv.y * 4.0 + uTime * 0.3);
    base += vec3(0.02, 0.0, 0.03) * (0.5 + 0.5 * prism) * uFx.y;
  } else if (uFxMode == 2) {
    float plasma = sin(r * 4.0 - uTime) * sin(uv.x * 8.0 + uTime * 0.7) * 0.5 + 0.5;
    base += vec3(0.03, 0.01, 0.02) * plasma * (0.6 + 0.4 * uFx.w);
  }

  if (uFx.z > 0.001) {
    float grain = hash12(vUv * uResolution + fract(uTime * 60.0));
    base += (grain - 0.5) * (0.004 + 0.018 * uFx.z);
  }

  base = clamp(base, 0.0, 1.0);
  outColor = vec4(base, 1.0);
}
"#;

const PARTICLE_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 aUv;
uniform sampler2D uStateTex;
uniform sampler2D uMetaTex;
uniform vec2 uResolution;
uniform float uPointScale;
out vec2 vMeta;
void main() {
  vec4 state = texture(uStateTex, aUv);
  vec4 meta = texture(uMetaTex, aUv);
  vec2 aPos = state.xy;
  float speed = length(state.zw);
  float aspect = uResolution.x / max(uResolution.y, 1.0);
  gl_Position = vec4(aPos.x / aspect, aPos.y, 0.0, 1.0);
  // Fixed size - sand particles don't grow when moving fast
  gl_PointSize = uPointScale;
  vMeta = vec2(meta.x, speed);
}
"#;

const PARTICLE_FS: &str = r#"#version 300 es
precision highp float;
in vec2 vMeta;
out vec4 outColor;
uniform float uTime;
uniform vec3 uTintA;
uniform vec3 uTintB;
uniform vec4 uFx; // x spark, y ring, z flare, w alpha
uniform int uFxMode;
void main() {
  vec2 p = gl_PointCoord * 2.0 - 1.0;
  float r2 = dot(p, p);
  if (r2 > 1.0) discard;

  // Sand-like: solid matte particles, no glow rings or flares
  float core = exp(-r2 * 8.0);
  
  float seed = vMeta.x;
  float speed = vMeta.y;
  
  // Stronger two-tone split with a little movement over time.
  float wave = 0.5 + 0.5 * sin(uTime * 0.58 + seed * 15.0 + speed * 1.4);
  float toneMix = clamp(mix(seed, wave, 0.62) + (speed - 0.2) * 0.14, 0.0, 1.0);
  float softSplit = smoothstep(0.28, 0.72, toneMix);
  float hardSplit = step(0.5, toneMix);
  vec3 col = mix(uTintA, uTintB, softSplit);
  col = mix(col, mix(uTintA, uTintB, hardSplit), 0.48);
  float luma = dot(col, vec3(0.2126, 0.7152, 0.0722));
  col = clamp(mix(vec3(luma), col, 1.26), 0.0, 1.0);
  col *= (0.8 + core * 0.4);

  // Solid matte appearance - no additive glow
  float outAlpha = core * 0.85;
  outAlpha = clamp(outAlpha, 0.0, 0.85);
  
  // No glow multiplication - matte finish
  outColor = vec4(col * outAlpha, outAlpha);
}
"#;

const SIM_FS: &str = r#"#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outState;

uniform sampler2D uStateTex;
uniform sampler2D uMetaTex;
uniform sampler2D uShapeTargetTex;
uniform vec2 uStateResolution;
uniform float uTime;
uniform float uDt;
uniform int uBrushMode;
uniform vec4 uPointer;      // xy pos, z radius, w active
uniform vec4 uPointerV;     // xy velocity, z strength, w unused
uniform vec4 uAttractor;    // xy pos, z mass, w enabled
uniform float uAttractorSpin;
uniform vec2 uViewport;
uniform vec4 uShape;        // x mix, y pull, z orbit, w active
uniform vec4 uFx;           // x flow gain, y particle spark, z flare, w unused
uniform vec4 uTouchBurst;   // xy center, z strength, w active
uniform vec4 uTouchBurstFx; // x progress, y age, z ring radius, w rebound
uniform float uIntroBurstDuration;
uniform int uIntroBurstCount;
uniform vec4 uIntroBursts[20]; // xy center, z strength, w start_s
uniform vec2 uTilt;         // device tilt as gravity bias (x right, y up in NDC)

void applyMagnet(inout vec2 acc, vec2 p, vec2 center, float mass, float spin, float falloffBias) {
  vec2 d = center - p;
  float r2 = dot(d, d) + falloffBias;
  float inv = 1.0 / max(r2, 1e-4);
  vec2 t = vec2(-d.y, d.x);
  acc += d * (mass * inv);
  acc += t * (spin * mass * inv * 0.9);
}

void main() {
  vec2 id = floor(gl_FragCoord.xy - 0.5);
  vec2 uv = (id + 0.5) / uStateResolution;
  vec4 stateData = texture(uStateTex, uv);
  vec4 metaData = texture(uMetaTex, uv);
  vec2 p = stateData.xy;
  vec2 v = stateData.zw;
  float seed = metaData.x;
  float phase = metaData.y;
  float t = uTime;
  float dt = min(uDt, 0.033);
  float aspect = uViewport.x / max(uViewport.y, 1.0);
  vec2 acc = vec2(0.0);
    float burstProgress = clamp(uTouchBurstFx.x, 0.0, 1.0);
    float shapeMix = clamp(uShape.x, 0.0, 1.0);
    float melt = 1.0 - shapeMix;
    float breathe = 0.5 + 0.5 * sin(t * 0.42 + seed * 4.1 + phase * 2.9);
    vec2 liveCenter = vec2(
        sin(t * 0.18 + seed * 6.2831) * 0.23,
        cos(t * 0.16 + phase * 6.2831) * 0.19
    );
    liveCenter += vec2(sin(t * 0.33) * 0.10, cos(t * 0.29) * 0.08);
    liveCenter *= melt;

    float n1 = sin(p.y * 4.7 + t * 0.9 + seed * 6.2831);
    float n2 = cos(p.x * 5.1 - t * 0.7 + phase * 4.9);
    float swirl = 0.04 + uFx.x * 0.10;
  acc += vec2(
    (-n2 + 0.35 * sin(p.y * 3.2 + t * 0.4 + seed * 2.0)) * swirl,
    ( n1 + 0.35 * cos(p.x * 2.7 - t * 0.5 + phase * 2.0)) * swirl
  );
    acc += vec2(-v.y, v.x) * 0.005;
    acc += uTilt;

    vec2 toCenter = liveCenter - p;

    vec2 warpCenter = liveCenter + vec2(
        sin(t * 0.54 + seed * 6.2831) * 0.17,
        cos(t * 0.59 + phase * 6.2831) * 0.14
    );
    vec2 dWarp = warpCenter - p;
    float warpR2 = dot(dWarp, dWarp) + 0.055;
    vec2 warpTangential = vec2(-dWarp.y, dWarp.x) / warpR2;
    acc += warpTangential * (melt * 0.03);

    vec2 twinA = liveCenter + vec2(-0.22 * aspect, 0.06);
    vec2 twinB = liveCenter + vec2(0.22 * aspect, -0.06);
    vec2 da = twinA - p;
    vec2 db = twinB - p;
    float ia = 1.0 / (dot(da, da) + 0.09);
    float ib = 1.0 / (dot(db, db) + 0.09);
    acc += vec2(-da.y, da.x) * ia * (melt * 0.022);
    acc += vec2(db.y, -db.x) * ib * (melt * 0.022);

    float microAx = sin(t * (0.73 + seed * 0.71) + phase * 6.2831);
    float microAy = cos(t * (0.61 + phase * 0.83) + seed * 6.2831);
    vec2 microWind = vec2(microAx, microAy);
    microWind /= max(length(microWind), 1e-4);
    acc += microWind * 0.005;

    vec2 bodyAxis = vec2(sin(t * 0.31 + phase * 5.3), cos(t * 0.27 + seed * 5.9));
    bodyAxis /= max(length(bodyAxis), 1e-4);
    vec2 rel = p - liveCenter;
    float relLen = max(length(rel), 1e-4);
    vec2 relDir = rel / relLen;
    float undulate = sin(t * 1.18 + dot(relDir, bodyAxis) * 6.5 + seed * 8.0);
    acc += bodyAxis * undulate * melt * 0.010;

    // When fully melted and left alone, pull particles into warped bands with
    // pockets along each band so they settle into visible clumped patterns.
    float activeTools = max(uPointer.w, uAttractor.w);
    float moltenIdle = smoothstep(0.70, 1.0, melt) * (1.0 - activeTools);
    float speedNow = length(v);
    float stillness = 1.0 - smoothstep(0.10, 0.48, speedNow);
    float patternGain = moltenIdle * stillness;
    if (patternGain > 0.0) {
      float bandSpacing = 0.22;
      float warpAmp = 0.17;
      float warpFreq = 1.8;
      float warpPhase = p.x * warpFreq + t * 0.09 + seed * 5.3;
      float warpedY = p.y + sin(warpPhase) * warpAmp;
      float bandIndex = floor(warpedY / bandSpacing + 0.5);
      float bandOffset = warpedY - bandIndex * bandSpacing;

      vec2 gradBand = vec2(cos(warpPhase) * warpAmp * warpFreq, 1.0);
      vec2 bandNormal = normalize(gradBand);
      vec2 bandTangent = vec2(-bandNormal.y, bandNormal.x);

      float pocketSpacing = 0.24;
      float jitter = fract(sin((bandIndex + phase * 3.7) * 17.31 + seed * 41.0) * 43758.5453);
      float pocketOffset = (jitter - 0.5) * pocketSpacing * 0.9;
      float along = dot(p, bandTangent);
      float pocketCenter =
          floor((along + pocketOffset) / pocketSpacing + 0.5) * pocketSpacing - pocketOffset;
      float pocketDelta = along - pocketCenter;

      float bandStrength = 0.24 * patternGain;
      float pocketStrength = 0.12 * patternGain;
      acc += -bandNormal * (bandOffset * bandStrength);
      acc += -bandTangent * (pocketDelta * pocketStrength);
    }

  if (uAttractor.w > 0.5) {
    float attractMass = uAttractor.z * 3.0;
    float attractSpin = uAttractorSpin * 0.65;
    applyMagnet(acc, p, uAttractor.xy, attractMass, attractSpin, 0.014);
  }

  if (uPointer.w > 0.5) {
    vec2 d = p - uPointer.xy;
    float r = max(uPointer.z, 0.001);
    float dist2 = dot(d, d);
    float dist = sqrt(max(dist2, 1e-8));
    float fall = exp(-pow(dist / r, 2.0) * 2.5);
    vec2 dir = (dist > 1e-4) ? d / dist : vec2(1.0, 0.0);
    float brushStrength = (1.3 + length(uPointerV.xy) * 3.2) * uPointerV.z;
    if (uBrushMode == 0) {
      acc += dir * brushStrength * fall * 2.8;
    } else if (uBrushMode == 1) {
      acc -= dir * brushStrength * fall * 2.8;
    } else {
      acc += vec2(-dir.y, dir.x) * brushStrength * fall * 2.4;
      acc += uPointerV.xy * fall * 2.2;
    }
  }

  if (uTouchBurst.w > 0.5) {
    vec2 dBurst = p - uTouchBurst.xy;
    float burstDist2 = dot(dBurst, dBurst) + 1e-6;
    float burstDist = sqrt(burstDist2);
    vec2 burstDir = dBurst / burstDist;
    float burstStrength = max(uTouchBurst.z, 0.0);
    float strengthNorm = clamp(burstStrength / 4.2, 0.0, 1.0);
    float core = exp(-burstDist2 * mix(14.0, 6.0, strengthNorm));
    float ringRadius = uTouchBurstFx.z;
    float ringWidth = mix(0.055, 0.14, strengthNorm);
    float ringDelta = (burstDist - ringRadius) / max(ringWidth, 1e-4);
    float shock = exp(-ringDelta * ringDelta);
    float envelope = 1.0 - smoothstep(0.0, 1.0, burstProgress);
    float pulse = 1.0 + 0.28 * sin((uTouchBurstFx.y * 18.0 + seed * 6.2831));
    float outward = core * (1.3 + 0.9 * burstStrength)
        + shock * (2.5 + 1.9 * burstStrength) * pulse;
    acc += burstDir * outward * envelope;
    acc += vec2(-burstDir.y, burstDir.x) * shock * (0.16 + 0.26 * burstStrength) * envelope;
    acc -= burstDir * shock * uTouchBurstFx.w * (0.9 + 1.1 * burstStrength);
  }

  for (int i = 0; i < 20; i++) {
    if (i >= uIntroBurstCount) break;
    vec2 bxy = uIntroBursts[i].xy;
    float bstr = max(uIntroBursts[i].z, 0.0);
    float bstart = uIntroBursts[i].w;
    float age = t - bstart;
    if (age >= uIntroBurstDuration || age < 0.0) continue;
    float progress = age / uIntroBurstDuration;
    vec2 dBurst = p - bxy;
    float burstDist2 = dot(dBurst, dBurst) + 1e-6;
    float burstDist = sqrt(burstDist2);
    vec2 burstDir = dBurst / burstDist;
    float strengthNorm = clamp(bstr / 4.2, 0.0, 1.0);
    float core = exp(-burstDist2 * mix(14.0, 6.0, strengthNorm));
    float ringRadius = 0.06 + progress * (0.74 + bstr * 0.26);
    float ringWidth = mix(0.055, 0.14, strengthNorm);
    float ringDelta = (burstDist - ringRadius) / max(ringWidth, 1e-4);
    float shock = exp(-ringDelta * ringDelta);
    float envelope = 1.0 - smoothstep(0.0, 1.0, progress);
    float pulse = 1.0 + 0.28 * sin((age * 18.0 + seed * 6.2831));
    float outward = core * (1.3 + 0.9 * bstr) + shock * (2.5 + 1.9 * bstr) * pulse;
    acc += burstDir * outward * envelope;
    acc += vec2(-burstDir.y, burstDir.x) * shock * (0.16 + 0.26 * bstr) * envelope;
  }

  if (uShape.w > 0.5) {
    vec2 target = texture(uShapeTargetTex, uv).xy;
    target.x *= aspect;
    vec2 d = target - p;
    float dist2 = dot(d, d);
    float fall = exp(-dist2 * 4.0);
    float near = exp(-dist2 * 12.0);
    float settle = smoothstep(0.62, 0.98, uShape.x);
    float pullGain = 1.0 + settle * near * 1.7;
    acc += d * (uShape.x * uShape.y * pullGain);

    float orbitFade = 1.0 - clamp(settle * near * 0.9, 0.0, 0.9);
    acc += vec2(-d.y, d.x) * (uShape.x * uShape.z * (0.2 + 0.8 * fall) * orbitFade);

    // When the word is formed, add a gentle constant wobble so particles keep moving.
    float wobble = 0.022 * sin(t * 1.15 + phase * 6.2831 + seed * 2.1) * uShape.x;
    acc += vec2(-d.y, d.x) * wobble;

    float velDamp = clamp((0.020 + 0.060 * near) * uShape.x * (0.4 + 0.6 * settle), 0.0, 0.16);
    velDamp *= mix(1.0, 0.5, settle);
    v *= (1.0 - velDamp);

    vec2 dirToTarget = d / max(sqrt(dist2), 1e-4);
    float radialVel = dot(v, dirToTarget);
    float outwardVel = max(-radialVel, 0.0);
    v += dirToTarget * outwardVel * clamp(near * (0.24 + 0.52 * settle), 0.0, 0.76);
  }

  float density = 1.18;
  v += acc * (dt / density);
    float damping = pow(0.9965, dt * 60.0);
    if (patternGain > 0.0) {
      // Extra settling once patterns form so clumps read clearly.
      float settleDamping = pow(0.989, dt * 60.0 * patternGain);
      damping *= settleDamping;
    }
  v *= damping;
  float speed = length(v);
    float maxSpeed = 4.9 + uFx.y * 0.8;
    if (uAttractor.w > 0.5) {
      maxSpeed *= 1.7;
    }
    if (uTouchBurst.w > 0.5) {
      float burstSpeedBoost = (1.0 - burstProgress) * (1.4 + uTouchBurst.z * 1.8);
      maxSpeed += burstSpeedBoost;
    }
    if (uIntroBurstCount > 0) {
      maxSpeed += 6.0;
    }
    if (uShape.w > 0.5) {
      float settleCap = smoothstep(0.62, 0.98, clamp(uShape.x, 0.0, 1.0));
      maxSpeed *= mix(1.0, 0.82, settleCap);
    }
  if (speed > maxSpeed) {
    v *= maxSpeed / max(speed, 1e-6);
  }

  p += v * dt;

    float breathRadius = 0.03 * melt * (0.5 + 0.5 * sin(t * 0.52 + phase * 6.2831));
    vec2 bounds = vec2((1.12 + breathRadius) * aspect, 1.12 + breathRadius);
    float settleBounce = smoothstep(0.62, 0.98, clamp(uShape.x, 0.0, 1.0));
    float restitution = mix(0.9, 0.78, settleBounce);
    if (uAttractor.w > 0.5) {
      restitution *= 0.92;
    }
    if (uTouchBurst.w > 0.5) {
      restitution = min(1.2, restitution + uTouchBurstFx.w * (0.22 + 0.06 * uTouchBurst.z));
    }
    if (p.x > bounds.x) { p.x = bounds.x; v.x *= -restitution; }
    else if (p.x < -bounds.x) { p.x = -bounds.x; v.x *= -restitution; }
    if (p.y > bounds.y) { p.y = bounds.y; v.y *= -restitution; }
    else if (p.y < -bounds.y) { p.y = -bounds.y; v.y *= -restitution; }

  outState = vec4(p, v);
}
"#;

const SNAP_STATE_FS: &str = r#"#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outState;
uniform sampler2D uShapeTargetTex;
uniform float uAspect;
void main() {
  vec4 shape = texture(uShapeTargetTex, vUv);
  float px = shape.x * uAspect;
  float py = shape.y;
  outState = vec4(px, py, 0.0, 0.0);
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrushMode {
    Push,
    Pull,
    Vortex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QualityMode {
    Auto,
    Ultra,
    Insane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FxMode {
    Neon,
    Prism,
    Plasma,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShapeLayout {
    Single,
    Multi,
}

#[derive(Clone, Copy)]
struct FxPreset {
    mode: i32,
    bloom: f32,
    chroma: f32,
    grain: f32,
    bg_pulse: f32,
    particle_spark: f32,
    ring_gain: f32,
    flare: f32,
    alpha_gain: f32,
    flow_gain: f32,
}

#[derive(Clone, Copy)]
struct QualityPreset {
    max_desktop_tex: i32,
    max_mobile_tex: i32,
    point_scale_mul: f32,
}

#[derive(Clone, Copy, Default)]
struct PointerState {
    x: f32,
    y: f32,
    nx: f32,
    ny: f32,
    vx: f32,
    vy: f32,
    active: bool,
    down: bool,
    last_seen_ms: f64,
    radius: f32,
    strength: f32,
}

#[derive(Clone, Copy)]
struct AttractorState {
    enabled: bool,
    x: f32,
    y: f32,
    mass: f32,
    spin: f32,
    touch_hold_active: bool,
    touch_hold_start_s: f64,
}

#[derive(Clone, Copy)]
struct StatsState {
    fps: f32,
    sample_accum: f64,
    frame_accum: f64,
    samples: u32,
}

struct ShapeState {
    text: String,
    layout: ShapeLayout,
    mix: f32,
    target_mix: f32,
    release_at: f64,
    duration: f64,
}

#[derive(Clone, Copy)]
struct MouseTapState {
    active: bool,
    x: f32,
    y: f32,
    t_ms: f64,
    moved: bool,
}

#[derive(Clone, Copy, Default)]
struct TouchBurstState {
    active: bool,
    x: f32,
    y: f32,
    start_s: f64,
    strength: f32,
}

struct AppState {
    time: f64,
    last_time: f64,
    /// Wall time when the first frame ran; intro uses (time - start_time).
    start_time: f64,
    /// 0 = hold words, 1 = hold, 2 = melt/explode, 3 = done
    intro_phase: u8,
    intro_snapped: bool,
    dpr: f64,
    width: i32,
    height: i32,
    brush: BrushMode,
    quality: QualityMode,
    particle_amount: i32,
    fx: FxMode,
    color_hex: String,
    color_rgb: [f32; 3],
    color_alt_rgb: [f32; 3],
    color_from_rgb: [f32; 3],
    color_alt_from_rgb: [f32; 3],
    color_target_rgb: [f32; 3],
    color_alt_target_rgb: [f32; 3],
    color_shift_start_s: f64,
    color_shift_end_s: f64,
    color_hold_until_s: f64,
    pointer: PointerState,
    attractor: AttractorState,
    shape: ShapeState,
    mouse_tap: MouseTapState,
    touch_burst: TouchBurstState,
    intro_burst_xy: [(f32, f32); INTRO_BURST_COUNT],
    intro_burst_start_s: f64,
    intro_burst_count: i32,
    stats: StatsState,
    /// Device tilt: gravity bias for particles (x = right, y = up in NDC). Smoothed.
    tilt_x: f32,
    tilt_y: f32,
    /// Story mode: one-shot form → hold → melt, then callback and stop.
    story_mode: bool,
    /// Set when story mode melt is done; animation loop stops.
    story_finished: bool,
}

struct UiRefs {
    brush_seg: Option<Element>,
    quality_seg: Option<Element>,
    fx_seg: Option<Element>,
    color_seg: Option<Element>,
    color_input: Option<HtmlInputElement>,
    color_hex: Option<HtmlElement>,
    particle_slider: Option<HtmlInputElement>,
    particle_count_out: Option<HtmlElement>,
    footer_particle_count: Option<HtmlElement>,
    shape_input: Option<HtmlInputElement>,
    layout_seg: Option<Element>,
    form_btn: Option<HtmlElement>,
    melt_btn: Option<HtmlElement>,
    control_panel: Option<Element>,
    panel_toggle: Option<HtmlElement>,
    text_entry_dot: Option<HtmlElement>,
    text_entry_sheet: Option<HtmlElement>,
}

struct Programs {
    background: WebGlProgram,
    sim: WebGlProgram,
    particle: WebGlProgram,
    snap_state: WebGlProgram,
}

struct ParticleStateBuffer {
    tex: WebGlTexture,
    fbo: WebGlFramebuffer,
}

struct ParticleSystem {
    size: i32,
    buffers: [ParticleStateBuffer; 2],
    meta_tex: WebGlTexture,
    shape_target_tex: WebGlTexture,
    read_index: usize,
    vao: WebGlVertexArrayObject,
    vbo: WebGlBuffer,
    count: usize,
}

struct App {
    window: Window,
    document: Document,
    canvas: HtmlCanvasElement,
    canvas_el: HtmlElement,
    gl: GL,
    quad_vao: WebGlVertexArrayObject,
    programs: Programs,
    particles: Option<ParticleSystem>,
    ui: UiRefs,
    max_texture_size: i32,
    debug_tex_override: Option<i32>,
    rng: Lcg,
    state: AppState,
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().ok_or_else(|| js_err("window unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| js_err("document unavailable"))?;

    let app = Rc::new(RefCell::new(App::new(window, document)?));
    {
        let mut a = app.borrow_mut();
        a.resize()?;
        a.sync_ui(false)?;
    }
    attach_listeners(app.clone())?;

    if app.borrow().state.story_mode && !invoke_should_run_story() {
        invoke_show_gone();
        return Ok(());
    }

    start_animation_loop(app.clone())?;

    APP_HOLDER.with(|slot| {
        *slot.borrow_mut() = Some(app);
    });

    Ok(())
}

impl App {
    fn new(window: Window, document: Document) -> Result<Self, JsValue> {
        let canvas = document
            .get_element_by_id("app")
            .ok_or_else(|| js_err("#app canvas not found"))?
            .dyn_into::<HtmlCanvasElement>()?;
        let canvas_el = canvas.clone().dyn_into::<HtmlElement>()?;

        let gl = canvas
            .get_context("webgl2")?
            .ok_or_else(|| {
                if let Some(body) = document.body() {
                    body.set_inner_html(
                        "<p style='padding:16px;color:#fff'>WebGL2 is required for this demo.</p>",
                    );
                }
                js_err("WebGL2 unavailable")
            })?
            .dyn_into::<GL>()?;

        if gl.get_extension("EXT_color_buffer_float")?.is_none() {
            if let Some(body) = document.body() {
                body.set_inner_html(
                    "<p style='padding:16px;color:#fff'>EXT_color_buffer_float is required for GPU particles.</p>",
                );
            }
            return Err(js_err("EXT_color_buffer_float unavailable"));
        }

        let max_texture_size = gl
            .get_parameter(GL::MAX_TEXTURE_SIZE)?
            .as_f64()
            .map(|v| v as i32)
            .unwrap_or(2048)
            .max(128);
        let debug_tex_override = parse_debug_tex_override(&document);

        gl.disable(GL::DEPTH_TEST);
        gl.disable(GL::CULL_FACE);
        gl.disable(GL::STENCIL_TEST);
        gl.disable(GL::SCISSOR_TEST);

        let quad_vao = gl
            .create_vertex_array()
            .ok_or_else(|| js_err("failed to create fullscreen VAO"))?;
        gl.bind_vertex_array(Some(&quad_vao));
        gl.bind_vertex_array(None);

        let programs = Programs::new(&gl)?;
        let ui = UiRefs::from_document(&document)?;
        let particle_amount = ui
            .particle_slider
            .as_ref()
            .and_then(|input| input.value().parse::<i32>().ok())
            .unwrap_or(50)
            .clamp(1, 100);

        let mut app = Self {
            window,
            document,
            canvas,
            canvas_el,
            gl,
            quad_vao,
            programs,
            particles: None,
            ui,
            max_texture_size,
            debug_tex_override,
            rng: Lcg::seeded(),
            state: AppState {
                time: 0.0,
                last_time: 0.0,
                start_time: 0.0,
                intro_phase: 0,
                intro_snapped: false,
                dpr: 1.0,
                width: 1,
                height: 1,
                brush: BrushMode::Push,
                quality: QualityMode::Insane,
                particle_amount,
                fx: FxMode::Neon,
                color_hex: "#9bffb3".to_string(),
                color_rgb: hex_to_rgb01("#9bffb3"),
                color_alt_rgb: hex_to_rgb01("#ff8ca4"),
                color_from_rgb: hex_to_rgb01("#9bffb3"),
                color_alt_from_rgb: hex_to_rgb01("#ff8ca4"),
                color_target_rgb: hex_to_rgb01("#9bffb3"),
                color_alt_target_rgb: hex_to_rgb01("#ff8ca4"),
                color_shift_start_s: 0.0,
                color_shift_end_s: 0.0,
                color_hold_until_s: 0.0,
                pointer: PointerState {
                    radius: 0.18,
                    strength: 1.0,
                    ..PointerState::default()
                },
                attractor: AttractorState {
                    enabled: false,
                    x: 0.0,
                    y: 0.0,
                    mass: 3.2,
                    spin: 0.45,
                    touch_hold_active: false,
                    touch_hold_start_s: 0.0,
                },
                shape: ShapeState {
                    text: INTRO_TITLE.to_string(),
                    layout: ShapeLayout::Single,
                    mix: 1.0,
                    target_mix: 1.0,
                    release_at: 0.0,
                    duration: SHAPE_FORM_DURATION_S,
                },
                mouse_tap: MouseTapState {
                    active: false,
                    x: 0.0,
                    y: 0.0,
                    t_ms: 0.0,
                    moved: false,
                },
                touch_burst: TouchBurstState::default(),
                intro_burst_xy: [(0.0, 0.0); INTRO_BURST_COUNT],
                intro_burst_start_s: 0.0,
                intro_burst_count: 0,
                stats: StatsState {
                    fps: 60.0,
                    sample_accum: 0.0,
                    frame_accum: 0.0,
                    samples: 0,
                },
                story_mode: false,
                story_finished: false,
                tilt_x: 0.0,
                tilt_y: 0.0,
            },
        };

        parse_story_mode_from_url(&mut app);
        STORY_INIT.with(|cell| {
            if let Some(init) = cell.borrow_mut().take() {
                if init.story_mode {
                    app.state.story_mode = true;
                    let msg = normalize_shape_text(&init.message, false);
                    app.state.shape.text = if msg.is_empty() {
                        INTRO_TITLE.to_string()
                    } else {
                        msg
                    };
                }
            }
        });

        app.apply_particle_color("#9bffb3")?;
        Ok(app)
    }

    fn resize(&mut self) -> Result<(), JsValue> {
        let dpr = self.window.device_pixel_ratio().min(2.0);
        let inner_w = self
            .window
            .inner_width()?
            .as_f64()
            .ok_or_else(|| js_err("innerWidth unavailable"))?;
        let inner_h = self
            .window
            .inner_height()?
            .as_f64()
            .ok_or_else(|| js_err("innerHeight unavailable"))?;

        let width = (inner_w * dpr).floor().max(1.0) as i32;
        let height = (inner_h * dpr).floor().max(1.0) as i32;

        self.state.dpr = dpr;
        self.state.width = width;
        self.state.height = height;
        self.canvas.set_width(width as u32);
        self.canvas.set_height(height as u32);

        self.canvas_el
            .style()
            .set_property("width", &format!("{}px", inner_w.floor() as i32))?;
        self.canvas_el
            .style()
            .set_property("height", &format!("{}px", inner_h.floor() as i32))?;

        self.gl.viewport(0, 0, width, height);
        self.ensure_particle_system()?;
        self.update_particle_count_ui(false)?;
        Ok(())
    }

    fn ensure_particle_system(&mut self) -> Result<(), JsValue> {
        let desired_size = self.choose_particle_tex_size();
        let count = (desired_size as usize) * (desired_size as usize);
        let needs_rebuild = self
            .particles
            .as_ref()
            .map(|ps| ps.size != desired_size)
            .unwrap_or(true);
        if !needs_rebuild {
            return Ok(());
        }

        if let Some(old) = self.particles.take() {
            old.destroy(&self.gl);
        }

        let ps = ParticleSystem::new(&self.gl, count)?;
        self.particles = Some(ps);
        self.rebuild_shape_targets()?;
        Ok(())
    }

    fn choose_particle_tex_size(&self) -> i32 {
        let q = quality_preset(self.state.quality);
        let mobile = self.is_likely_mobile();
        let quality_cap = if mobile {
            q.max_mobile_tex
        } else {
            q.max_desktop_tex
        };
        let global_cap = self.max_texture_size.max(128);
        let scaled_quality_cap = ((quality_cap as f64) * PARTICLE_RESOLUTION_SCALE).floor() as i32;
        let final_cap = global_cap.min(scaled_quality_cap.max(128));

        if let Some(forced) = self.debug_tex_override {
            return choose_particle_tex_from_ladder(forced, global_cap.min(2048));
        }

        // Match the old JS selector more closely: CSS area * dpr (Rust canvas uses CSS area * dpr^2).
        let area = (self.state.width.max(1) as f64 * self.state.height.max(1) as f64)
            / self.state.dpr.max(1.0);
        let slider_fraction = (self.state.particle_amount as f64 / 100.0).clamp(0.0, 1.0);
        let slider_target =
            (128.0 + (960.0 - 128.0) * slider_fraction * slider_fraction).round() as i32;
        let area_target = if area < 320_000.0 {
            160
        } else if area < 650_000.0 {
            224
        } else if area < 1_150_000.0 {
            320
        } else if area < 1_900_000.0 {
            384
        } else if area < 2_800_000.0 {
            448
        } else if area < 3_800_000.0 {
            512
        } else if area < 5_200_000.0 {
            576
        } else if area < 7_000_000.0 {
            704
        } else if area < 9_400_000.0 {
            768
        } else {
            896
        };
        let mut target = slider_target.max(area_target);

        target = ((target as f64) * PARTICLE_RESOLUTION_SCALE).round() as i32;

        if self.state.quality == QualityMode::Auto {
            target = target.min(((512.0_f64) * PARTICLE_RESOLUTION_SCALE).round() as i32);
        }
        if self.state.quality == QualityMode::Insane && area > 10_500_000.0 {
            target = ((2304.0_f64) * PARTICLE_RESOLUTION_SCALE).round() as i32;
        }

        choose_particle_tex_from_ladder(target, final_cap)
    }

    fn is_likely_mobile(&self) -> bool {
        self.window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .map(|w| w <= 820.0)
            .unwrap_or(false)
    }

    fn step_intro(&mut self) -> Result<(), JsValue> {
        if self.state.intro_phase >= 3 {
            return Ok(());
        }
        let elapsed = self.state.time - self.state.start_time;
        match self.state.intro_phase {
            0 => {
                if elapsed >= INTRO_FADE_IN_S {
                    self.state.intro_phase = 1;
                    // #region agent log
                    debug_log(
                        "lib.rs:step_intro",
                        "phase 0->1",
                        "A",
                        &[
                            ("elapsed", elapsed),
                            ("start_time", self.state.start_time),
                        ],
                    );
                    // #endregion
                }
            }
            1 => {
                if elapsed >= INTRO_FADE_IN_S + INTRO_HOLD_S {
                    self.state.shape.target_mix = 0.0;
                    self.state.intro_phase = 2;
                    // #region agent log
                    debug_log(
                        "lib.rs:step_intro",
                        "phase 1->2 melt+bursts",
                        "A",
                        &[
                            ("elapsed", elapsed),
                            ("start_time", self.state.start_time),
                        ],
                    );
                    // #endregion
                    self.trigger_intro_bursts_multi()?;
                }
            }
            2 => {
                let melt_done = elapsed >= INTRO_FADE_IN_S + INTRO_HOLD_S + INTRO_MELT_S
                    || self.state.shape.mix < 0.08;
                if melt_done {
                    if self.state.story_mode {
                        self.state.intro_phase = 3;
                        self.state.story_finished = true;
                        invoke_story_complete_callback();
                    } else {
                        self.state.shape.text = DEFAULT_SHAPE_TEXT.to_string();
                        self.state.shape.target_mix = 1.0;
                        self.state.intro_phase = 3;
                        self.rebuild_shape_targets()?;
                        self.update_shape_action_buttons()?;
                    }
                    // #region agent log
                    debug_log(
                        "lib.rs:step_intro",
                        "phase 2->3 done",
                        "A",
                        &[("elapsed", elapsed), ("shape_mix", self.state.shape.mix as f64)],
                    );
                    // #endregion
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn trigger_intro_bursts_multi(&mut self) -> Result<(), JsValue> {
        let aspect =
            (self.state.width as f32) / (self.state.height as f32).max(1.0);
        let now = self.state.time;
        self.state.intro_burst_start_s = now;
        self.state.intro_burst_count = INTRO_BURST_COUNT as i32;
        // Spread 20 burst points over the letter area (NDC-like: x ~[-0.6,0.6], y ~[-0.35,0.35])
        let mut idx = 0usize;
        for row in 0..5 {
            for col in 0..4 {
                if idx >= INTRO_BURST_COUNT {
                    break;
                }
                let jitter_x = ((idx as u32).wrapping_mul(0x9e3779b9) % 1000) as f32 / 1000.0 * 0.12 - 0.06;
                let jitter_y = ((idx as u32).wrapping_mul(0x85ebca6b) % 1000) as f32 / 1000.0 * 0.08 - 0.04;
                let x_ndc = -0.55 + (col as f32 + 0.5) * (1.1 / 4.0) + jitter_x;
                let y_ndc = -0.28 + (row as f32 + 0.5) * (0.56 / 5.0) + jitter_y;
                self.state.intro_burst_xy[idx] = (x_ndc * aspect, y_ndc);
                idx += 1;
            }
        }
        // #region agent log
        let (bx, by) = self.state.intro_burst_xy[0];
        debug_log(
            "lib.rs:trigger_intro_bursts_multi",
            "intro bursts triggered",
            "C",
            &[
                ("intro_burst_start_s", self.state.intro_burst_start_s),
                ("intro_burst_count", self.state.intro_burst_count as f64),
                ("aspect", aspect as f64),
                ("burst0_x", bx as f64),
                ("burst0_y", by as f64),
            ],
        );
        // #endregion
        Ok(())
    }

    fn snap_particles_to_shape(&mut self) -> Result<(), JsValue> {
        let Some(ps) = self.particles.as_ref() else {
            return Ok(());
        };
        let read_index = ps.read_index;
        let write_fbo = ps.buffers[read_index].fbo.clone();
        let shape_target_tex = ps.shape_target_tex.clone();
        let state_size = ps.size;
        let aspect =
            (self.state.width as f32) / (self.state.height as f32).max(1.0);

        self.gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&write_fbo));
        self.gl.viewport(0, 0, state_size, state_size);
        self.gl.disable(GL::BLEND);
        self.gl.use_program(Some(&self.programs.snap_state));
        self.gl.active_texture(GL::TEXTURE0);
        self.gl
            .bind_texture(GL::TEXTURE_2D, Some(&shape_target_tex));
        self.gl.uniform1i(
            self.uniform(&self.programs.snap_state, "uShapeTargetTex").as_ref(),
            0,
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.snap_state, "uAspect").as_ref(),
            aspect,
        );
        self.run_fullscreen();
        self.gl.bind_framebuffer(GL::FRAMEBUFFER, None);
        Ok(())
    }

    fn frame(&mut self, now_ms: f64) -> Result<(), JsValue> {
        let now = now_ms * 0.001;
        // #region agent log
        if self.state.last_time == 0.0 {
            self.state.last_time = now;
            self.state.start_time = now;
            debug_log(
                "lib.rs:frame",
                "first frame",
                "A",
                &[
                    ("start_time", self.state.start_time),
                    ("intro_phase", self.state.intro_phase as f64),
                    ("intro_snapped", if self.state.intro_snapped { 1.0 } else { 0.0 }),
                ],
            );
        }
        // #endregion
        let mut dt = (now - self.state.last_time).clamp(0.0, 0.05);
        self.state.last_time = now;
        self.state.time = now;

        if !dt.is_finite() {
            dt = 1.0 / 60.0;
        }

        self.state.stats.sample_accum += dt;
        self.state.stats.frame_accum += dt;
        self.state.stats.samples += 1;
        if self.state.stats.sample_accum >= 0.35 {
            let frame_accum = self.state.stats.frame_accum.max(1e-3);
            self.state.stats.fps = (self.state.stats.samples as f64 / frame_accum) as f32;
            self.state.stats.sample_accum = 0.0;
            self.state.stats.frame_accum = 0.0;
            self.state.stats.samples = 0;
            self.update_particle_count_ui(true)?;
        }

        self.state.pointer.vx *= 0.9;
        self.state.pointer.vy *= 0.9;
        self.step_random_color_shift();

        self.step_intro()?;

        // #region agent log
        if self.state.intro_burst_count > 0
            && self.state.time >= self.state.intro_burst_start_s + TOUCH_BURST_DURATION_S
        {
            debug_log(
                "lib.rs:frame",
                "intro_burst_count zeroed",
                "D",
                &[
                    ("time", self.state.time),
                    ("intro_burst_start_s", self.state.intro_burst_start_s),
                    ("duration", TOUCH_BURST_DURATION_S),
                    ("threshold", self.state.intro_burst_start_s + TOUCH_BURST_DURATION_S),
                ],
            );
            self.state.intro_burst_count = 0;
        }
        // #endregion

        if self.state.intro_phase < 3 && !self.state.intro_snapped {
            // #region agent log
            debug_log(
                "lib.rs:frame",
                "snap_particles_to_shape (first time)",
                "B",
                &[
                    ("time", self.state.time),
                    ("intro_phase", self.state.intro_phase as f64),
                    ("has_particles", if self.particles.is_some() { 1.0 } else { 0.0 }),
                ],
            );
            // #endregion
            self.snap_particles_to_shape()?;
            self.state.intro_snapped = true;
        }

        if self.state.shape.release_at > 0.0 && self.state.time >= self.state.shape.release_at {
            self.state.shape.target_mix = 0.0;
            self.state.shape.release_at = 0.0;
            self.update_shape_action_buttons()?;
        }
        let shape_rate = if self.state.shape.target_mix > self.state.shape.mix {
            6.5
        } else {
            2.2
        };
        self.state.shape.mix += (self.state.shape.target_mix - self.state.shape.mix)
            * (1.0 - (-dt * shape_rate).exp()) as f32;

        self.step_particles(dt as f32);
        self.render()?;
        Ok(())
    }

    fn touch_hold_attractor_gain(&self) -> f32 {
        if !(self.state.attractor.enabled && self.state.attractor.touch_hold_active) {
            return 1.0;
        }

        let hold_s = (self.state.time - self.state.attractor.touch_hold_start_s).max(0.0) as f32;
        let ramp = (hold_s / TOUCH_ATTRACT_RAMP_S as f32).clamp(0.0, 1.0);
        let ramp_eased = ramp * ramp * (3.0 - 2.0 * ramp);
        let ramp_gain = 1.0 + (TOUCH_ATTRACT_MAX_GAIN - 1.0) * ramp_eased;
        if ramp < 1.0 {
            return ramp_gain;
        }

        let wave_t = hold_s - TOUCH_ATTRACT_RAMP_S as f32;
        let wave = (wave_t * TAU * TOUCH_ATTRACT_WAVE_A_HZ).sin() * 0.19
            + (wave_t * TAU * TOUCH_ATTRACT_WAVE_B_HZ).sin() * 0.11;
        let gain = TOUCH_ATTRACT_MAX_GAIN * (1.0 + wave * TOUCH_ATTRACT_WAVE_BLEND);
        gain.clamp(TOUCH_ATTRACT_MAX_GAIN * 0.78, TOUCH_ATTRACT_MAX_GAIN * 1.28)
    }

    fn touch_hold_charge_from_duration(hold_s: f64) -> f32 {
        let span = (TOUCH_BURST_FULL_HOLD_S - TOUCH_BURST_MIN_HOLD_S).max(1e-6);
        let t = ((hold_s - TOUCH_BURST_MIN_HOLD_S) / span).clamp(0.0, 1.0) as f32;
        t * t * (3.0 - 2.0 * t)
    }

    fn trigger_touch_release_burst(&mut self, hold_s: f64) {
        if hold_s < TOUCH_BURST_MIN_HOLD_S {
            return;
        }
        let charge = Self::touch_hold_charge_from_duration(hold_s);
        let strength = TOUCH_BURST_MIN_STRENGTH
            + (TOUCH_BURST_MAX_STRENGTH - TOUCH_BURST_MIN_STRENGTH) * charge;
        self.state.touch_burst = TouchBurstState {
            active: true,
            x: self.state.pointer.nx,
            y: self.state.pointer.ny,
            start_s: self.state.time,
            strength,
        };
    }

    fn touch_burst_uniforms(&mut self) -> ([f32; 4], [f32; 4]) {
        if !self.state.touch_burst.active {
            return ([0.0; 4], [0.0; 4]);
        }

        let age = (self.state.time - self.state.touch_burst.start_s).max(0.0);
        if age >= TOUCH_BURST_DURATION_S {
            self.state.touch_burst.active = false;
            return ([0.0; 4], [0.0; 4]);
        }

        let progress = (age / TOUCH_BURST_DURATION_S) as f32;
        let strength = self.state.touch_burst.strength.max(0.0);
        let ring_radius = 0.06 + progress * (0.74 + strength * 0.26);
        let rebound_up = smoothstep_f32(0.20, 0.52, progress);
        let rebound_down = 1.0 - smoothstep_f32(0.62, 0.98, progress);
        let rebound = (rebound_up * rebound_down * (0.35 + 0.24 * strength)).clamp(0.0, 1.3);

        (
            [
                self.state.touch_burst.x,
                self.state.touch_burst.y,
                strength,
                1.0,
            ],
            [progress, age as f32, ring_radius, rebound],
        )
    }

    fn step_particles(&mut self, dt: f32) {
        let (touch_burst, touch_burst_fx) = self.touch_burst_uniforms();
        let Some(ps) = self.particles.as_ref() else {
            return;
        };

        let fx = fx_preset(self.state.fx);
        let pointer_recent = self.state.pointer.active
            && (self.state.time - self.state.pointer.last_seen_ms * 0.001) < 0.15;
        let shape_mix = self.state.shape.mix.clamp(0.0, 1.0);
        let attract_gain = self.touch_hold_attractor_gain();
        let attract_mass = (self.state.attractor.mass * attract_gain).clamp(0.08, 20.0);
        let attract_spin = self.state.attractor.spin * (1.0 + (attract_gain - 1.0) * 0.12);
        let read_index = ps.read_index;
        let write_index = 1 - read_index;
        let read_tex = ps.buffers[read_index].tex.clone();
        let write_fbo = ps.buffers[write_index].fbo.clone();
        let meta_tex = ps.meta_tex.clone();
        let shape_target_tex = ps.shape_target_tex.clone();
        let state_size = ps.size;

        self.gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&write_fbo));
        self.gl.viewport(0, 0, state_size, state_size);
        self.gl.disable(GL::BLEND);
        self.gl.use_program(Some(&self.programs.sim));

        self.gl.active_texture(GL::TEXTURE0);
        self.gl.bind_texture(GL::TEXTURE_2D, Some(&read_tex));
        self.gl
            .uniform1i(self.uniform(&self.programs.sim, "uStateTex").as_ref(), 0);

        self.gl.active_texture(GL::TEXTURE1);
        self.gl.bind_texture(GL::TEXTURE_2D, Some(&meta_tex));
        self.gl
            .uniform1i(self.uniform(&self.programs.sim, "uMetaTex").as_ref(), 1);

        self.gl.active_texture(GL::TEXTURE2);
        self.gl
            .bind_texture(GL::TEXTURE_2D, Some(&shape_target_tex));
        self.gl.uniform1i(
            self.uniform(&self.programs.sim, "uShapeTargetTex").as_ref(),
            2,
        );

        self.gl.uniform2f(
            self.uniform(&self.programs.sim, "uStateResolution")
                .as_ref(),
            state_size as f32,
            state_size as f32,
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.sim, "uTime").as_ref(),
            self.state.time as f32,
        );
        self.gl
            .uniform1f(self.uniform(&self.programs.sim, "uDt").as_ref(), dt);
        self.gl.uniform1i(
            self.uniform(&self.programs.sim, "uBrushMode").as_ref(),
            self.state.brush.as_i32(),
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uPointer").as_ref(),
            self.state.pointer.nx,
            self.state.pointer.ny,
            self.state.pointer.radius.max(0.01),
            if pointer_recent { 1.0 } else { 0.0 },
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uPointerV").as_ref(),
            self.state.pointer.vx,
            self.state.pointer.vy,
            self.state.pointer.strength,
            0.0,
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uAttractor").as_ref(),
            self.state.attractor.x,
            self.state.attractor.y,
            attract_mass,
            if self.state.attractor.enabled {
                1.0
            } else {
                0.0
            },
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.sim, "uAttractorSpin").as_ref(),
            attract_spin,
        );
        self.gl.uniform2f(
            self.uniform(&self.programs.sim, "uViewport").as_ref(),
            self.state.width as f32,
            self.state.height as f32,
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uShape").as_ref(),
            shape_mix,
            3.6 + fx.particle_spark * 0.45,
            0.42 + fx.flare * 0.2,
            if shape_mix > 0.001 { 1.0 } else { 0.0 },
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uFx").as_ref(),
            fx.flow_gain,
            fx.particle_spark,
            fx.flare,
            0.0,
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uTouchBurst").as_ref(),
            touch_burst[0],
            touch_burst[1],
            touch_burst[2],
            touch_burst[3],
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.sim, "uTouchBurstFx").as_ref(),
            touch_burst_fx[0],
            touch_burst_fx[1],
            touch_burst_fx[2],
            touch_burst_fx[3],
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.sim, "uIntroBurstDuration").as_ref(),
            TOUCH_BURST_DURATION_S as f32,
        );
        self.gl.uniform1i(
            self.uniform(&self.programs.sim, "uIntroBurstCount").as_ref(),
            self.state.intro_burst_count,
        );
        let mut intro_burst_data = [0.0f32; INTRO_BURST_COUNT * 4];
        let start_s = self.state.intro_burst_start_s as f32;
        for i in 0..INTRO_BURST_COUNT {
            let base = i * 4;
            intro_burst_data[base] = self.state.intro_burst_xy[i].0;
            intro_burst_data[base + 1] = self.state.intro_burst_xy[i].1;
            intro_burst_data[base + 2] = INTRO_BURST_STRENGTH;
            intro_burst_data[base + 3] = start_s;
        }
        if let Some(loc) = self.uniform(&self.programs.sim, "uIntroBursts").as_ref() {
            self.gl
                .uniform4fv_with_f32_array(Some(loc), &intro_burst_data);
        }
        self.gl.uniform2f(
            self.uniform(&self.programs.sim, "uTilt").as_ref(),
            self.state.tilt_x,
            self.state.tilt_y,
        );

        self.run_fullscreen();
        self.gl.bind_framebuffer(GL::FRAMEBUFFER, None);

        if let Some(ps_mut) = self.particles.as_mut() {
            ps_mut.read_index = write_index;
        }
    }

    fn render(&mut self) -> Result<(), JsValue> {
        let fx = fx_preset(self.state.fx);

        self.gl.viewport(0, 0, self.state.width, self.state.height);
        self.gl.disable(GL::BLEND);

        self.gl.use_program(Some(&self.programs.background));
        self.gl.uniform2f(
            self.uniform(&self.programs.background, "uResolution")
                .as_ref(),
            self.state.width as f32,
            self.state.height as f32,
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.background, "uTime").as_ref(),
            self.state.time as f32,
        );
        self.gl.uniform3f(
            self.uniform(&self.programs.background, "uTint").as_ref(),
            self.state.color_rgb[0],
            self.state.color_rgb[1],
            self.state.color_rgb[2],
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.background, "uFx").as_ref(),
            fx.bloom,
            fx.chroma,
            fx.grain,
            fx.bg_pulse,
        );
        self.gl.uniform1i(
            self.uniform(&self.programs.background, "uFxMode").as_ref(),
            fx.mode,
        );
        self.run_fullscreen();

        let Some(ps) = self.particles.as_ref() else {
            return Ok(());
        };

        self.gl.enable(GL::BLEND);
        self.gl.blend_func(GL::ONE, GL::ONE_MINUS_SRC_ALPHA);
        self.gl.use_program(Some(&self.programs.particle));
        self.gl.active_texture(GL::TEXTURE0);
        self.gl
            .bind_texture(GL::TEXTURE_2D, Some(&ps.buffers[ps.read_index].tex));
        self.gl.uniform1i(
            self.uniform(&self.programs.particle, "uStateTex").as_ref(),
            0,
        );
        self.gl.active_texture(GL::TEXTURE1);
        self.gl.bind_texture(GL::TEXTURE_2D, Some(&ps.meta_tex));
        self.gl.uniform1i(
            self.uniform(&self.programs.particle, "uMetaTex").as_ref(),
            1,
        );
        self.gl.uniform2f(
            self.uniform(&self.programs.particle, "uResolution")
                .as_ref(),
            self.state.width as f32,
            self.state.height as f32,
        );
        let pointer_speed = (self.state.pointer.vx * self.state.pointer.vx
            + self.state.pointer.vy * self.state.pointer.vy)
            .sqrt()
            .min(2.0);
        let breathe = 1.0 + 0.08 * (self.state.time as f32 * 0.9).sin();
        let motion_boost = 1.0 + pointer_speed * 0.035;
        let point_scale = (self.state.dpr as f32
            * 3.4
            * quality_preset(self.state.quality).point_scale_mul
            * breathe
            * motion_boost)
            .clamp(1.6, 7.0);
        let alpha_gain = (fx.alpha_gain * (0.94 + 0.08 * breathe)).clamp(0.68, 0.98);
        self.gl.uniform1f(
            self.uniform(&self.programs.particle, "uPointScale")
                .as_ref(),
            point_scale,
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.particle, "uTime").as_ref(),
            self.state.time as f32,
        );
        self.gl.uniform3f(
            self.uniform(&self.programs.particle, "uTintA").as_ref(),
            self.state.color_rgb[0],
            self.state.color_rgb[1],
            self.state.color_rgb[2],
        );
        self.gl.uniform3f(
            self.uniform(&self.programs.particle, "uTintB").as_ref(),
            self.state.color_alt_rgb[0],
            self.state.color_alt_rgb[1],
            self.state.color_alt_rgb[2],
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.particle, "uFx").as_ref(),
            fx.particle_spark,
            fx.ring_gain,
            fx.flare,
            alpha_gain,
        );
        self.gl.uniform1i(
            self.uniform(&self.programs.particle, "uFxMode").as_ref(),
            fx.mode,
        );

        self.gl.bind_vertex_array(Some(&ps.vao));
        self.gl.draw_arrays(GL::POINTS, 0, ps.count as i32);
        self.gl.bind_vertex_array(None);
        self.gl.disable(GL::BLEND);

        Ok(())
    }

    fn run_fullscreen(&self) {
        self.gl.bind_vertex_array(Some(&self.quad_vao));
        self.gl.draw_arrays(GL::TRIANGLES, 0, 3);
        self.gl.bind_vertex_array(None);
    }

    fn uniform(&self, program: &WebGlProgram, name: &str) -> Option<WebGlUniformLocation> {
        self.gl.get_uniform_location(program, name)
    }

    fn on_pointer_move(&mut self, client_x: f64, client_y: f64, now_ms: f64) {
        let prev_nx = self.state.pointer.nx;
        let prev_ny = self.state.pointer.ny;
        let (x, y, nx, ny) = self.norm_from_client(client_x, client_y);
        if self.state.mouse_tap.active {
            let dx = client_x as f32 - self.state.mouse_tap.x;
            let dy = client_y as f32 - self.state.mouse_tap.y;
            if dx * dx + dy * dy > TAP_TRIGGER_MAX_MOVE_PX * TAP_TRIGGER_MAX_MOVE_PX {
                self.state.mouse_tap.moved = true;
            }
        }
        self.state.pointer.x = x;
        self.state.pointer.y = y;
        self.state.pointer.nx = nx;
        self.state.pointer.ny = ny;
        if self.state.pointer.last_seen_ms > 0.0 {
            let dt = ((now_ms - self.state.pointer.last_seen_ms) / 1000.0).max(1.0 / 120.0) as f32;
            self.state.pointer.vx = (nx - prev_nx) / dt;
            self.state.pointer.vy = (ny - prev_ny) / dt;
        }
        self.state.pointer.last_seen_ms = now_ms;
        self.state.pointer.active = true;
        if self.state.pointer.down {
            self.state.attractor.x = nx;
            self.state.attractor.y = ny;
        }
    }

    fn on_pointer_leave(&mut self) {
        self.state.pointer.active = false;
        self.state.pointer.vx = 0.0;
        self.state.pointer.vy = 0.0;
        self.state.mouse_tap.active = false;
    }

    fn clear_touch_hold(&mut self) {
        self.state.attractor.touch_hold_active = false;
        self.state.attractor.touch_hold_start_s = 0.0;
    }

    fn haptic_light(&self) {
        let _ = self.window.navigator().vibrate_with_duration(8);
    }

    fn haptic_medium(&self) {
        let _ = self.window.navigator().vibrate_with_duration(20);
    }

    fn haptic_burst(&self) {
        let pattern = js_sys::Array::of3(&8.into(), &35.into(), &12.into());
        let _ = self.window.navigator().vibrate_with_pattern(pattern.as_ref());
    }

    fn on_pointer_down(&mut self, client_x: f64, client_y: f64, now_ms: f64, is_touch: bool) {
        self.on_pointer_move(client_x, client_y, now_ms);
        self.state.pointer.down = true;
        self.state.attractor.enabled = true;
        self.state.attractor.x = self.state.pointer.nx;
        self.state.attractor.y = self.state.pointer.ny;
        if is_touch {
            self.state.attractor.touch_hold_active = true;
            self.state.attractor.touch_hold_start_s = now_ms * 0.001;
            self.haptic_light();
        } else {
            self.clear_touch_hold();
        }
        self.state.mouse_tap.active = true;
        self.state.mouse_tap.x = client_x as f32;
        self.state.mouse_tap.y = client_y as f32;
        self.state.mouse_tap.t_ms = now_ms;
        self.state.mouse_tap.moved = false;
    }

    fn on_pointer_up(
        &mut self,
        client_x: f64,
        client_y: f64,
        now_ms: f64,
        is_touch: bool,
    ) -> Result<(), JsValue> {
        self.on_pointer_move(client_x, client_y, now_ms);
        let hold_s = if is_touch && self.state.attractor.touch_hold_active {
            (now_ms * 0.001 - self.state.attractor.touch_hold_start_s).max(0.0)
        } else {
            0.0
        };
        self.state.pointer.down = false;
        self.state.attractor.enabled = false;
        if is_touch {
            self.trigger_touch_release_burst(hold_s);
            if hold_s >= TOUCH_BURST_MIN_HOLD_S {
                self.haptic_burst();
            } else {
                self.haptic_light();
            }
        }
        self.clear_touch_hold();
        let dt = now_ms - self.state.mouse_tap.t_ms;
        if self.state.mouse_tap.active && !self.state.mouse_tap.moved && dt <= TAP_TRIGGER_MAX_MS {
            self.toggle_shape_word()?;
        }
        self.state.mouse_tap.active = false;
        Ok(())
    }

    fn on_pointer_cancel(&mut self) {
        self.state.pointer.down = false;
        self.state.attractor.enabled = false;
        self.state.mouse_tap.active = false;
        self.clear_touch_hold();
    }

    /// Update tilt from device acceleration (including gravity). x/y/z in m/s²; smoothed. (Unused when devicemotion is disabled.)
    #[allow(dead_code)]
    fn on_device_motion(&mut self, gx: Option<f64>, gy: Option<f64>, gz: Option<f64>) {
        let gx = gx.unwrap_or(0.0) as f32;
        let _gy = gy.unwrap_or(0.0) as f32;
        let gz = gz.unwrap_or(0.0) as f32;
        let norm = 9.8f32.max(1e-3);
        let target_x = (gx / norm) * TILT_GRAVITY_SCALE;
        let target_y = (-gz / norm) * TILT_GRAVITY_SCALE;
        self.state.tilt_x += (target_x - self.state.tilt_x) * TILT_SMOOTH;
        self.state.tilt_y += (target_y - self.state.tilt_y) * TILT_SMOOTH;
    }

    fn norm_from_client(&self, client_x: f64, client_y: f64) -> (f32, f32, f32, f32) {
        let rect = self.canvas.get_bounding_client_rect();
        let rw = rect.width().max(1.0);
        let rh = rect.height().max(1.0);
        let x = ((client_x - rect.left()) / rw).clamp(0.0, 1.0);
        let y = ((client_y - rect.top()) / rh).clamp(0.0, 1.0);
        let nx = ((x * 2.0 - 1.0) * (rw / rh)) as f32;
        let ny = (-(y * 2.0 - 1.0)) as f32;
        (x as f32, y as f32, nx, ny)
    }

    fn schedule_random_palette_shift(&mut self, now_s: f64) {
        self.state.color_from_rgb = self.state.color_rgb;
        self.state.color_alt_from_rgb = self.state.color_alt_rgb;

        let (to_a, to_b) = random_two_tone_pair(&mut self.rng);
        self.state.color_target_rgb = to_a;
        self.state.color_alt_target_rgb = to_b;

        let duration =
            COLOR_SHIFT_MIN_S + (COLOR_SHIFT_MAX_S - COLOR_SHIFT_MIN_S) * self.rng.f32() as f64;
        let hold = COLOR_HOLD_MIN_S + (COLOR_HOLD_MAX_S - COLOR_HOLD_MIN_S) * self.rng.f32() as f64;

        self.state.color_shift_start_s = now_s;
        self.state.color_shift_end_s = now_s + duration;
        self.state.color_hold_until_s = self.state.color_shift_end_s + hold;
    }

    fn step_random_color_shift(&mut self) {
        let now_s = self.state.time;
        if self.state.color_shift_end_s <= self.state.color_shift_start_s
            || now_s >= self.state.color_hold_until_s
        {
            self.schedule_random_palette_shift(now_s);
        }

        let span = (self.state.color_shift_end_s - self.state.color_shift_start_s).max(1e-6);
        let t = ((now_s - self.state.color_shift_start_s) / span).clamp(0.0, 1.0) as f32;
        let eased = t * t * (3.0 - 2.0 * t);
        self.state.color_rgb = lerp_rgb(
            self.state.color_from_rgb,
            self.state.color_target_rgb,
            eased,
        );
        self.state.color_alt_rgb = lerp_rgb(
            self.state.color_alt_from_rgb,
            self.state.color_alt_target_rgb,
            eased,
        );
        self.update_css_palette_vars();
    }

    fn update_css_palette_vars(&self) {
        let Some(root) = self.document.document_element() else {
            return;
        };
        let Ok(root_el) = root.dyn_into::<HtmlElement>() else {
            return;
        };
        let base_hex = rgb01_to_hex(self.state.color_rgb);
        let alt_hex = rgb01_to_hex(self.state.color_alt_rgb);
        let _ = root_el.style().set_property("--accent", &base_hex);
        let _ = root_el.style().set_property("--accent-alt", &alt_hex);
    }

    fn apply_particle_color(&mut self, hex: &str) -> Result<(), JsValue> {
        let normalized = normalize_hex_color(hex).unwrap_or_else(|| "#9bffb3".to_string());
        let base = hex_to_rgb01(&normalized);
        let (h, s, v) = rgb_to_hsv(base);
        let sign = if self.rng.f32() > 0.5 { 1.0 } else { -1.0 };
        let partner_h = wrap01(h + sign * (0.34 + 0.18 * self.rng.f32()));
        let partner_s = clamp_f32(0.74 + s * 0.32, 0.68, 1.0);
        let partner_v = if v > 0.62 {
            clamp_f32(v * 0.54, 0.34, 0.72)
        } else {
            clamp_f32(v * 1.26 + 0.18, 0.72, 1.0)
        };
        let partner = hsv_to_rgb(partner_h, partner_s, partner_v);

        self.state.color_rgb = base;
        self.state.color_alt_rgb = partner;
        self.state.color_from_rgb = base;
        self.state.color_alt_from_rgb = partner;
        self.state.color_target_rgb = base;
        self.state.color_alt_target_rgb = partner;
        self.state.color_shift_start_s = self.state.time;
        self.state.color_shift_end_s = self.state.time;
        self.state.color_hold_until_s = self.state.time;
        self.state.color_hex = normalized.clone();

        if let Some(input) = &self.ui.color_input {
            input.set_value(&normalized);
        }
        if let Some(out) = &self.ui.color_hex {
            out.set_text_content(Some(&normalized.to_ascii_uppercase()));
        }
        self.update_css_palette_vars();

        self.schedule_random_palette_shift(self.state.time);
        set_active_button(&self.ui.color_seg, "color", &normalized)?;
        Ok(())
    }

    fn set_text_entry_open(&mut self, open: bool, clear_input: bool) -> Result<(), JsValue> {
        if let Some(sheet) = &self.ui.text_entry_sheet {
            sheet.class_list().toggle_with_force("is-open", open)?;
            sheet.set_attribute("aria-hidden", if open { "false" } else { "true" })?;
        }
        if let Some(dot) = &self.ui.text_entry_dot {
            dot.set_attribute("aria-expanded", if open { "true" } else { "false" })?;
        }
        if open
            && clear_input
            && let Some(input) = &self.ui.shape_input
        {
            input.set_value("");
        }
        if open && let Some(input) = &self.ui.shape_input {
            let _ = input.focus();
        }
        Ok(())
    }

    fn submit_text_entry(&mut self) -> Result<(), JsValue> {
        let typed = self
            .ui
            .shape_input
            .as_ref()
            .map(|input| normalize_shape_text(&input.value(), false))
            .unwrap_or_default();
        if typed.is_empty() {
            return self.set_text_entry_open(false, false);
        }
        if let Some(input) = &self.ui.shape_input {
            input.set_value(&typed);
        }
        self.trigger_shape_form(None)?;
        self.set_text_entry_open(false, false)
    }

    fn set_layout(&mut self, layout: ShapeLayout) -> Result<(), JsValue> {
        self.state.shape.layout = layout;
        self.state.shape.text = normalize_shape_text(&self.state.shape.text, true);
        if let Some(input) = &self.ui.shape_input {
            input.set_value(&self.state.shape.text);
        }
        self.rebuild_shape_targets()?;
        set_active_button(&self.ui.layout_seg, "layout", layout.as_str())
    }

    fn is_shape_word_visible(&self) -> bool {
        self.state.shape.mix > 0.04 || self.state.shape.target_mix > 0.04
    }

    fn trigger_shape_form(&mut self, hold_duration_s: Option<f64>) -> Result<(), JsValue> {
        if let Some(input) = &self.ui.shape_input {
            let typed = normalize_shape_text(&input.value(), false);
            if !typed.is_empty() {
                self.state.shape.text = typed;
            } else {
                self.state.shape.text = normalize_shape_text(&self.state.shape.text, true);
            }
            input.set_value(&self.state.shape.text);
        } else {
            self.state.shape.text = normalize_shape_text(&self.state.shape.text, true);
        }
        self.rebuild_shape_targets()?;
        self.state.shape.target_mix = 1.0;
        let hold = hold_duration_s.unwrap_or(self.state.shape.duration);
        self.state.shape.release_at = if hold > 0.0 {
            self.state.time + hold
        } else {
            0.0
        };
        self.haptic_medium();
        self.update_shape_action_buttons()
    }

    fn trigger_shape_melt(&mut self) -> Result<(), JsValue> {
        self.state.shape.target_mix = 0.0;
        self.state.shape.release_at = 0.0;
        self.haptic_medium();
        self.update_shape_action_buttons()
    }

    fn toggle_shape_word(&mut self) -> Result<(), JsValue> {
        if self.is_shape_word_visible() {
            self.trigger_shape_melt()
        } else {
            self.trigger_shape_form(Some(0.0))
        }
    }

    fn update_shape_action_buttons(&self) -> Result<(), JsValue> {
        let shaped = self.is_shape_word_visible();
        if let Some(form_btn) = &self.ui.form_btn {
            form_btn
                .class_list()
                .toggle_with_force("is-active", shaped)?;
        }
        if let Some(melt_btn) = &self.ui.melt_btn {
            melt_btn
                .class_list()
                .toggle_with_force("is-active", !shaped)?;
        }
        Ok(())
    }

    fn rebuild_shape_targets(&mut self) -> Result<(), JsValue> {
        let text = normalize_shape_text(&self.state.shape.text, true);
        let layout = self.state.shape.layout;
        if let Some(input) = &self.ui.shape_input {
            input.set_value(&text);
        }
        self.state.shape.text = text.clone();

        let count = match self.particles.as_ref() {
            Some(ps) => ps.count,
            None => return Ok(()),
        };
        let targets = build_shape_targets(&self.document, &text, layout, count);

        let gl = self.gl.clone();
        if let Some(ps) = self.particles.as_mut() {
            ps.upload_shape_targets(&gl, &targets)?;
        }
        Ok(())
    }

    fn set_brush(&mut self, brush: BrushMode) -> Result<(), JsValue> {
        self.state.brush = brush;
        set_active_button(&self.ui.brush_seg, "brush", brush.as_str())
    }

    fn set_quality(&mut self, quality: QualityMode) -> Result<(), JsValue> {
        self.state.quality = quality;
        set_active_button(&self.ui.quality_seg, "quality", quality.as_str())?;
        self.ensure_particle_system()?;
        self.update_particle_count_ui(false)
    }

    fn set_particle_amount(&mut self, amount: i32) -> Result<(), JsValue> {
        let clamped = amount.clamp(1, 100);
        self.state.particle_amount = clamped;
        if let Some(slider) = &self.ui.particle_slider {
            slider.set_value(&clamped.to_string());
        }
        self.ensure_particle_system()?;
        self.update_particle_count_ui(false)
    }

    fn set_fx(&mut self, fx: FxMode) -> Result<(), JsValue> {
        self.state.fx = fx;
        set_active_button(&self.ui.fx_seg, "fx", fx.as_str())
    }

    fn sync_ui(&mut self, include_fps: bool) -> Result<(), JsValue> {
        set_active_button(&self.ui.brush_seg, "brush", self.state.brush.as_str())?;
        set_active_button(&self.ui.quality_seg, "quality", self.state.quality.as_str())?;
        set_active_button(&self.ui.fx_seg, "fx", self.state.fx.as_str())?;
        set_active_button(
            &self.ui.layout_seg,
            "layout",
            self.state.shape.layout.as_str(),
        )?;
        if let Some(slider) = &self.ui.particle_slider {
            slider.set_value(&self.state.particle_amount.to_string());
        }
        if let Some(input) = &self.ui.shape_input {
            input.set_value(&self.state.shape.text);
        }
        self.apply_particle_color(&self.state.color_hex.clone())?;
        self.set_text_entry_open(false, false)?;
        self.update_particle_count_ui(include_fps)?;
        self.update_shape_action_buttons()?;
        Ok(())
    }

    fn update_particle_count_ui(&self, include_fps: bool) -> Result<(), JsValue> {
        let Some(out) = &self.ui.particle_count_out else {
            return Ok(());
        };
        let (count, tex_size) = self
            .particles
            .as_ref()
            .map(|p| (p.count, p.size))
            .unwrap_or((0, 0));
        let lock = if self.debug_tex_override.is_some() {
            " lock"
        } else {
            ""
        };
        let base = if tex_size > 0 {
            format!(
                "{} pts [{}x{}]{}",
                format_particle_count(count),
                tex_size,
                tex_size,
                lock
            )
        } else {
            format!("{} pts{}", format_particle_count(count), lock)
        };
        let fps = self.state.stats.fps.round().max(0.0) as i32;
        let text = if include_fps {
            format!("{base} @ {fps}fps")
        } else {
            base
        };
        out.set_text_content(Some(&text));
        out.set_attribute("value", &text)?;
        if let Some(footer) = &self.ui.footer_particle_count {
            footer.set_text_content(Some(&format_particle_count(count)));
        }
        Ok(())
    }

    fn toggle_panel(&mut self) -> Result<(), JsValue> {
        let (Some(panel), Some(toggle)) = (&self.ui.control_panel, &self.ui.panel_toggle) else {
            return Ok(());
        };
        let collapsed = !panel.class_list().contains("is-collapsed");
        panel
            .class_list()
            .toggle_with_force("is-collapsed", collapsed)?;
        toggle.set_attribute("aria-expanded", if collapsed { "false" } else { "true" })?;
        toggle.set_text_content(Some(if collapsed { "Expand" } else { "Collapse" }));
        Ok(())
    }

    fn handle_keydown(&mut self, e: &KeyboardEvent) -> Result<(), JsValue> {
        if is_typing_target(e.target()) {
            return Ok(());
        }
        let key = e.key();
        let k = key.to_ascii_lowercase();
        match k.as_str() {
            "q" => self.set_brush(BrushMode::Push)?,
            "w" => self.set_brush(BrushMode::Pull)?,
            "e" => self.set_brush(BrushMode::Vortex)?,
            "z" => self.set_quality(QualityMode::Auto)?,
            "x" => self.set_quality(QualityMode::Ultra)?,
            "c" => self.set_quality(QualityMode::Insane)?,
            "m" => self.trigger_shape_melt()?,
            "t" => {
                let next = if self.state.shape.layout == ShapeLayout::Single {
                    ShapeLayout::Multi
                } else {
                    ShapeLayout::Single
                };
                self.set_layout(next)?;
            }
            _ => {}
        }
        match key.as_str() {
            "1" => self.set_fx(FxMode::Neon)?,
            "2" => self.set_fx(FxMode::Prism)?,
            "3" => self.set_fx(FxMode::Plasma)?,
            "Enter" => self.trigger_shape_form(None)?,
            _ => {}
        }
        Ok(())
    }
}

impl UiRefs {
    fn from_document(document: &Document) -> Result<Self, JsValue> {
        Ok(Self {
            brush_seg: document.get_element_by_id("brushSeg"),
            quality_seg: document.get_element_by_id("qualitySeg"),
            fx_seg: document.get_element_by_id("fxSeg"),
            color_seg: document.get_element_by_id("colorSeg"),
            color_input: document
                .get_element_by_id("colorInput")
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok()),
            color_hex: document
                .get_element_by_id("colorHex")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            particle_slider: document
                .get_element_by_id("particleSlider")
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok()),
            particle_count_out: document
                .get_element_by_id("particleCountOut")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            footer_particle_count: document
                .get_element_by_id("footerParticleCount")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            shape_input: document
                .get_element_by_id("shapeInput")
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok()),
            layout_seg: document.get_element_by_id("layoutSeg"),
            form_btn: document
                .get_element_by_id("formBtn")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            melt_btn: document
                .get_element_by_id("meltBtn")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            control_panel: document.get_element_by_id("controlPanel"),
            panel_toggle: document
                .get_element_by_id("panelToggle")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            text_entry_dot: document
                .get_element_by_id("textEntryDot")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
            text_entry_sheet: document
                .get_element_by_id("textEntrySheet")
                .and_then(|e| e.dyn_into::<HtmlElement>().ok()),
        })
    }
}

impl Programs {
    fn new(gl: &GL) -> Result<Self, JsValue> {
        Ok(Self {
            background: create_program(gl, FULLSCREEN_VS, BACKGROUND_FS)?,
            sim: create_program(gl, FULLSCREEN_VS, SIM_FS)?,
            particle: create_program(gl, PARTICLE_VS, PARTICLE_FS)?,
            snap_state: create_program(gl, FULLSCREEN_VS, SNAP_STATE_FS)?,
        })
    }
}

impl ParticleSystem {
    fn new(gl: &GL, count: usize) -> Result<Self, JsValue> {
        let size = ((count.max(1) as f64).sqrt().ceil() as i32).max(1);
        let count = (size as usize) * (size as usize);

        let vao = gl
            .create_vertex_array()
            .ok_or_else(|| js_err("failed to create particle VAO"))?;
        let vbo = gl
            .create_buffer()
            .ok_or_else(|| js_err("failed to create particle VBO"))?;

        let mut rng = Lcg::seeded();
        let mut state_data = vec![0.0f32; count * 4];
        let mut meta_data = vec![0.0f32; count * 4];

        for i in 0..count {
            let a = rng.f32() * TAU;
            let r = rng.f32().powf(0.75) * 0.82;
            let x = a.cos() * r;
            let y = a.sin() * r;
            let vx = 0.02 * (a + 1.2).cos();
            let vy = 0.02 * (a - 0.7).sin();
            let seed = rng.f32();
            let phase = rng.f32();

            let base = i * 4;
            state_data[base] = x;
            state_data[base + 1] = y;
            state_data[base + 2] = vx;
            state_data[base + 3] = vy;
            meta_data[base] = seed;
            meta_data[base + 1] = phase;
        }

        let state_a = create_rgba32f_texture(gl, size, size, Some(&state_data))?;
        let state_b = create_rgba32f_texture(gl, size, size, None)?;
        let meta_tex = create_rgba32f_texture(gl, size, size, Some(&meta_data))?;
        let shape_target_tex = create_rgba32f_texture(gl, size, size, None)?;
        let fbo_a = create_color_framebuffer(gl, &state_a)?;
        let fbo_b = create_color_framebuffer(gl, &state_b)?;

        let uv_data = build_particle_uv_data(size);
        gl.bind_vertex_array(Some(&vao));
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&vbo));
        unsafe {
            let view = Float32Array::view(&uv_data);
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &view, GL::STATIC_DRAW);
        }
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, GL::FLOAT, false, 0, 0);
        gl.bind_vertex_array(None);
        gl.bind_buffer(GL::ARRAY_BUFFER, None);

        Ok(Self {
            size,
            buffers: [
                ParticleStateBuffer {
                    tex: state_a,
                    fbo: fbo_a,
                },
                ParticleStateBuffer {
                    tex: state_b,
                    fbo: fbo_b,
                },
            ],
            meta_tex,
            shape_target_tex,
            read_index: 0,
            vao,
            vbo,
            count,
        })
    }

    fn upload_shape_targets(&mut self, gl: &GL, targets_xy: &[f32]) -> Result<(), JsValue> {
        let count = self.count;
        let needed = count * 2;
        if targets_xy.len() < needed {
            return Err(js_err("shape target buffer too small"));
        }
        let mut rgba = vec![0.0f32; count * 4];
        for i in 0..count {
            let src = i * 2;
            let dst = i * 4;
            rgba[dst] = targets_xy[src];
            rgba[dst + 1] = targets_xy[src + 1];
        }
        upload_rgba32f_texture(gl, &self.shape_target_tex, self.size, self.size, &rgba)
    }

    fn destroy(self, gl: &GL) {
        for b in self.buffers {
            gl.delete_framebuffer(Some(&b.fbo));
            gl.delete_texture(Some(&b.tex));
        }
        gl.delete_texture(Some(&self.meta_tex));
        gl.delete_texture(Some(&self.shape_target_tex));
        gl.delete_buffer(Some(&self.vbo));
        gl.delete_vertex_array(Some(&self.vao));
    }
}

impl BrushMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::Pull => "pull",
            Self::Vortex => "vortex",
        }
    }

    fn from_attr(v: &str) -> Option<Self> {
        match v {
            "push" => Some(Self::Push),
            "pull" => Some(Self::Pull),
            "vortex" => Some(Self::Vortex),
            _ => None,
        }
    }

    fn as_i32(self) -> i32 {
        match self {
            Self::Push => 0,
            Self::Pull => 1,
            Self::Vortex => 2,
        }
    }
}

impl QualityMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ultra => "ultra",
            Self::Insane => "insane",
        }
    }

    fn from_attr(v: &str) -> Option<Self> {
        match v {
            "auto" => Some(Self::Auto),
            "ultra" => Some(Self::Ultra),
            "insane" => Some(Self::Insane),
            _ => None,
        }
    }
}

impl FxMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Neon => "neon",
            Self::Prism => "prism",
            Self::Plasma => "plasma",
        }
    }

    fn from_attr(v: &str) -> Option<Self> {
        match v {
            "neon" => Some(Self::Neon),
            "prism" => Some(Self::Prism),
            "plasma" => Some(Self::Plasma),
            _ => None,
        }
    }
}

impl ShapeLayout {
    fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Multi => "multi",
        }
    }

    fn from_attr(v: &str) -> Option<Self> {
        match v {
            "single" => Some(Self::Single),
            "multi" => Some(Self::Multi),
            _ => None,
        }
    }
}

fn quality_preset(mode: QualityMode) -> QualityPreset {
    match mode {
        QualityMode::Auto => QualityPreset {
            max_desktop_tex: 512,
            max_mobile_tex: 384,
            point_scale_mul: 1.0,
        },
        QualityMode::Ultra => QualityPreset {
            max_desktop_tex: 768,
            max_mobile_tex: 576,
            point_scale_mul: 0.94,
        },
        QualityMode::Insane => QualityPreset {
            max_desktop_tex: 2304,
            max_mobile_tex: 1280,
            point_scale_mul: 0.84,
        },
    }
}

fn fx_preset(mode: FxMode) -> FxPreset {
    // Reduced glow effects for sand-like particles - less CPU/GPU work
    match mode {
        FxMode::Neon => FxPreset {
            mode: 0,
            bloom: 0.0,
            chroma: 0.0,
            grain: 0.0,
            bg_pulse: 0.0,
            particle_spark: 0.0,
            ring_gain: 0.0,
            flare: 0.0,
            alpha_gain: 0.85,
            flow_gain: 0.0,
        },
        FxMode::Prism => FxPreset {
            mode: 1,
            bloom: 0.0,
            chroma: 0.0,
            grain: 0.0,
            bg_pulse: 0.0,
            particle_spark: 0.0,
            ring_gain: 0.0,
            flare: 0.0,
            alpha_gain: 0.85,
            flow_gain: 0.0,
        },
        FxMode::Plasma => FxPreset {
            mode: 2,
            bloom: 0.0,
            chroma: 0.0,
            grain: 0.0,
            bg_pulse: 0.0,
            particle_spark: 0.0,
            ring_gain: 0.0,
            flare: 0.0,
            alpha_gain: 0.85,
            flow_gain: 0.0,
        },
    }
}

fn attach_listeners(app: Rc<RefCell<App>>) -> Result<(), JsValue> {
    let window = app.borrow().window.clone();
    let document = app.borrow().document.clone();
    let canvas = app.borrow().canvas.clone();

    if let Some(seg) = document.get_element_by_id("brushSeg") {
        bind_button_group(app.clone(), seg, "brush", move |a, value| {
            if let Some(mode) = BrushMode::from_attr(&value) {
                a.set_brush(mode)
            } else {
                Ok(())
            }
        })?;
    }

    if let Some(seg) = document.get_element_by_id("qualitySeg") {
        bind_button_group(app.clone(), seg, "quality", move |a, value| {
            if let Some(mode) = QualityMode::from_attr(&value) {
                a.set_quality(mode)
            } else {
                Ok(())
            }
        })?;
    }

    if let Some(seg) = document.get_element_by_id("fxSeg") {
        bind_button_group(app.clone(), seg, "fx", move |a, value| {
            if let Some(mode) = FxMode::from_attr(&value) {
                a.set_fx(mode)
            } else {
                Ok(())
            }
        })?;
    }

    if let Some(seg) = document.get_element_by_id("colorSeg") {
        bind_button_group(app.clone(), seg, "color", move |a, value| {
            a.apply_particle_color(&value)
        })?;
    }

    if let Some(seg) = document.get_element_by_id("layoutSeg") {
        bind_button_group(app.clone(), seg, "layout", move |a, value| {
            if let Some(layout) = ShapeLayout::from_attr(&value) {
                a.set_layout(layout)
            } else {
                Ok(())
            }
        })?;
    }

    if let Some(input) = document
        .get_element_by_id("colorInput")
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        let app2 = app.clone();
        let input_for_cb = input.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            let value = input_for_cb.value();
            if let Err(err) = app2.borrow_mut().apply_particle_color(&value) {
                log_error(err);
            }
        }));
        input.add_event_listener_with_callback("input", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(input) = document
        .get_element_by_id("shapeInput")
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::wrap(Box::new(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                e.prevent_default();
                if let Err(err) = app2.borrow_mut().submit_text_entry() {
                    log_error(err);
                }
            } else if e.key() == "Escape" {
                e.prevent_default();
                if let Err(err) = app2.borrow_mut().set_text_entry_open(false, false) {
                    log_error(err);
                }
            }
        }));
        input.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(dot) = document
        .get_element_by_id("textEntryDot")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().set_text_entry_open(true, true) {
                log_error(err);
            }
        }));
        dot.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(btn) = document
        .get_element_by_id("textEntryClose")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().set_text_entry_open(false, false) {
                log_error(err);
            }
        }));
        btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(btn) = document
        .get_element_by_id("textEntryApply")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().submit_text_entry() {
                log_error(err);
            }
        }));
        btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(input) = document
        .get_element_by_id("particleSlider")
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        let app2 = app.clone();
        let input_for_cb = input.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            let value = input_for_cb.value().parse::<i32>().unwrap_or(50);
            if let Err(err) = app2.borrow_mut().set_particle_amount(value) {
                log_error(err);
            }
        }));
        input.add_event_listener_with_callback("input", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(btn) = document
        .get_element_by_id("formBtn")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().trigger_shape_form(None) {
                log_error(err);
            }
        }));
        btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(btn) = document
        .get_element_by_id("meltBtn")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().trigger_shape_melt() {
                log_error(err);
            }
        }));
        btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    if let Some(toggle) = document
        .get_element_by_id("panelToggle")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().toggle_panel() {
                log_error(err);
            }
        }));
        toggle.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().resize() {
                log_error(err);
            }
        }));
        window.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::wrap(Box::new(move |e: PointerEvent| {
            let now = e.time_stamp();
            app2.borrow_mut()
                .on_pointer_move(e.client_x() as f64, e.client_y() as f64, now);
        }));
        canvas.add_event_listener_with_callback("pointermove", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::wrap(Box::new(move |_e: PointerEvent| {
            app2.borrow_mut().on_pointer_leave();
        }));
        canvas.add_event_listener_with_callback("pointerleave", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::wrap(Box::new(move |e: PointerEvent| {
            if e.button() != 0 {
                return;
            }
            let now = e.time_stamp();
            let is_touch = e.pointer_type() == "touch";
            app2.borrow_mut().on_pointer_down(
                e.client_x() as f64,
                e.client_y() as f64,
                now,
                is_touch,
            );
        }));
        canvas.add_event_listener_with_callback("pointerdown", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::wrap(Box::new(move |e: PointerEvent| {
            if e.button() != 0 {
                return;
            }
            let now = e.time_stamp();
            let is_touch = e.pointer_type() == "touch";
            if let Err(err) = app2.borrow_mut().on_pointer_up(
                e.client_x() as f64,
                e.client_y() as f64,
                now,
                is_touch,
            ) {
                log_error(err);
            }
        }));
        canvas.add_event_listener_with_callback("pointerup", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::wrap(Box::new(move |_e: PointerEvent| {
            app2.borrow_mut().on_pointer_cancel();
        }));
        canvas.add_event_listener_with_callback("pointercancel", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(WheelEvent)>::wrap(Box::new(move |e: WheelEvent| {
            e.prevent_default();
            let mut app = app2.borrow_mut();
            let factor = (-e.delta_y() as f32 * 0.0012).exp();
            app.state.attractor.mass = clamp_f32(app.state.attractor.mass * factor, 0.08, 8.0);
        }));
        canvas.add_event_listener_with_callback("wheel", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let cb = Closure::<dyn FnMut(MouseEvent)>::wrap(Box::new(move |e: MouseEvent| {
            e.prevent_default();
        }));
        canvas.add_event_listener_with_callback("contextmenu", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::wrap(Box::new(move |e: KeyboardEvent| {
            if let Err(err) = app2.borrow_mut().handle_keydown(&e) {
                log_error(err);
            }
        }));
        window.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    Ok(())
}

fn bind_button_group<F>(
    app: Rc<RefCell<App>>,
    seg: Element,
    attr: &'static str,
    handler: F,
) -> Result<(), JsValue>
where
    F: 'static + Fn(&mut App, String) -> Result<(), JsValue>,
{
    let buttons = seg.query_selector_all("button")?;
    let handler = Rc::new(handler);
    for i in 0..buttons.length() {
        let Some(node) = buttons.item(i) else {
            continue;
        };
        let Ok(btn) = node.dyn_into::<HtmlElement>() else {
            continue;
        };
        let value = btn
            .get_attribute(&format!("data-{attr}"))
            .unwrap_or_default();
        let app2 = app.clone();
        let handler2 = handler.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = handler2(&mut app2.borrow_mut(), value.clone()) {
                log_error(err);
            }
        }));
        btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    Ok(())
}

fn parse_story_mode_from_url(app: &mut App) {
    let search = match app.window.location().search() {
        Ok(s) => s,
        Err(_) => return,
    };
    let search = search.trim_start_matches('?');
    let mut story = false;
    let mut text = String::new();
    for part in search.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == "m" && (v == "1" || v.eq_ignore_ascii_case("true")) {
                story = true;
            } else if k == "t" {
                text = percent_decode_query(v);
            }
        }
    }
    if story {
        app.state.story_mode = true;
        let msg = normalize_shape_text(&text, false);
        app.state.shape.text = if msg.is_empty() {
            INTRO_TITLE.to_string()
        } else {
            msg
        };
    }
}

fn percent_decode_query(input: &str) -> String {
    let mut out = String::new();
    let mut bytes = input.bytes();
    while let Some(b) = bytes.next() {
        if b == b'+' {
            out.push(' ');
        } else if b == b'%' {
            let h = bytes.next().and_then(|c| hex_val(c));
            let l = bytes.next().and_then(|c| hex_val(c));
            if let (Some(hi), Some(lo)) = (h, l) {
                out.push((hi << 4 | lo) as char);
            } else {
                out.push('%');
            }
        } else {
            out.push(b as char);
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

fn invoke_should_run_story() -> bool {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return true,
    };
    let key = JsValue::from_str("rustypartsShouldRunStory");
    if let Ok(v) = Reflect::get(window.as_ref(), &key) {
        if let Ok(f) = v.dyn_into::<js_sys::Function>() {
            if let Ok(result) = f.call0(&JsValue::NULL) {
                return result.as_bool().unwrap_or(true);
            }
        }
    }
    true
}

fn invoke_show_gone() {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let key = JsValue::from_str("rustypartsShowGone");
    if let Ok(v) = Reflect::get(window.as_ref(), &key) {
        if let Ok(f) = v.dyn_into::<js_sys::Function>() {
            let _ = f.call0(&JsValue::NULL);
        }
    }
}

fn invoke_story_complete_callback() {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let key = JsValue::from_str("rustypartsOnStoryComplete");
    if let Ok(v) = Reflect::get(window.as_ref(), &key) {
        if let Ok(f) = v.dyn_into::<js_sys::Function>() {
            let _ = f.call0(&JsValue::NULL);
        }
    }
}

fn start_animation_loop(app: Rc<RefCell<App>>) -> Result<(), JsValue> {
    let raf_cell = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let raf_cell_for_cb = raf_cell.clone();

    *raf_cell.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::wrap(Box::new(
        move |now_ms: f64| {
            if let Err(err) = app.borrow_mut().frame(now_ms) {
                log_error(err);
                return;
            }
            if app.borrow().state.story_finished {
                return;
            }
            if let Some(win) = web_sys::window()
                && let Some(cb) = raf_cell_for_cb.borrow().as_ref()
            {
                let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        },
    )));

    let window = web_sys::window().ok_or_else(|| js_err("window unavailable"))?;
    if let Some(cb) = raf_cell.borrow().as_ref() {
        window.request_animation_frame(cb.as_ref().unchecked_ref())?;
    }

    // Leak the closure container for the page lifetime so RAF can reschedule itself.
    std::mem::forget(raf_cell);

    Ok(())
}

fn set_active_button(seg: &Option<Element>, key: &str, value: &str) -> Result<(), JsValue> {
    let Some(seg) = seg else {
        return Ok(());
    };
    let buttons = seg.query_selector_all("button")?;
    for i in 0..buttons.length() {
        let Some(node) = buttons.item(i) else {
            continue;
        };
        let Ok(el) = node.dyn_into::<Element>() else {
            continue;
        };
        let active = el
            .get_attribute(&format!("data-{key}"))
            .map(|v| v.eq_ignore_ascii_case(value))
            .unwrap_or(false);
        el.class_list().toggle_with_force("is-active", active)?;
    }
    Ok(())
}

fn is_typing_target(target: Option<EventTarget>) -> bool {
    let Some(target) = target else {
        return false;
    };
    let Ok(el) = target.dyn_into::<Element>() else {
        return false;
    };
    let tag = el.tag_name();
    if matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT") {
        return true;
    }
    el.dyn_into::<HtmlElement>()
        .ok()
        .map(|h| h.is_content_editable())
        .unwrap_or(false)
}

fn normalize_shape_text(input: &str, fallback_on_empty: bool) -> String {
    let mut out = String::new();
    for ch in input.trim().chars() {
        if out.chars().count() >= 18 {
            break;
        }
        let up = ch.to_ascii_uppercase();
        if up.is_ascii_alphanumeric() || matches!(up, ' ' | '!' | '?' | '-' | '.') {
            out.push(up);
        } else if up.is_ascii_whitespace() {
            out.push(' ');
        }
    }
    let squashed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.is_empty() && fallback_on_empty {
        DEFAULT_SHAPE_TEXT.to_string()
    } else {
        squashed
    }
}

/// Max characters on one line before stacking; also stack when text has spaces (multiple words).
const SINGLE_LINE_MAX_CHARS: usize = 10;

fn build_shape_targets(
    document: &Document,
    text: &str,
    layout: ShapeLayout,
    count: usize,
) -> Vec<f32> {
    let mut samples = Vec::<(f32, f32)>::new();
    let cleaned = normalize_shape_text(text, true);
    let char_count = cleaned.chars().count();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let stack_words = layout == ShapeLayout::Single
        && (words.len() > 1 || char_count > SINGLE_LINE_MAX_CHARS);

    match layout {
        ShapeLayout::Single if stack_words => {
            let n = words.len().max(1);
            let total_height = 0.88f32;
            let line_height = total_height / (n as f32);
            for (i, word) in words.iter().enumerate() {
                let word_points = raster_text_points(document, word)
                    .unwrap_or_else(|_| raster_text_cells(word));
                let center_y = 0.02 + total_height * 0.5 - (i as f32 + 0.5) * line_height;
                push_text_stamp(&mut samples, &word_points, 0.0, center_y, 1.38, line_height * 0.95, 2);
            }
        }
        ShapeLayout::Single => {
            let glyph_points =
                raster_text_points(document, &cleaned).unwrap_or_else(|_| raster_text_cells(&cleaned));
            push_text_stamp(&mut samples, &glyph_points, 0.0, 0.02, 1.38, 0.88, 2);
        }
        ShapeLayout::Multi => {
            let glyph_points =
                raster_text_points(document, &cleaned).unwrap_or_else(|_| raster_text_cells(&cleaned));
            let placements: &[(f32, f32, f32, f32)] = &[
                (-0.62, 0.34, 0.92, 0.42),
                (0.57, 0.31, 0.78, 0.38),
                (-0.14, -0.06, 1.02, 0.46),
                (0.62, -0.35, 0.68, 0.34),
                (-0.66, -0.42, 0.72, 0.34),
            ];
            for (idx, &(cx, cy, w, h)) in placements.iter().enumerate() {
                let copies = if idx == 2 { 2 } else { 1 };
                push_text_stamp(&mut samples, &glyph_points, cx, cy, w, h, copies);
            }
        }
    }

    if samples.is_empty() {
        samples.push((0.0, 0.0));
    }

    let mut out = vec![0.0f32; count * 2];
    for i in 0..count {
        let idx = (i * 131 + (i / 7) * 17) % samples.len();
        let (sx, sy) = samples[idx];
        let jx =
            (hash01_u32((i as u32).wrapping_mul(1664525).wrapping_add(1013904223)) - 0.5) * 0.004;
        let jy = (hash01_u32((i as u32).wrapping_mul(22695477).wrapping_add(1)) - 0.5) * 0.004;
        out[i * 2] = sx + jx;
        out[i * 2 + 1] = sy + jy;
    }
    out
}

fn push_text_stamp(
    out: &mut Vec<(f32, f32)>,
    points: &[(f32, f32)],
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    density: usize,
) {
    if points.is_empty() {
        return;
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in points {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    let span_x = (max_x - min_x + 1.0).max(1.0);
    let span_y = (max_y - min_y + 1.0).max(1.0);
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let sx = width / span_x;
    let sy = height / span_y;

    let density = density.max(1);
    for &(x, y) in points {
        let base_x = center_x + (x - cx) * sx;
        let base_y = center_y - (y - cy) * sy;
        let cell_seed = (x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b);
        for k in 0..density {
            let sample_seed = cell_seed ^ (k as u32).wrapping_mul(0x27d4_eb2d);
            let jitter_x = (hash01_u32(sample_seed ^ 0x1b56_c4e9) - 0.5) * sx * 0.22;
            let jitter_y = (hash01_u32(sample_seed ^ 0xc2b2_ae35) - 0.5) * sy * 0.22;
            let ox = jitter_x;
            let oy = jitter_y;
            out.push((base_x + ox, base_y + oy));
        }
    }
}

fn raster_text_points(document: &Document, text: &str) -> Result<Vec<(f32, f32)>, JsValue> {
    const CANVAS_W: u32 = 1200;
    const CANVAS_H: u32 = 360;
    const SAMPLE_STEP: usize = 2;
    const ALPHA_THRESHOLD: u8 = 20;

    let canvas = document
        .create_element("canvas")?
        .dyn_into::<HtmlCanvasElement>()?;
    canvas.set_width(CANVAS_W);
    canvas.set_height(CANVAS_H);

    let Some(ctx_any) = canvas.get_context("2d")? else {
        return Err(js_err("2d canvas context unavailable"));
    };
    let ctx = ctx_any.dyn_into::<CanvasRenderingContext2d>()?;

    let w = CANVAS_W as f64;
    let h = CANVAS_H as f64;
    ctx.clear_rect(0.0, 0.0, w, h);
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    ctx.set_fill_style_str("#ffffff");

    let chars = text.chars().count().max(1) as f64;
    let font_by_height = h * 0.72;
    let font_by_width = (w * 0.90) / (chars * 0.58);
    let font_px = font_by_height.min(font_by_width).max(40.0);
    ctx.set_font(&format!(
        "900 {:.0}px \"Trebuchet MS\", \"Arial Black\", sans-serif",
        font_px
    ));
    ctx.fill_text(text, w * 0.5, h * 0.53)?;

    let rgba = ctx.get_image_data(0.0, 0.0, w, h)?.data().0.to_vec();

    let mut points = Vec::new();
    let width_px = CANVAS_W as usize;
    let height_px = CANVAS_H as usize;
    for y in (0..height_px).step_by(SAMPLE_STEP) {
        let row_base = y * width_px * 4;
        for x in (0..width_px).step_by(SAMPLE_STEP) {
            let a = rgba[row_base + x * 4 + 3];
            if a >= ALPHA_THRESHOLD {
                points.push((x as f32, y as f32));
            }
        }
    }

    if points.is_empty() {
        return Err(js_err("rasterized text produced no points"));
    }
    Ok(points)
}

fn raster_text_cells(text: &str) -> Vec<(f32, f32)> {
    let mut cells = Vec::new();
    let mut cursor = 0i32;
    for ch in text.chars() {
        if ch == ' ' {
            cursor += 3;
            continue;
        }
        let rows = glyph_5x7_rows(ch);
        for (yy, row) in rows.iter().enumerate() {
            for xx in 0..5usize {
                let bit = (*row >> (4 - xx)) & 1;
                if bit == 1 {
                    cells.push(((cursor + xx as i32) as f32, yy as f32));
                }
            }
        }
        cursor += 6;
    }
    cells
}

fn glyph_5x7_rows(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        _ => [
            0b11111, 0b00001, 0b00110, 0b00100, 0b00000, 0b00100, 0b00100,
        ],
    }
}

fn hash01_u32(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x as f32) / (u32::MAX as f32)
}

fn create_shader(gl: &GL, shader_type: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl
        .create_shader(shader_type)
        .ok_or_else(|| js_err("failed to create shader"))?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    let ok = gl
        .get_shader_parameter(&shader, GL::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false);
    if ok {
        return Ok(shader);
    }
    let info = gl
        .get_shader_info_log(&shader)
        .unwrap_or_else(|| "Shader compile failed".to_string());
    gl.delete_shader(Some(&shader));
    Err(js_err(&info))
}

fn create_program(gl: &GL, vs_source: &str, fs_source: &str) -> Result<WebGlProgram, JsValue> {
    let program = gl
        .create_program()
        .ok_or_else(|| js_err("failed to create program"))?;
    let vs = create_shader(gl, GL::VERTEX_SHADER, vs_source)?;
    let fs = create_shader(gl, GL::FRAGMENT_SHADER, fs_source)?;
    gl.attach_shader(&program, &vs);
    gl.attach_shader(&program, &fs);
    gl.link_program(&program);
    gl.delete_shader(Some(&vs));
    gl.delete_shader(Some(&fs));

    let ok = gl
        .get_program_parameter(&program, GL::LINK_STATUS)
        .as_bool()
        .unwrap_or(false);
    if ok {
        return Ok(program);
    }
    let info = gl
        .get_program_info_log(&program)
        .unwrap_or_else(|| "Program link failed".to_string());
    gl.delete_program(Some(&program));
    Err(js_err(&info))
}

fn build_particle_uv_data(size: i32) -> Vec<f32> {
    let size_us = size.max(1) as usize;
    let count = size_us * size_us;
    let mut uv = vec![0.0f32; count * 2];
    let mut k = 0usize;
    for y in 0..size_us {
        for x in 0..size_us {
            uv[k] = (x as f32 + 0.5) / size_us as f32;
            uv[k + 1] = (y as f32 + 0.5) / size_us as f32;
            k += 2;
        }
    }
    uv
}

fn create_rgba32f_texture(
    gl: &GL,
    width: i32,
    height: i32,
    data: Option<&[f32]>,
) -> Result<WebGlTexture, JsValue> {
    let tex = gl
        .create_texture()
        .ok_or_else(|| js_err("failed to create texture"))?;
    gl.bind_texture(GL::TEXTURE_2D, Some(&tex));
    gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_MIN_FILTER, GL::NEAREST as i32);
    gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_MAG_FILTER, GL::NEAREST as i32);
    gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_WRAP_S, GL::CLAMP_TO_EDGE as i32);
    gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_WRAP_T, GL::CLAMP_TO_EDGE as i32);

    match data {
        Some(data) => unsafe {
            let view = Float32Array::view(data);
            let view_obj: &js_sys::Object = view.unchecked_ref();
            gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(
                GL::TEXTURE_2D,
                0,
                GL::RGBA32F as i32,
                width,
                height,
                0,
                GL::RGBA,
                GL::FLOAT,
                Some(view_obj),
            )?;
        },
        None => {
            gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(
                GL::TEXTURE_2D,
                0,
                GL::RGBA32F as i32,
                width,
                height,
                0,
                GL::RGBA,
                GL::FLOAT,
                None,
            )?;
        }
    }

    gl.bind_texture(GL::TEXTURE_2D, None);
    Ok(tex)
}

fn upload_rgba32f_texture(
    gl: &GL,
    tex: &WebGlTexture,
    width: i32,
    height: i32,
    data: &[f32],
) -> Result<(), JsValue> {
    gl.bind_texture(GL::TEXTURE_2D, Some(tex));
    unsafe {
        let view = Float32Array::view(data);
        let view_obj: &js_sys::Object = view.unchecked_ref();
        gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_array_buffer_view(
            GL::TEXTURE_2D,
            0,
            0,
            0,
            width,
            height,
            GL::RGBA,
            GL::FLOAT,
            Some(view_obj),
        )?;
    }
    gl.bind_texture(GL::TEXTURE_2D, None);
    Ok(())
}

fn create_color_framebuffer(gl: &GL, tex: &WebGlTexture) -> Result<WebGlFramebuffer, JsValue> {
    let fbo = gl
        .create_framebuffer()
        .ok_or_else(|| js_err("failed to create framebuffer"))?;
    gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&fbo));
    gl.framebuffer_texture_2d(
        GL::FRAMEBUFFER,
        GL::COLOR_ATTACHMENT0,
        GL::TEXTURE_2D,
        Some(tex),
        0,
    );
    let status = gl.check_framebuffer_status(GL::FRAMEBUFFER);
    gl.bind_framebuffer(GL::FRAMEBUFFER, None);
    if status != GL::FRAMEBUFFER_COMPLETE {
        return Err(js_err(&format!("framebuffer incomplete: 0x{status:04x}")));
    }
    Ok(fbo)
}

fn choose_particle_tex_from_ladder(target: i32, cap: i32) -> i32 {
    let target = target.max(128);
    let cap = cap.max(128);
    for &size in PARTICLE_TEX_LADDER.iter().rev() {
        if size <= cap && size <= target {
            return size;
        }
    }
    128
}

fn parse_debug_tex_override(document: &Document) -> Option<i32> {
    let url = document.url().ok()?;
    let (_, query_and_hash) = url.split_once('?')?;
    let query = query_and_hash.split('#').next().unwrap_or(query_and_hash);

    for pair in query.split('&') {
        let (key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        if !matches!(
            key.to_ascii_lowercase().as_str(),
            "tex" | "particle_tex" | "ptex"
        ) {
            continue;
        }
        let value = raw_value.trim();
        if let Ok(v) = value.parse::<i32>() {
            return Some(v.max(1));
        }
    }
    None
}

fn normalize_hex_color(input: &str) -> Option<String> {
    let s = input.trim();
    let stripped = s.strip_prefix('#').unwrap_or(s);
    if stripped.len() != 6 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", stripped.to_ascii_lowercase()))
}

fn hex_to_rgb01(hex: &str) -> [f32; 3] {
    let Some(h) = normalize_hex_color(hex) else {
        return [0.6078, 1.0, 0.702];
    };
    let v = u32::from_str_radix(&h[1..], 16).unwrap_or(0x9bffb3);
    [
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
    ]
}

fn rgb01_to_hex(rgb: [f32; 3]) -> String {
    let r = (clamp_f32(rgb[0], 0.0, 1.0) * 255.0).round() as u8;
    let g = (clamp_f32(rgb[1], 0.0, 1.0) * 255.0).round() as u8;
    let b = (clamp_f32(rgb[2], 0.0, 1.0) * 255.0).round() as u8;
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn random_two_tone_pair(rng: &mut Lcg) -> ([f32; 3], [f32; 3]) {
    let h1 = rng.f32();
    let hue_gap = 0.34 + 0.24 * rng.f32();
    let sign = if rng.f32() > 0.5 { 1.0 } else { -1.0 };
    let h2 = wrap01(h1 + sign * hue_gap);

    let s1 = 0.76 + 0.22 * rng.f32();
    let v1 = 0.84 + 0.16 * rng.f32();
    let s2 = 0.72 + 0.26 * rng.f32();
    let mut v2 = 0.48 + 0.42 * rng.f32();

    let color_a = hsv_to_rgb(h1, s1, v1);
    let mut color_b = hsv_to_rgb(h2, s2, v2);
    let lum_delta = (relative_luma(color_a) - relative_luma(color_b)).abs();
    if lum_delta < 0.22 {
        v2 = if relative_luma(color_a) > 0.5 {
            (v2 * 0.56).max(0.26)
        } else {
            (v2 * 1.34 + 0.1).min(1.0)
        };
        color_b = hsv_to_rgb(h2, s2, v2);
    }

    (color_a, color_b)
}

fn rgb_to_hsv(rgb: [f32; 3]) -> (f32, f32, f32) {
    let r = rgb[0];
    let g = rgb[1];
    let b = rgb[2];
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let delta = max - min;
    if delta <= 1e-6 {
        return (0.0, 0.0, max);
    }

    let mut h = if (max - r).abs() <= 1e-6 {
        (g - b) / delta
    } else if (max - g).abs() <= 1e-6 {
        ((b - r) / delta) + 2.0
    } else {
        ((r - g) / delta) + 4.0
    };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    let s = if max <= 1e-6 { 0.0 } else { delta / max };
    (h, s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = wrap01(h) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match i.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn relative_luma(rgb: [f32; 3]) -> f32 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}

fn wrap01(v: f32) -> f32 {
    let mut x = v % 1.0;
    if x < 0.0 {
        x += 1.0;
    }
    x
}

fn format_particle_count(count: usize) -> String {
    if count >= 1_000_000 {
        format!("{:.2}M", count as f64 / 1_000_000.0)
    } else if count >= 1000 {
        let precision = if count >= 100_000 { 0 } else { 1 };
        format!("{:.*}k", precision, count as f64 / 1000.0)
    } else {
        count.to_string()
    }
}

fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

fn smoothstep_f32(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = clamp_f32((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn js_err(msg: &str) -> JsValue {
    JsValue::from_str(msg)
}

fn log_error(err: JsValue) {
    web_sys::console::error_1(&err);
}

// #region agent log
fn debug_log(location: &str, message: &str, hypothesis_id: &str, data: &[(&str, f64)]) {
    let _ = (|| -> Result<(), JsValue> {
        let obj = js_sys::Object::new();
        Reflect::set(&obj, &"sessionId".into(), &"759d2c".into())?;
        Reflect::set(&obj, &"location".into(), &JsValue::from_str(location))?;
        Reflect::set(&obj, &"message".into(), &JsValue::from_str(message))?;
        Reflect::set(&obj, &"timestamp".into(), &js_sys::Date::now().into())?;
        Reflect::set(&obj, &"hypothesisId".into(), &JsValue::from_str(hypothesis_id))?;
        let data_obj = js_sys::Object::new();
        for (k, v) in data {
            Reflect::set(&data_obj, &(*k).into(), &JsValue::from_f64(*v))?;
        }
        Reflect::set(&obj, &"data".into(), &data_obj)?;
        let json = js_sys::JSON::stringify(&obj)?;
        let global = js_sys::global().dyn_into::<js_sys::Object>()?;
        let log_fn = Reflect::get(&global, &"rustypartsDebugLog".into())?;
        if let Some(f) = log_fn.dyn_ref::<js_sys::Function>() {
            f.call1(&JsValue::NULL, &json)?;
        }
        Ok(())
    })();
}
// #endregion

struct Lcg {
    state: u64,
}

impl Lcg {
    fn seeded() -> Self {
        let seed = (Math::random() * (u64::MAX as f64)) as u64 ^ 0x9E37_79B9_7F4A_7C15;
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }

    fn f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }
}
