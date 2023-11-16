struct TextureWidth {
    value: u32,
    // Uniform values must be padded to a multiple of 16 bytes for WebGL
    _padding0: u32,
    _padding1: u32,
    _padding2: u32,
}

@group(0) @binding(0)
var texture_in: texture_2d<f32>;
@group(0) @binding(1)
var<uniform> texture_width: TextureWidth;

fn to_texture_position(fragment_position: vec4f) -> vec2u {
    let texture_position = round(fragment_position.xy - vec2f(0.5));
    return vec2u(u32(texture_position.x), u32(texture_position.y));
}

@fragment
fn hblur_2px(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);

    let left = textureLoad(texture_in, t_position, 0).rgb;
    let right = select(
        left,
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        t_position.x != texture_width.value - 1u,
    );

    let color = (left + right) / 2.0;
    return vec4f(color, 1.0);
}

@fragment
fn hblur_3px(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);
    let color = compute_hblur_3px(t_position);
    return vec4f(color, 1.0);
}

fn compute_hblur_3px(t_position: vec2u) -> vec3f {
    let center = textureLoad(texture_in, t_position, 0).rgb;
    let left = select(
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        textureLoad(texture_in, t_position - vec2u(1u, 0u), 0).rgb,
        t_position.x != 0u,
    );
    let right = select(
        textureLoad(texture_in, t_position - vec2u(1u, 0u), 0).rgb,
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        t_position.x != texture_width.value - 1u,
    );

    return (2.0 * center + left + right) / 4.0;
}

@fragment
fn hblur_snes(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);

    let color = select(
        hblur_snes_256px(t_position),
        compute_hblur_3px(t_position),
        texture_width.value == 512u,
    );

    return vec4f(color, 1.0);
}

fn hblur_snes_256px(out_t_position: vec2u) -> vec3f {
    let in_t_position = out_t_position / vec2u(2u, 1u);

    let center = textureLoad(texture_in, in_t_position, 0).rgb;

    let left_x = select(
        in_t_position.x,
        in_t_position.x - 1u,
        out_t_position.x % 2u == 0u && in_t_position.x != 0u,
    );

    let right_x = select(
        in_t_position.x,
        in_t_position.x + 1u,
        out_t_position.x % 2u == 1u && in_t_position.x != texture_width.value - 1u,
    );

    let left = textureLoad(texture_in, vec2u(left_x, in_t_position.y), 0).rgb;
    let right = textureLoad(texture_in, vec2u(right_x, in_t_position.y), 0).rgb;

    return (2.0 * center + left + right) / 4.0;
}

// Returns a value in the range [0.0, 3.0]
fn diff(a: vec3f, b: vec3f) -> f32 {
    return dot(abs(a - b), vec3f(1.0));
}

fn shift_left(position: vec2u, shift: u32) -> vec2u {
    return select(
        position + vec2u(shift, 0u),
        position - vec2u(shift, 0u),
        position.x > shift - 1u,
    );
}

fn shift_right(position: vec2u, shift: u32) -> vec2u {
    return select(
        position - vec2u(shift, 0u),
        position + vec2u(shift, 0u),
        position.x < texture_width.value - shift,
    );
}

fn should_apply_strong_anti_dither(left2: vec3f, left: vec3f, center: vec3f, right: vec3f, right2: vec3f) -> bool {
    return diff(left, right) < 0.001 && diff(left, center) < 2.5 && diff(left2, left) >= 0.001 && diff(right2, right) >= 0.001;
}

@fragment
fn anti_dither_weak(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);

    let center = textureLoad(texture_in, t_position, 0).rgb;

    let left = textureLoad(texture_in, shift_left(t_position, 1u), 0).rgb;
    let right = textureLoad(texture_in, shift_right(t_position, 1u), 0).rgb;

    let left2 = textureLoad(texture_in, shift_left(t_position, 2u), 0).rgb;
    let right2 = textureLoad(texture_in, shift_right(t_position, 2u), 0).rgb;

    let color = select(
        center,
        (2.0 * center + left + right) / 4.0,
        diff(left, right) < 0.001
            && diff(center, left) < 2.0
            && diff(left2, left) >= 0.001
            && diff(right2, right) >= 0.001,
    );
    return vec4f(color, 1.0);
}

@fragment
fn anti_dither_strong(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);

    let center = textureLoad(texture_in, t_position, 0).rgb;

    let left = textureLoad(texture_in, shift_left(t_position, 1u), 0).rgb;
    let right = textureLoad(texture_in, shift_right(t_position, 1u), 0).rgb;

    let left2 = textureLoad(texture_in, shift_left(t_position, 2u), 0).rgb;
    let right2 = textureLoad(texture_in, shift_right(t_position, 2u), 0).rgb;

    let left3 = textureLoad(texture_in, shift_left(t_position, 3u), 0).rgb;
    let right3 = textureLoad(texture_in, shift_right(t_position, 3u), 0).rgb;

    let color = select(
        select(
            select(
                center,
                (2.0 * left + center + left2) / 4.0,
                should_apply_strong_anti_dither(left3, left2, left, center, right),
            ),
            (2.0 * right + center + right2) / 4.0,
            should_apply_strong_anti_dither(left, center, right, right2, right3),
        ),
        (2.0 * center + left + right) / 4.0,
        should_apply_strong_anti_dither(left2, left, center, right, right2),
    );
    return vec4f(color, 1.0);
}