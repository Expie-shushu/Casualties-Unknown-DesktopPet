struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    transform: mat4x4<f32>,
    color: vec4<f32>,
    uvRect: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var t_diffuse: texture_2d<f32>;
@group(0) @binding(2) var s_diffuse: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = u.transform * vec4<f32>(in.position, 0.0, 1.0);
    out.uv = mix(u.uvRect.xy, u.uvRect.zw, in.uv);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(t_diffuse, s_diffuse, in.uv);
    let color = s * u.color;
    return vec4<f32>(color.rgb * color.a, color.a);
}