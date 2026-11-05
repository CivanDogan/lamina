struct U {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // 2D: x_clip = x * s.x + s.z, y_clip = y * s.y + s.w
    view2d: vec4<f32>,
    params: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bot: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: U;

// ---------------------------------------------------------------- background

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: BgOut;
    o.pos = vec4<f32>(p[i], 0.0, 1.0);
    o.uv = p[i] * 0.5 + vec2<f32>(0.5, 0.5);
    return o;
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    let c = mix(u.bg_bot.rgb, u.bg_top.rgb, clamp(in.uv.y, 0.0, 1.0));
    let d = distance(in.uv, vec2<f32>(0.5, 0.58));
    let v = 1.0 - 0.45 * d * d;
    return vec4<f32>(c * v, 1.0);
}

// ---------------------------------------------------------------- 2D layers

struct V2In {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) depth: f32,
};

struct V2Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_2d(v: V2In) -> V2Out {
    var o: V2Out;
    o.pos = vec4<f32>(v.pos.x * u.view2d.x + u.view2d.z, v.pos.y * u.view2d.y + u.view2d.w, v.depth, 1.0);
    o.color = v.color;
    return o;
}

@fragment
fn fs_2d(in: V2Out) -> @location(0) vec4<f32> {
    return in.color;
}

// ---------------------------------------------------------------- shading

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(1.0) - exp(-c * 1.15);
}

fn shade(albedo: vec3<f32>, n0: vec3<f32>, world: vec3<f32>, gloss: f32) -> vec3<f32> {
    let n = normalize(n0);
    let v = normalize(u.eye.xyz - world);
    let l1 = normalize(vec3<f32>(-0.45, -0.6, 0.75));
    let l2 = normalize(vec3<f32>(0.6, 0.35, 0.45));
    let l3 = normalize(vec3<f32>(0.15, 0.85, -0.35));
    let hemi = mix(vec3<f32>(0.10, 0.10, 0.11), vec3<f32>(0.40, 0.43, 0.50), n.z * 0.5 + 0.5);
    let d = max(dot(n, l1), 0.0) * vec3<f32>(1.0, 0.96, 0.90) * 0.95
          + max(dot(n, l2), 0.0) * vec3<f32>(0.70, 0.80, 1.0) * 0.35
          + max(dot(n, l3), 0.0) * vec3<f32>(0.9, 0.9, 1.0) * 0.18;
    let h = normalize(l1 + v);
    let spec = pow(max(dot(n, h), 0.0), mix(18.0, 110.0, gloss)) * mix(0.05, 0.65, gloss);
    let h2 = normalize(l2 + v);
    let spec2 = pow(max(dot(n, h2), 0.0), mix(12.0, 60.0, gloss)) * mix(0.02, 0.2, gloss);
    let fres = pow(1.0 - max(dot(n, v), 0.0), 4.0) * 0.10;
    return albedo * (hemi * 0.8 + d) + vec3<f32>(spec + spec2 + fres);
}

// ---------------------------------------------------------------- part meshes

struct MIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

struct MOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

@vertex
fn vs_mesh(v: MIn) -> MOut {
    var o: MOut;
    o.pos = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.color = v.color;
    return o;
}

@fragment
fn fs_mesh(in: MOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    // Many vendor meshes have mixed winding: light the side we see.
    if (dot(n, u.eye.xyz - in.world) < 0.0) {
        n = -n;
    }
    let lum = dot(in.color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let sat = max(max(in.color.r, in.color.g), in.color.b) - min(min(in.color.r, in.color.g), in.color.b);
    // Bright neutral colours are usually metal.
    let gloss = clamp(smoothstep(0.25, 0.7, lum) * (1.0 - sat * 2.0), 0.08, 0.9);
    return vec4<f32>(tonemap(shade(in.color, n, in.world, gloss)), 1.0);
}

// ---------------------------------------------------------------- board

struct BIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) face: f32,
};

struct BOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) face: f32,
};

@group(1) @binding(0) var t_top: texture_2d<f32>;
@group(1) @binding(1) var t_bot: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

@vertex
fn vs_board(v: BIn) -> BOut {
    var o: BOut;
    o.pos = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.uv = v.uv;
    o.face = v.face;
    return o;
}

@fragment
fn fs_board(in: BOut) -> @location(0) vec4<f32> {
    let top = textureSample(t_top, samp, in.uv);
    let bot = textureSample(t_bot, samp, in.uv);
    var albedo = vec3<f32>(0.42, 0.36, 0.22);
    var gloss = 0.15;
    if (in.face < 0.5) {
        albedo = top.rgb;
        gloss = 0.45;
    } else if (in.face < 1.5) {
        albedo = bot.rgb;
        gloss = 0.45;
    } else {
        // Layered laminate edge.
        let band = fract(in.world.z * 3.0);
        albedo = mix(vec3<f32>(0.38, 0.32, 0.19), vec3<f32>(0.46, 0.40, 0.25), smoothstep(0.3, 0.7, band));
    }
    // Exposed metal (bright, neutral) gets a stronger highlight.
    let lum = dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
    if (in.face < 1.5 && lum > 0.45) {
        gloss = 0.85;
    }
    return vec4<f32>(tonemap(shade(albedo, in.normal, in.world, gloss)), 1.0);
}
