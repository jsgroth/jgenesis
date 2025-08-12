// WebGL requires uniforms to be padded to a multiple of 16 bytes
struct PaddedF32 {
    value: f32,
    _padding0: f32,
    _padding1: f32,
    _padding2: f32,
}

@group(0) @binding(0)
var texture_in: texture_2d<f32>;
@group(0) @binding(1)
var<uniform> gamma: PaddedF32;

fn to_texture_position(fragment_position: vec4f) -> vec2u {
    let texture_position = round(fragment_position.xy - vec2f(0.5));
    return vec2u(u32(texture_position.x), u32(texture_position.y));
}

fn color_correction(position: vec4f, correction: mat3x3f) -> vec4f {
    let tex_position = to_texture_position(position);

    var color = textureLoad(texture_in, tex_position, 0).rgb;

    // Gamma correct for device's screen before applying color correction matrix
    color = pow(color, vec3f(gamma.value / 2.2));

    color = color * correction;

    return vec4f(color, 1.0);
}

// Based on this public domain shader:
// https://github.com/libretro/common-shaders/blob/master/handheld/shaders/color/gbc-color.cg
const GBC_CORRECTION: mat3x3f = mat3x3(
    0.78824, 0.12157, 0.0,
    0.025,   0.72941, 0.275,
    0.12039, 0.12157, 0.82,
);


// Based on this public domain shader:
// https://github.com/libretro/common-shaders/blob/master/handheld/shaders/color/gba-color.cg
const GBA_CORRECTION: mat3x3f = mat3x3(
    0.845, 0.17,  0.015,
    0.09,  0.68,  0.23,
    0.16,  0.085, 0.755,
);

@fragment
fn gbc_color_correction(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return color_correction(position, GBC_CORRECTION);
}

@fragment
fn gba_color_correction(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return color_correction(position, GBA_CORRECTION);
}
