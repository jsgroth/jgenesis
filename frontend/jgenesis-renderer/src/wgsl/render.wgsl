override encode_to_srgb: bool = false;

struct VertexInput {
    @location(0) position: vec2f,
    @location(1) texture_coords: vec2f,
}

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(perspective, sample) texture_coords: vec2f,
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
    var pixel = textureSample(texture_in, sampler_in, input.texture_coords);

    if encode_to_srgb {
        // https://en.wikipedia.org/wiki/SRGB#Transfer_function_(%22gamma%22)
        let srgb = select(
            1.055 * pow(pixel.rgb, vec3f(1.0 / 2.4)) - 0.055,
            12.92 * pixel.rgb,
            pixel.rgb <= vec3f(0.0031308),
        );
        pixel = vec4f(srgb, pixel.a);
    }

    return pixel;
}