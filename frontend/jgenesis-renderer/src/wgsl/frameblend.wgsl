@group(0) @binding(0)
var texture_0: texture_2d<f32>;
@group(0) @binding(1)
var texture_1: texture_2d<f32>;

fn to_texture_position(fragment_position: vec4f) -> vec2u {
    let texture_position = round(fragment_position.xy - vec2f(0.5));
    return vec2u(u32(texture_position.x), u32(texture_position.y));
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let tex_position = to_texture_position(position);

    let a = textureLoad(texture_0, tex_position, 0).rgb;
    let b = textureLoad(texture_1, tex_position, 0).rgb;

    let blended = 0.5 * (a + b);

    return vec4f(blended, 1.0);
}