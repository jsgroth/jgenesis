// Used by all (assumed to be 15 or 12)
override samples_per_color_cycle: i32;

// Used by all (assumed to be between 1 and 84, inclusive)
override fir_len: i32;

// Used by rgb_to_ntsc and luma_chroma_to_rgb (assumed to be at least 1)
override upscale_factor: i32;

// Used by rgb_to_ntsc
@group(0) @binding(0) var<uniform> y_encode_lpf_coefficients: array<vec4f, 21>;
@group(0) @binding(1) var<uniform> iq_encode_lpf_coefficients: array<vec4f, 21>;
@group(0) @binding(2) var input_frame: texture_2d<f32>;
@group(0) @binding(3) var ntsc_frame_w: texture_storage_2d<r32float, write>;

// Used by separate_luma_chroma
@group(0) @binding(4) var<uniform> luma_bsf_coefficients: array<vec4f, 21>;
@group(0) @binding(5) var<uniform> chroma_bpf_coefficients: array<vec4f, 21>;
@group(0) @binding(6) var ntsc_frame_r: texture_2d<f32>;
@group(0) @binding(7) var ntsc_luma_w: texture_storage_2d<r32float, write>;
@group(0) @binding(8) var ntsc_chroma_w: texture_storage_2d<r32float, write>;

// Used by luma_chroma_to_rgb
@group(0) @binding(9) var<uniform> y_decode_lpf_coefficients: array<vec4f, 21>;
@group(0) @binding(10) var<uniform> iq_decode_lpf_coefficients: array<vec4f, 21>;
@group(0) @binding(11) var ntsc_luma_r: texture_2d<f32>;
@group(0) @binding(12) var ntsc_chroma_r: texture_2d<f32>;
@group(0) @binding(13) var output_frame: texture_storage_2d<rgba8unorm, write>;

// https://en.wikipedia.org/wiki/YIQ#From_RGB_to_YIQ
const RGB_TO_YIQ: mat3x3f = mat3x3f(
    vec3f(0.299, 0.5959, 0.2115),
    vec3f(0.587, -0.2746, -0.5227),
    vec3f(0.114, -0.3213, 0.3112),
);

// https://en.wikipedia.org/wiki/YIQ#From_YIQ_to_RGB
const YIQ_TO_RGB: mat3x3f = mat3x3f(
    vec3f(1.0, 1.0, 1.0),
    vec3f(0.956, -0.272, -1.106),
    vec3f(0.619, -0.647, 1.703),
);

const PI: f32 = radians(180.0);

// Extra pixels to render at the horizontal edges, to avoid the NTSC signal sharply cutting off at the borders
const BACKDROP_PIXELS: i32 = 6;

// Convert from RGB to YIQ, apply LPF to each YIQ component, encode from YIQ to NTSC
// LPF to Y instead of BSF because I think it looks slightly better, and it's going to get LPFed during decoding anyway
@compute @workgroup_size(16, 16, 1)
fn rgb_to_ntsc(@builtin(global_invocation_id) invocation: vec3u) {
    let ntsc_size = vec2i(textureDimensions(ntsc_frame_w));
    let position = vec2i(invocation.xy);
    if position.x >= ntsc_size.x || position.y >= ntsc_size.y {
        return;
    }

    let input_size = vec2i(textureDimensions(input_frame));

    let start_x = position.x + fir_len / 2 - upscale_factor * BACKDROP_PIXELS;
    let input_divisor = vec2i(upscale_factor, 1);

    var filtered_y = vec4f(0.0);
    var filtered_i = vec4f(0.0);
    var filtered_q = vec4f(0.0);
    for (var i = 0; i < fir_len; i += 4) {
        let rgb_pixels = array(
            load_input_bounds_checked(vec2i(start_x - i, position.y) / input_divisor, input_size),
            load_input_bounds_checked(vec2i(start_x - i - 1, position.y) / input_divisor, input_size),
            load_input_bounds_checked(vec2i(start_x - i - 2, position.y) / input_divisor, input_size),
            load_input_bounds_checked(vec2i(start_x - i - 3, position.y) / input_divisor, input_size),
        );

        let yiq_pixels = array(
            RGB_TO_YIQ * rgb_pixels[0],
            RGB_TO_YIQ * rgb_pixels[1],
            RGB_TO_YIQ * rgb_pixels[2],
            RGB_TO_YIQ * rgb_pixels[3],
        );

        let y_coefficients = y_encode_lpf_coefficients[i / 4];
        let iq_coefficients = iq_encode_lpf_coefficients[i / 4];

        filtered_y = fma(
            y_coefficients,
            vec4f(yiq_pixels[0].r, yiq_pixels[1].r, yiq_pixels[2].r, yiq_pixels[3].r),
            filtered_y,
        );
        filtered_i = fma(
            iq_coefficients,
            vec4f(yiq_pixels[0].g, yiq_pixels[1].g, yiq_pixels[2].g, yiq_pixels[3].g),
            filtered_i,
        );
        filtered_q = fma(
            iq_coefficients,
            vec4f(yiq_pixels[0].b, yiq_pixels[1].b, yiq_pixels[2].b, yiq_pixels[3].b),
            filtered_q,
        );
    }

    let yiq = vec3f(
        dot(filtered_y, vec4f(1.0)),
        dot(filtered_i, vec4f(1.0)),
        dot(filtered_q, vec4f(1.0)),
    );

    let phase = f32(position.x) / f32(samples_per_color_cycle) * 2.0 * PI;
    let ntsc = yiq.r + yiq.g * sin(phase) + yiq.b * cos(phase);
    textureStore(ntsc_frame_w, position, vec4f(ntsc, vec3f(0.0)));
}

fn load_input_bounds_checked(position: vec2i, input_size: vec2i) -> vec3f {
    if position.x < 0 || position.y < 0 || position.x >= input_size.x || position.y >= input_size.y {
        // WGSL spec says implementations may return any texel within the texture if coordinates are out of bounds;
        // guarantee that a black pixel gets loaded instead
        return vec3f(0.0);
    }

    return textureLoad(input_frame, position, 0).rgb;
}

// Apply BPF and BSF to NTSC signal
@compute @workgroup_size(16, 16, 1)
fn separate_luma_chroma(@builtin(global_invocation_id) invocation: vec3u) {
    let frame_size = vec2i(textureDimensions(ntsc_frame_r));
    let position = vec2i(invocation.xy);
    if position.x >= frame_size.x || position.y >= frame_size.y {
        return;
    }

    let start_x = position.x + fir_len / 2;

    var pass_filtered = vec4f(0.0);
    var stop_filtered = vec4f(0.0);
    for (var i = 0; i < fir_len; i += 4) {
        let ntsc_samples = vec4f(
            textureLoad(ntsc_frame_r, vec2i(start_x - i, position.y), 0).r,
            textureLoad(ntsc_frame_r, vec2i(start_x - i - 1, position.y), 0).r,
            textureLoad(ntsc_frame_r, vec2i(start_x - i - 2, position.y), 0).r,
            textureLoad(ntsc_frame_r, vec2i(start_x - i - 3, position.y), 0).r,
        );

        pass_filtered = fma(ntsc_samples, chroma_bpf_coefficients[i / 4], pass_filtered);
        stop_filtered = fma(ntsc_samples, luma_bsf_coefficients[i / 4], stop_filtered);
    }

    let pass_sample = dot(pass_filtered, vec4f(1.0));
    let stop_sample = dot(stop_filtered, vec4f(1.0));

    textureStore(ntsc_chroma_w, position, vec4f(pass_sample, vec3f(0.0)));
    textureStore(ntsc_luma_w, position, vec4f(stop_sample, vec3f(0.0)));
}

// Decode I and Q from chroma, apply LPF to each YIQ component, convert from YIQ to RGB
@compute @workgroup_size(16, 16, 1)
fn luma_chroma_to_rgb(@builtin(global_invocation_id) invocation: vec3u) {
    let output_size = vec2i(textureDimensions(output_frame));
    let position = vec2i(invocation.xy);
    if position.x >= output_size.x || position.y >= output_size.y {
        return;
    }

    let start_x = position.x + fir_len / 2 + upscale_factor * BACKDROP_PIXELS;

    var filtered_y = vec4f(0.0);
    var filtered_i = vec4f(0.0);
    var filtered_q = vec4f(0.0);
    for (var i = 0; i < fir_len; i += 4) {
        let luma_samples = vec4f(
            textureLoad(ntsc_luma_r, vec2i(start_x - i, position.y), 0).r,
            textureLoad(ntsc_luma_r, vec2i(start_x - i - 1, position.y), 0).r,
            textureLoad(ntsc_luma_r, vec2i(start_x - i - 2, position.y), 0).r,
            textureLoad(ntsc_luma_r, vec2i(start_x - i - 3, position.y), 0).r,
        );

        let chroma_samples = vec4f(
             textureLoad(ntsc_chroma_r, vec2i(start_x - i, position.y), 0).r,
             textureLoad(ntsc_chroma_r, vec2i(start_x - i - 1, position.y), 0).r,
             textureLoad(ntsc_chroma_r, vec2i(start_x - i - 2, position.y), 0).r,
             textureLoad(ntsc_chroma_r, vec2i(start_x - i - 3, position.y), 0).r,
        );

        let y_coefficients = y_decode_lpf_coefficients[i / 4];
        let iq_coefficients = iq_decode_lpf_coefficients[i / 4];

        let phases = vec4f(vec4i(
            start_x - i,
            start_x - i - 1,
            start_x - i - 2,
            start_x - i - 3,
        )) / f32(samples_per_color_cycle) * 2.0 * PI;

        filtered_y = fma(y_coefficients, luma_samples, filtered_y);
        filtered_i = fma(iq_coefficients, chroma_samples * sin(phases) * 2.0, filtered_i);
        filtered_q = fma(iq_coefficients, chroma_samples * cos(phases) * 2.0, filtered_q);
    }

    let yiq = vec3f(
        dot(filtered_y, vec4f(1.0)),
        dot(filtered_i, vec4f(1.0)),
        dot(filtered_q, vec4f(1.0)),
    );
    let rgb = YIQ_TO_RGB * yiq;
    textureStore(output_frame, position, vec4f(rgb, 1.0));
}