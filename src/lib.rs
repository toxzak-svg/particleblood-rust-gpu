use std::cell::RefCell;
use std::f32::consts::TAU;
use std::rc::Rc;

use js_sys::{Float32Array, Math};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    Document, Element, Event, EventTarget, HtmlCanvasElement, HtmlElement, HtmlInputElement,
    KeyboardEvent, MouseEvent, PointerEvent, WebGl2RenderingContext as GL, WebGlBuffer,
    WebGlFramebuffer, WebGlProgram, WebGlShader, WebGlTexture, WebGlUniformLocation,
    WebGlVertexArrayObject, WheelEvent, Window,
};

thread_local! {
    static APP_HOLDER: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

const TAP_TRIGGER_MAX_MS: f64 = 260.0;
const TAP_TRIGGER_MAX_MOVE_PX: f32 = 16.0;
const SHAPE_FORM_DURATION_S: f64 = 2.3;
const DEFAULT_SHAPE_TEXT: &str = "TOUCH!";
const PARTICLE_RESOLUTION_SCALE: f64 = 1.30;
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

void main() {
  vec3 color = vec3(0.0);
  if (uFx.z > 0.001) {
    float grain = hash12(vUv * uResolution + fract(uTime * 60.0));
    color += (grain - 0.5) * (0.004 + 0.018 * uFx.z);
  }
  color = max(color, 0.0);
  outColor = vec4(color, 1.0);
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
uniform vec3 uTint;
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
  
  // Sand/earth tones - more muted, matte colors
  vec3 baseA = vec3(0.76, 0.70, 0.55); // tan/sand
  vec3 baseB = vec3(0.82, 0.65, 0.45);  // ochre
  vec3 baseC = vec3(0.65, 0.55, 0.45);  // brownish
  vec3 col = mix(baseA, baseB, smoothstep(0.0, 1.0, seed));
  col = mix(col, baseC, smoothstep(0.5, 1.0, speed * 0.5));
  col = mix(col, uTint, 0.35);

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

  if (uAttractor.w > 0.5) {
    applyMagnet(acc, p, uAttractor.xy, uAttractor.z, uAttractorSpin, 0.025);
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

  if (uShape.w > 0.5) {
    vec2 target = texture(uShapeTargetTex, uv).xy;
    target.x *= aspect;
    vec2 d = target - p;
    float dist2 = dot(d, d);
    float fall = exp(-dist2 * 4.0);
    acc += d * (uShape.x * uShape.y);
    acc += vec2(-d.y, d.x) * (uShape.x * uShape.z * (0.2 + 0.8 * fall));
    float damp = 1.0 - clamp(0.035 * uShape.x * fall, 0.0, 0.08);
    v *= damp;
  }

  v += acc * dt;
    float damping = pow(0.993, dt * 60.0);
  v *= damping;
  float speed = length(v);
    float maxSpeed = 3.8 + uFx.y * 0.6;
  if (speed > maxSpeed) {
    v *= maxSpeed / max(speed, 1e-6);
  }

  p += v * dt;

    float breathRadius = 0.03 * melt * (0.5 + 0.5 * sin(t * 0.52 + phase * 6.2831));
    vec2 bounds = vec2((1.12 + breathRadius) * aspect, 1.12 + breathRadius);
    if (p.x > bounds.x) { p.x = bounds.x; v.x *= -0.78; }
    else if (p.x < -bounds.x) { p.x = -bounds.x; v.x *= -0.78; }
    if (p.y > bounds.y) { p.y = bounds.y; v.y *= -0.78; }
    else if (p.y < -bounds.y) { p.y = -bounds.y; v.y *= -0.78; }

  outState = vec4(p, v);
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

struct AppState {
    time: f64,
    last_time: f64,
    dpr: f64,
    width: i32,
    height: i32,
    brush: BrushMode,
    quality: QualityMode,
    fx: FxMode,
    color_hex: String,
    color_rgb: [f32; 3],
    pointer: PointerState,
    attractor: AttractorState,
    shape: ShapeState,
    mouse_tap: MouseTapState,
    stats: StatsState,
}

struct UiRefs {
    brush_seg: Option<Element>,
    quality_seg: Option<Element>,
    fx_seg: Option<Element>,
    color_seg: Option<Element>,
    color_input: Option<HtmlInputElement>,
    color_hex: Option<HtmlElement>,
    particle_count_out: Option<HtmlElement>,
    shape_input: Option<HtmlInputElement>,
    layout_seg: Option<Element>,
    form_btn: Option<HtmlElement>,
    melt_btn: Option<HtmlElement>,
    control_panel: Option<Element>,
    panel_toggle: Option<HtmlElement>,
}

struct Programs {
    background: WebGlProgram,
    sim: WebGlProgram,
    particle: WebGlProgram,
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
            state: AppState {
                time: 0.0,
                last_time: 0.0,
                dpr: 1.0,
                width: 1,
                height: 1,
                brush: BrushMode::Push,
                quality: QualityMode::Insane,
                fx: FxMode::Neon,
                color_hex: "#9bffb3".to_string(),
                color_rgb: hex_to_rgb01("#9bffb3"),
                pointer: PointerState {
                    radius: 0.18,
                    strength: 1.0,
                    ..PointerState::default()
                },
                attractor: AttractorState {
                    enabled: false,
                    x: 0.0,
                    y: 0.0,
                    mass: 1.8,
                    spin: 0.45,
                },
                shape: ShapeState {
                    text: DEFAULT_SHAPE_TEXT.to_string(),
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
                stats: StatsState {
                    fps: 60.0,
                    sample_accum: 0.0,
                    frame_accum: 0.0,
                    samples: 0,
                },
            },
        };

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
        let mut target = if area < 320_000.0 {
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

    fn frame(&mut self, now_ms: f64) -> Result<(), JsValue> {
        let now = now_ms * 0.001;
        if self.state.last_time == 0.0 {
            self.state.last_time = now;
        }
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

    fn step_particles(&mut self, dt: f32) {
        let Some(ps) = self.particles.as_ref() else {
            return;
        };

        let fx = fx_preset(self.state.fx);
        let pointer_recent = self.state.pointer.active
            && (self.state.time - self.state.pointer.last_seen_ms * 0.001) < 0.15;
        let shape_mix = self.state.shape.mix.clamp(0.0, 1.0);
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
            self.state.attractor.mass,
            if self.state.attractor.enabled {
                1.0
            } else {
                0.0
            },
        );
        self.gl.uniform1f(
            self.uniform(&self.programs.sim, "uAttractorSpin").as_ref(),
            self.state.attractor.spin,
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
        let point_scale =
            (self.state.dpr as f32 * 2.28 * quality_preset(self.state.quality).point_scale_mul)
                .clamp(1.35, 4.8);
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
            self.uniform(&self.programs.particle, "uTint").as_ref(),
            self.state.color_rgb[0],
            self.state.color_rgb[1],
            self.state.color_rgb[2],
        );
        self.gl.uniform4f(
            self.uniform(&self.programs.particle, "uFx").as_ref(),
            fx.particle_spark,
            fx.ring_gain,
            fx.flare,
            fx.alpha_gain,
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

    fn on_pointer_down(&mut self, client_x: f64, client_y: f64, now_ms: f64) {
        self.on_pointer_move(client_x, client_y, now_ms);
        self.state.pointer.down = true;
        self.state.attractor.enabled = true;
        self.state.attractor.x = self.state.pointer.nx;
        self.state.attractor.y = self.state.pointer.ny;
        self.state.mouse_tap.active = true;
        self.state.mouse_tap.x = client_x as f32;
        self.state.mouse_tap.y = client_y as f32;
        self.state.mouse_tap.t_ms = now_ms;
        self.state.mouse_tap.moved = false;
    }

    fn on_pointer_up(&mut self, now_ms: f64) -> Result<(), JsValue> {
        self.state.pointer.down = false;
        self.state.attractor.enabled = false;
        let dt = now_ms - self.state.mouse_tap.t_ms;
        if self.state.mouse_tap.active && !self.state.mouse_tap.moved && dt <= TAP_TRIGGER_MAX_MS {
            self.toggle_shape_word()?;
        }
        self.state.mouse_tap.active = false;
        Ok(())
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

    fn apply_particle_color(&mut self, hex: &str) -> Result<(), JsValue> {
        let normalized = normalize_hex_color(hex).unwrap_or_else(|| "#9bffb3".to_string());
        self.state.color_rgb = hex_to_rgb01(&normalized);
        self.state.color_hex = normalized.clone();

        if let Some(input) = &self.ui.color_input {
            input.set_value(&normalized);
        }
        if let Some(out) = &self.ui.color_hex {
            out.set_text_content(Some(&normalized.to_ascii_uppercase()));
        }
        if let Some(root) = self.document.document_element() {
            if let Ok(root_el) = root.dyn_into::<HtmlElement>() {
                let _ = root_el.style().set_property("--accent", &normalized);
            }
        }

        set_active_button(&self.ui.color_seg, "color", &normalized)?;
        Ok(())
    }

    fn set_layout(&mut self, layout: ShapeLayout) -> Result<(), JsValue> {
        self.state.shape.layout = layout;
        if let Some(input) = &self.ui.shape_input {
            self.state.shape.text = normalize_shape_text(&input.value(), true);
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
            self.state.shape.text = normalize_shape_text(&input.value(), true);
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
        self.update_shape_action_buttons()
    }

    fn trigger_shape_melt(&mut self) -> Result<(), JsValue> {
        self.state.shape.target_mix = 0.0;
        self.state.shape.release_at = 0.0;
        self.update_shape_action_buttons()
    }

    fn toggle_shape_word(&mut self) -> Result<(), JsValue> {
        if self.is_shape_word_visible() {
            self.trigger_shape_melt()
        } else {
            self.trigger_shape_form(Some(0.0))
        }
    }

    fn sync_shape_text_from_input(&mut self, fallback_on_empty: bool) -> Result<(), JsValue> {
        let Some(input) = self.ui.shape_input.clone() else {
            return Ok(());
        };
        self.state.shape.text = normalize_shape_text(&input.value(), fallback_on_empty);
        if fallback_on_empty || !self.state.shape.text.is_empty() {
            input.set_value(&self.state.shape.text);
        }
        self.rebuild_shape_targets()?;
        self.update_shape_action_buttons()
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
        let targets = build_shape_targets(&text, layout, count);

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
        if let Some(input) = &self.ui.shape_input {
            input.set_value(&self.state.shape.text);
        }
        self.apply_particle_color(&self.state.color_hex.clone())?;
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
            particle_count_out: document
                .get_element_by_id("particleCountOut")
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
        })
    }
}

impl Programs {
    fn new(gl: &GL) -> Result<Self, JsValue> {
        Ok(Self {
            background: create_program(gl, FULLSCREEN_VS, BACKGROUND_FS)?,
            sim: create_program(gl, FULLSCREEN_VS, SIM_FS)?,
            particle: create_program(gl, PARTICLE_VS, PARTICLE_FS)?,
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
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().sync_shape_text_from_input(false) {
                log_error(err);
            }
        }));
        input.add_event_listener_with_callback("input", cb.as_ref().unchecked_ref())?;
        cb.forget();

        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e: Event| {
            if let Err(err) = app2.borrow_mut().sync_shape_text_from_input(true) {
                log_error(err);
            }
        }));
        input.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref())?;
        cb.forget();

        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::wrap(Box::new(move |e: KeyboardEvent| {
            if e.key() != "Enter" {
                return;
            }
            e.prevent_default();
            if let Err(err) = app2.borrow_mut().trigger_shape_form(None) {
                log_error(err);
            }
        }));
        input.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())?;
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
            app2.borrow_mut()
                .on_pointer_down(e.client_x() as f64, e.client_y() as f64, now);
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
            if let Err(err) = app2.borrow_mut().on_pointer_up(now) {
                log_error(err);
            }
        }));
        canvas.add_event_listener_with_callback("pointerup", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    {
        let app2 = app.clone();
        let cb = Closure::<dyn FnMut(WheelEvent)>::wrap(Box::new(move |e: WheelEvent| {
            e.prevent_default();
            let mut app = app2.borrow_mut();
            let factor = (-e.delta_y() as f32 * 0.0012).exp();
            app.state.attractor.mass = clamp_f32(app.state.attractor.mass * factor, 0.08, 2.2);
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

fn start_animation_loop(app: Rc<RefCell<App>>) -> Result<(), JsValue> {
    let raf_cell = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let raf_cell_for_cb = raf_cell.clone();

    *raf_cell.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::wrap(Box::new(
        move |now_ms: f64| {
            if let Err(err) = app.borrow_mut().frame(now_ms) {
                log_error(err);
                return;
            }
            if let Some(win) = web_sys::window() {
                if let Some(cb) = raf_cell_for_cb.borrow().as_ref() {
                    let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
                }
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

fn build_shape_targets(text: &str, layout: ShapeLayout, count: usize) -> Vec<f32> {
    let mut samples = Vec::<(f32, f32)>::new();
    let cleaned = normalize_shape_text(text, true);

    match layout {
        ShapeLayout::Single => {
            push_text_stamp(&mut samples, &cleaned, 0.0, 0.02, 1.38, 0.88, 8);
        }
        ShapeLayout::Multi => {
            let stamp = cleaned;
            let placements: &[(f32, f32, f32, f32)] = &[
                (-0.62, 0.34, 0.92, 0.42),
                (0.57, 0.31, 0.78, 0.38),
                (-0.14, -0.06, 1.02, 0.46),
                (0.62, -0.35, 0.68, 0.34),
                (-0.66, -0.42, 0.72, 0.34),
            ];
            for (idx, &(cx, cy, w, h)) in placements.iter().enumerate() {
                let copies = if idx == 2 { 6 } else { 4 };
                push_text_stamp(&mut samples, &stamp, cx, cy, w, h, copies);
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
            (hash01_u32((i as u32).wrapping_mul(1664525).wrapping_add(1013904223)) - 0.5) * 0.0025;
        let jy = (hash01_u32((i as u32).wrapping_mul(22695477).wrapping_add(1)) - 0.5) * 0.0025;
        out[i * 2] = sx + jx;
        out[i * 2 + 1] = sy + jy;
    }
    out
}

fn push_text_stamp(
    out: &mut Vec<(f32, f32)>,
    text: &str,
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    density: usize,
) {
    let cells = raster_text_cells(text);
    if cells.is_empty() {
        return;
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in &cells {
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
    let side = (density as f32).sqrt().ceil() as i32;
    for &(x, y) in &cells {
        let base_x = center_x + (x - cx) * sx;
        let base_y = center_y - (y - cy) * sy;
        for k in 0..density {
            let gx = (k as i32 % side) as f32;
            let gy = (k as i32 / side) as f32;
            let fx = if side > 1 {
                gx / (side - 1) as f32 - 0.5
            } else {
                0.0
            };
            let fy = if side > 1 {
                gy / (side - 1) as f32 - 0.5
            } else {
                0.0
            };
            out.push((base_x + fx * sx * 0.7, base_y + fy * sy * 0.7));
        }
    }
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

fn js_err(msg: &str) -> JsValue {
    JsValue::from_str(msg)
}

fn log_error(err: JsValue) {
    web_sys::console::error_1(&err);
}

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
