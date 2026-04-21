struct PrescaleParams {
    width_scale: u32,
    height_scale: u32,
    original_height: u32,
    output_height: u32,
    scanline_multiplier: f32,
}

@group(0) @binding(0) var texture_in: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: PrescaleParams;

@fragment
fn basic_prescale(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let top_left = vec2u(position.xy);
    let input_position = top_left / vec2u(params.width_scale, params.height_scale);
    return textureLoad(texture_in, input_position, 0);
}

@fragment
fn scanlines(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let top_left = vec2u(position.xy);
    let input_position = top_left / vec2u(params.width_scale, params.height_scale);

    let texture_color = textureLoad(texture_in, input_position, 0).rgb;

    let crt_line = top_left.y * 2 * params.original_height / params.output_height;
    let color = select(texture_color, texture_color * params.scanline_multiplier, (crt_line % 2) == 1);
    return vec4f(color, 1.0);
}
