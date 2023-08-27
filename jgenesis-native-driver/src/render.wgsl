struct VertexInput {
    @location(0) position: vec2f,
    @location(1) texture_coords: vec2f,
}

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) texture_coords: vec2f,
}

@group(0) @binding(0)
var texture_in: texture_2d<f32>;
@group(0) @binding(1)
var sampler_in: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    out.position = vec4f(input.position, 0.0, 1.0);
    out.texture_coords = input.texture_coords;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    return textureSample(texture_in, sampler_in, input.texture_coords);
}