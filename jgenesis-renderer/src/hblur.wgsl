@group(0) @binding(0)
var texture_in: texture_2d<f32>;
@group(0) @binding(1)
var<uniform> texture_width: u32;

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
        t_position.x != texture_width - 1u,
    );

    let color = (left + right) / 2.0;
    return vec4f(color, 1.0);
}

@fragment
fn hblur_3px(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);

    let center = textureLoad(texture_in, t_position, 0).rgb;
    let left = select(
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        textureLoad(texture_in, t_position - vec2u(1u, 0u), 0).rgb,
        t_position.x != 0u,
    );
    let right = select(
        textureLoad(texture_in, t_position - vec2u(1u, 0u), 0).rgb,
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        t_position.x != texture_width - 1u,
    );

    let color = (2.0 * center + left + right) / 4.0;
    return vec4f(color, 1.0);
}

// Returns a value in the range [0.0, 3.0]
fn diff(a: vec3f, b: vec3f) -> f32 {
    return dot(abs(a - b), vec3f(1.0));
}

@fragment
fn anti_dither(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let t_position = to_texture_position(position);

    let center = textureLoad(texture_in, t_position, 0).rgb;

    let left = select(
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        textureLoad(texture_in, t_position - vec2u(1u, 0u), 0).rgb,
        t_position.x != 0u,
    );
    let right = select(
        textureLoad(texture_in, t_position - vec2u(1u, 0u), 0).rgb,
        textureLoad(texture_in, t_position + vec2u(1u, 0u), 0).rgb,
        t_position.x != texture_width - 1u,
    );

    let left2 = select(
        textureLoad(texture_in, t_position + vec2u(2u, 0u), 0).rgb,
        textureLoad(texture_in, t_position - vec2u(2u, 0u), 0).rgb,
        t_position.x > 1u,
    );
    let right2 = select(
        textureLoad(texture_in, t_position - vec2u(2u, 0u), 0).rgb,
        textureLoad(texture_in, t_position + vec2u(2u, 0u), 0).rgb,
        t_position.x < texture_width - 2u,
    );

    let color = select(
        center,
        (2.0 * center + left + right) / 4.0,
        diff(left, right) < 0.001 && (diff(left2, left) >= 0.001 || diff(right2, right) >= 0.001),
    );
    return vec4f(color, 1.0);
}