struct PrescaleFactor {
    width: u32,
    height: u32,
    // Uniform structs must be padded to a 16 byte boundary for WebGL
    _padding1: u32,
    _padding2: u32,
}

@group(0) @binding(0)
var texture_in: texture_2d<f32>;
@group(0) @binding(1)
var<uniform> prescale_factor: PrescaleFactor;

@fragment
fn basic_prescale(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let top_left = vec2u(u32(round(position.x - 0.5)), u32(round(position.y - 0.5)));
    let input_position = top_left / vec2u(prescale_factor.width, prescale_factor.height);
    return textureLoad(texture_in, input_position, 0);
}

fn scanlines_fs(position: vec4f, scanline_multiplier: f32) -> vec4f {
    let top_left = vec2u(u32(round(position.x - 0.5)), u32(round(position.y - 0.5)));
    let input_position = top_left / vec2u(prescale_factor.width, prescale_factor.height);

    let texture_color = textureLoad(texture_in, input_position, 0).rgb;

    let crt_line = u32(round(position.y - 0.5)) / (prescale_factor.height / 2u);
    let color = select(texture_color, texture_color * scanline_multiplier, crt_line % 2u == 1u);
    return vec4f(color, 1.0);
}

@fragment
fn dim_scanlines(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return scanlines_fs(position, 0.5);
}

@fragment
fn black_scanlines(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return scanlines_fs(position, 0.0);
}