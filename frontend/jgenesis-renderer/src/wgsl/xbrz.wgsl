// ****************************************************************************
// * This file is part of the xBRZ project. It is distributed under           *
// * GNU General Public License: https://www.gnu.org/licenses/gpl-3.0         *
// * Copyright (C) Zenju (zenju AT gmx DOT de) - All Rights Reserved          *
// *                                                                          *
// * Additionally and as a special exception, the author gives permission     *
// * to link the code of this program with the following libraries            *
// * (or with modified versions that use the same licenses), and distribute   *
// * linked combinations including the two: MAME, FreeFileSync, Snes9x, ePSXe *
// *                                                                          *
// * You must obey the GNU General Public License in all respects for all of  *
// * the code used other than MAME, FreeFileSync, Snes9x, ePSXe.              *
// * If you modify this file, you may extend this exception to your version   *
// * of the file, but you are not obligated to do so. If you do not wish to   *
// * do so, delete this exception statement from your version.                *
// ****************************************************************************

// WGSL shader port of xBRZ v1.9
// Copyright (C) 2026 James Groth

// Assumed to be between 2 and 6
override scale_factor: i32;

override equal_color_tolerance: f32 = 30.0 / 255.0;
override center_direction_bias: f32 = 4.0;
override dominant_direction_threshold: f32 = 3.6;
override steep_direction_threshold: f32 = 2.4;

@group(0) @binding(0) var input_frame: texture_2d<f32>;
@group(0) @binding(1) var output_frame: texture_storage_2d<rgba8unorm, write>;

// 5x5 grid of pixels with the pixel at (X, Y) in the center
var<private> input_pixels: array<array<vec3f, 5>, 5>;

// NxN grid of pixels to output starting at (N*X, N*Y)
var<private> output_pixels: array<array<vec3f, 6>, 6>;

const BLEND_NONE: u32 = 0;
const BLEND_NORMAL: u32 = 1;
const BLEND_DOMINANT: u32 = 2;

// Computed blend mode in each diagonal from the center pixel
struct BlendModes {
    tl: u32,
    tr: u32,
    bl: u32,
    br: u32,
}

fn any_blend_mode(blend: BlendModes) -> bool {
    return any(vec4u(blend.tl, blend.tr, blend.bl, blend.br) != vec4u(BLEND_NONE));
}

@compute @workgroup_size(16, 16, 1)
fn xbrz(@builtin(global_invocation_id) invocation: vec3u) {
    let position = vec2i(invocation.xy);
    let input_size = vec2i(textureDimensions(input_frame));
    if any(position >= input_size) {
        return;
    }

    // Load input pixels surrounding current position
    load_input(position, input_size);

    // Initialize output full of center input pixel
    for (var i = 0; i < scale_factor; i++) {
        for (var j = 0; j < scale_factor; j++) {
            output_pixels[i][j] = input_pixels[2][2];
        }
    }

    // Compute blend mode for each diagonal
    var blend_modes = compute_blend_modes();

    if any_blend_mode(blend_modes) {
        // Perform blending in each diagonal direction

        // Bottom-right diagonal
        if blend_modes.br != BLEND_NONE {
            // - - - - -
            // - - B C -
            // - D E F -
            // - G H I -
            // - - - - -
            blend_pixel(
                blend_modes,
                input_pixels[1][2],
                input_pixels[1][3],
                input_pixels[2][1],
                input_pixels[2][2],
                input_pixels[2][3],
                input_pixels[3][1],
                input_pixels[3][2],
                input_pixels[3][3],
            );
        }

        rotate_output();
        blend_modes = rotate_blend_modes(blend_modes);

        // Top-right diagonal
        if blend_modes.br != BLEND_NONE {
            // - - - - -
            // - C F I -
            // - B E H -
            // - - D G -
            // - - - - -
            blend_pixel(
                blend_modes,
                input_pixels[2][1],
                input_pixels[1][1],
                input_pixels[3][2],
                input_pixels[2][2],
                input_pixels[1][2],
                input_pixels[3][3],
                input_pixels[2][3],
                input_pixels[1][3],
            );
        }

        rotate_output();
        blend_modes = rotate_blend_modes(blend_modes);

        // Top-left diagonal
        if blend_modes.br != BLEND_NONE {
            // - - - - -
            // - I H G -
            // - F E D -
            // - C B - -
            // - - - - -
            blend_pixel(
                blend_modes,
                input_pixels[3][2],
                input_pixels[3][1],
                input_pixels[2][3],
                input_pixels[2][2],
                input_pixels[2][1],
                input_pixels[1][3],
                input_pixels[1][2],
                input_pixels[1][1],
            );
        }

        rotate_output();
        blend_modes = rotate_blend_modes(blend_modes);

        // Bottom-left diagonal
        if blend_modes.br != BLEND_NONE {
            // - - - - -
            // - G D - -
            // - H E B -
            // - I F C -
            // - - - - -
            blend_pixel(
                blend_modes,
                input_pixels[2][3],
                input_pixels[3][3],
                input_pixels[1][2],
                input_pixels[2][2],
                input_pixels[3][2],
                input_pixels[1][1],
                input_pixels[2][1],
                input_pixels[3][1],
            );
        }

        rotate_output();
    }

    // Write output pixels
    let out_pos_tl = scale_factor * position;
    for (var y = 0; y < scale_factor; y++) {
        for (var x = 0; x < scale_factor; x++) {
            let output_pixel = output_pixels[y][x];
            textureStore(output_frame, out_pos_tl + vec2i(x, y), vec4f(output_pixel, 1.0));
        }
    }
}

// Load the 5x5 grid of pixels surrounding (X, Y), clamping to edge
fn load_input(position: vec2i, input_size: vec2i) {
    let upper_bound = input_size - 1;

    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            if abs(dx) + abs(dy) == 4 {
                // The 4 corner pixels are not used; don't bother to load
                continue;
            }

            let input_pixel = textureLoad(
                input_frame,
                clamp(position + vec2i(dx, dy), vec2i(0), upper_bound),
                0,
            ).rgb;
            input_pixels[dy + 2][dx + 2] = input_pixel;
        }
    }
}

// Rotate output matrix 90 degrees clockwise
fn rotate_output() {
    var rotated: array<array<vec3f, 6>, 6>;

    for (var i = 0; i < scale_factor; i++) {
        for (var j = 0; j < scale_factor; j++) {
            rotated[i][j] = output_pixels[scale_factor - 1 - j][i];
        }
    }

    output_pixels = rotated;
}

// Rotate blend modes 90 degrees clockwise
fn rotate_blend_modes(blend: BlendModes) -> BlendModes {
    var rotated: BlendModes;
    rotated.tl = blend.bl;
    rotated.tr = blend.tl;
    rotated.br = blend.tr;
    rotated.bl = blend.br;
    return rotated;
}

// https://en.wikipedia.org/wiki/YCbCr#ITU-R_BT.2020_conversion
const K_B: f32 = 0.0593;
const K_R: f32 = 0.2627;
const K_G: f32 = 1.0 - K_B - K_R;
const RGB_TO_YCBCR: mat3x3f = mat3x3f(
    vec3f(K_R, -0.5 * K_R / (1.0 - K_B),           0.5           ),
    vec3f(K_G, -0.5 * K_G / (1.0 - K_B), -0.5 * K_G / (1.0 - K_R)),
    vec3f(K_B,           0.5           , -0.5 * K_B / (1.0 - K_R)),
);

fn color_distance(c0: vec3f, c1: vec3f) -> f32 {
    let diff_ycbcr = RGB_TO_YCBCR * (c0 - c1);
    return sqrt(dot(diff_ycbcr, diff_ycbcr));
}

fn equal_within_tolerance(c0: vec3f, c1: vec3f) -> bool {
    return color_distance(c0, c1) < equal_color_tolerance;
}

fn compute_blend_modes() -> BlendModes {
    var blend: BlendModes;
    blend.tl = compute_blend_mode(0, 0);
    blend.tr = compute_blend_mode(0, 1);
    blend.bl = compute_blend_mode(1, 0);
    blend.br = compute_blend_mode(1, 1);
    return blend;
}

// Compute the blend mode for a single diagonal
fn compute_blend_mode(row: i32, col: i32) -> u32 {
    // - B C -
    // D E F O
    // G H I N
    // - K L -

    let e = input_pixels[row + 1][col + 1];
    let f = input_pixels[row + 1][col + 2];
    let h = input_pixels[row + 2][col + 1];
    let i = input_pixels[row + 2][col + 2];

    if all((e == f) & (h == i)) || all((e == h) & (f == i)) {
        return BLEND_NONE;
    }

    let b = input_pixels[row + 0][col + 1];
    let c = input_pixels[row + 0][col + 2];
    let d = input_pixels[row + 1][col + 0];
    let g = input_pixels[row + 2][col + 0];
    let k = input_pixels[row + 3][col + 1];
    let l = input_pixels[row + 3][col + 2];
    let n = input_pixels[row + 2][col + 3];
    let o = input_pixels[row + 1][col + 3];

    let hf =
          color_distance(g, e)
        + color_distance(e, c)
        + color_distance(k, i)
        + color_distance(i, o)
        + center_direction_bias * color_distance(h, f);
    let ei =
          color_distance(d, h)
        + color_distance(h, l)
        + color_distance(b, f)
        + color_distance(f, n)
        + center_direction_bias * color_distance(e, i);

    switch (2 * row + col) {
        case 0: {
            // (0, 0): Top-left diagonal from I
            if hf < ei && any(i != h) && any(i != f) {
                let dominant = dominant_direction_threshold * hf < ei;
                return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
            }
        }
        case 1: {
            // (0, 1): Top-right diagonal from H
            if ei < hf && any(h != e) && any(h != i) {
                let dominant = dominant_direction_threshold * ei < hf;
                return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
            }
        }
        case 2: {
            // (1, 0): Bottom-left diagonal from F
            if ei < hf && any(f != e) && any(f != i) {
                let dominant = dominant_direction_threshold * ei < hf;
                return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
            }
        }
        case 3: {
            // (1, 1): Bottom-right diagonal from E
            if hf < ei && any(e != f) && any(e != h) {
                let dominant = dominant_direction_threshold * hf < ei;
                return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
            }
        }
        default: {}
    }

    return BLEND_NONE;
}

// Perform blending in the bottom-right diagonal from the center pixel
fn blend_pixel(
    blend: BlendModes,
    b: vec3f,
    c: vec3f,
    d: vec3f,
    e: vec3f,
    f: vec3f,
    g: vec3f,
    h: vec3f,
    i: vec3f,
) {
    // - - - - -
    // - - B C -
    // - D E F -
    // - G H I -
    // - - - - -

    let do_line_blend = (blend.br == BLEND_DOMINANT) || !(
        (blend.tr != BLEND_NONE && !equal_within_tolerance(e, g))
            || (blend.bl != BLEND_NONE && !equal_within_tolerance(e, c))
            || (!equal_within_tolerance(e, i)
                && equal_within_tolerance(g, h)
                && equal_within_tolerance(h, i)
                && equal_within_tolerance(i, f)
                && equal_within_tolerance(f, c))
    );

    let fg = color_distance(f, g);
    let hc = color_distance(h, c);

    let shallow_line = do_line_blend && steep_direction_threshold * fg <= hc && any(e != g) && any(d != g);
    let steep_line   = do_line_blend && steep_direction_threshold * hc <= fg && any(e != c) && any(b != c);

    let blend_color = select(h, f, color_distance(e, f) <= color_distance(e, h)); //choose most similar color

    switch (scale_factor) {
        case 2: {
            scale_pixel_2x(blend_color, do_line_blend, shallow_line, steep_line);
        }
        case 3: {
            scale_pixel_3x(blend_color, do_line_blend, shallow_line, steep_line);
        }
        case 4: {
            scale_pixel_4x(blend_color, do_line_blend, shallow_line, steep_line);
        }
        case 5: {
            scale_pixel_5x(blend_color, do_line_blend, shallow_line, steep_line);
        }
        case 6: {
            scale_pixel_6x(blend_color, do_line_blend, shallow_line, steep_line);
        }
        default: {}
    }
}

// Alpha blend into output pixel at [i][j], in linear color space
fn alpha_blend(i: u32, j: u32, color: vec3f, alpha: f32) {
    let back = srgb_to_linear(output_pixels[i][j]);
    let front = srgb_to_linear(color);
    let blended = (1.0 - alpha) * back + alpha * front;
    output_pixels[i][j] = linear_to_srgb(blended);
}

fn srgb_to_linear(c: vec3f) -> vec3f {
    return pow(c, vec3f(2.2));
}

fn linear_to_srgb(c: vec3f) -> vec3f {
    return pow(c, vec3f(1.0 / 2.2));
}

fn scale_pixel_2x(c: vec3f, do_line_blend: bool, shallow_line: bool, steep_line: bool) {
    alpha_blend(0, 1, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(1, 0, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(1, 1, c, select(
        select(
            select(
                0.2146018366,
                1.0 / 2.0,
                do_line_blend,
            ),
            3.0 / 4.0,
            steep_line || shallow_line,
        ),
        5.0 / 6.0,
        steep_line && shallow_line,
    ));
}

fn scale_pixel_3x(c: vec3f, do_line_blend: bool, shallow_line: bool, steep_line: bool) {
    alpha_blend(0, 2, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(2, 0, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(1, 2, c, select(
        select(
            select(
                0.0,
                1.0 / 8.0,
                do_line_blend,
            ),
            1.0 / 4.0,
            shallow_line,
        ),
        3.0 / 4.0,
        steep_line,
    ));

    alpha_blend(2, 1, c, select(
        select(
            select(
                0.0,
                1.0 / 8.0,
                do_line_blend,
            ),
            1.0 / 4.0,
            steep_line,
        ),
        3.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(2, 2, c, select(
        select(
            0.4545939598,
            7.0 / 8.0,
            do_line_blend,
        ),
        1.0,
        shallow_line || steep_line,
    ));
}

fn scale_pixel_4x(c: vec3f, do_line_blend: bool, shallow_line: bool, steep_line: bool) {
    alpha_blend(3, 0, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(0, 3, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(3, 1, c, select(
        0.0,
        3.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(1, 3, c, select(
        0.0,
        3.0 / 4.0,
        steep_line,
    ));

    alpha_blend(3, 2, c, select(
        select(
            select(
                0.08677704501,
                1.0 / 2.0,
                do_line_blend,
            ),
            3.0 / 4.0,
            steep_line,
        ),
        1.0,
        shallow_line,
    ));

    alpha_blend(2, 3, c, select(
        select(
            select(
                0.08677704501,
                1.0 / 2.0,
                do_line_blend,
            ),
            3.0 / 4.0,
            shallow_line,
        ),
        1.0,
        steep_line,
    ));

    alpha_blend(2, 2, c, select(
        select(
            0.0,
            1.0 / 4.0,
            steep_line || shallow_line,
        ),
        1.0 / 3.0,
        steep_line && shallow_line,
    ));

    alpha_blend(3, 3, c, select(
        0.6848532563,
        1.0,
        do_line_blend,
    ));
}

fn scale_pixel_5x(c: vec3f, do_line_blend: bool, shallow_line: bool, steep_line: bool) {
    alpha_blend(4, 0, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(0, 4, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(3, 2, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(2, 3, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(4, 1, c, select(
        0.0,
        3.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(1, 4, c, select(
        0.0,
        3.0 / 4.0,
        steep_line,
    ));

    alpha_blend(3, 3, c, select(
        select(
            select(
                0.0,
                1.0 / 8.0,
                do_line_blend,
            ),
            3.0 / 4.0,
            steep_line || shallow_line,
        ),
        2.0 / 3.0,
        steep_line && shallow_line,
    ));

    alpha_blend(2, 4, c, select(
        select(
            select(
                0.0,
                1.0 / 8.0,
                do_line_blend,
            ),
            1.0 / 4.0,
            shallow_line,
        ),
        1.0,
        steep_line,
    ));

    alpha_blend(4, 2, c, select(
        select(
            select(
                0.0,
                1.0 / 8.0,
                do_line_blend,
            ),
            1.0 / 4.0,
            steep_line,
        ),
        1.0,
        shallow_line,
    ));

    alpha_blend(4, 3, c, select(
        select(
            0.2306749731,
            7.0 / 8.0,
            do_line_blend,
        ),
        1.0,
        steep_line || shallow_line,
    ));

    alpha_blend(3, 4, c, select(
        select(
            0.2306749731,
            7.0 / 8.0,
            do_line_blend,
        ),
        1.0,
        steep_line || shallow_line,
    ));

    alpha_blend(4, 4, c, select(
        0.8631434088,
        1.0,
        do_line_blend,
    ));
}

fn scale_pixel_6x(c: vec3f, do_line_blend: bool, shallow_line: bool, steep_line: bool) {
    alpha_blend(0, 5, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(5, 0, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(2, 4, c, select(
        0.0,
        1.0 / 4.0,
        steep_line,
    ));

    alpha_blend(4, 2, c, select(
        0.0,
        1.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(1, 5, c, select(
        0.0,
        3.0 / 4.0,
        steep_line,
    ));

    alpha_blend(5, 1, c, select(
        0.0,
        3.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(3, 4, c, select(
        select(
            0.0,
            1.0 / 4.0,
            shallow_line,
        ),
        3.0 / 4.0,
        steep_line,
    ));

    alpha_blend(4, 3, c, select(
        select(
            0.0,
            1.0 / 4.0,
            steep_line,
        ),
        3.0 / 4.0,
        shallow_line,
    ));

    alpha_blend(2, 5, c, select(
        0.0,
        1.0,
        steep_line,
    ));

    alpha_blend(5, 2, c, select(
        0.0,
        1.0,
        shallow_line,
    ));

    alpha_blend(3, 5, c, select(
        select(
            select(
                0.05652034508,
                1.0 / 2.0,
                do_line_blend,
            ),
            3.0 / 4.0,
            shallow_line,
        ),
        1.0,
        steep_line,
    ));

    alpha_blend(5, 3, c, select(
        select(
            select(
                0.05652034508,
                1.0 / 2.0,
                do_line_blend,
            ),
            3.0 / 4.0,
            steep_line,
        ),
        1.0,
        shallow_line,
    ));

    alpha_blend(4, 5, c, select(
        0.4236372243,
        1.0,
        do_line_blend,
    ));

    alpha_blend(5, 4, c, select(
        0.4236372243,
        1.0,
        do_line_blend,
    ));

    alpha_blend(4, 4, c, select(
        select(
            0.0,
            1.0 / 2.0,
            do_line_blend,
        ),
        1.0,
        steep_line || shallow_line,
    ));

    alpha_blend(5, 5, c, select(
        0.9711013910,
        1.0,
        do_line_blend,
    ));
}
