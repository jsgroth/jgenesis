struct PrescaleFactor {
    value: u32,
    // Uniform structs must be padded to a 16 byte boundary for WebGL
    _padding0: u32,
    _padding1: u32,
    _padding2: u32,
}

@group(0) @binding(0)
var texture_in: texture_2d<f32>;
@group(0) @binding(1)
var<uniform> prescale_factor: PrescaleFactor;

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let top_left = vec2u(u32(round(position.x - 0.5)), u32(round(position.y - 0.5)));
    let input_position = top_left / prescale_factor.value;
    return textureLoad(texture_in, input_position, 0);
}