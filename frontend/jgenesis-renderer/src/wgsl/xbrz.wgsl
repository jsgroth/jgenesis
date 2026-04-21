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

// Aassumed to be between 2 and 6
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
    return blend.tl != BLEND_NONE || blend.tr != BLEND_NONE || blend.bl != BLEND_NONE || blend.br != BLEND_NONE;
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
        for (var orientation = 0; orientation < 4; orientation++) {
            blend_pixel(blend_modes);

            rotate_input();
            rotate_output();
            blend_modes = rotate_blend_modes(blend_modes);
        }
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

// Rotate input matrix 90 degrees clockwise
fn rotate_input() {
    var rotated: array<array<vec3f, 5>, 5>;

    for (var i = 0; i < 5; i++) {
        for (var j = 0; j < 5; j++) {
            rotated[i][j] = input_pixels[4 - j][i];
        }
    }

    input_pixels = rotated;
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

    if hf < ei {
        let dominant = dominant_direction_threshold * hf < ei;

        // (1, 1): Bottom-right diagonal from E
        if row == 1 && col == 1 && any(e != f) && any(e != h) {
            return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
        }

        // (0, 0): Top-left diagonal from I
        if row == 0 && col == 0 && any(i != h) && any(i != f) {
            return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
        }
    } else if ei < hf {
        let dominant = dominant_direction_threshold * ei < hf;

        // (0, 1): Top-right diagonal from H
        if row == 0 && col == 1 && any(h != e) && any(h != i) {
            return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
        }

        // (1, 0): Bottom-left diagonal from F
        if row == 1 && col == 0 && any(f != e) && any(f != i) {
            return select(BLEND_NORMAL, BLEND_DOMINANT, dominant);
        }
    }

    return BLEND_NONE;
}

// Perform blending in the bottom-right diagonal from the center pixel
fn blend_pixel(blend: BlendModes) {
    if blend.br == BLEND_NONE {
        return;
    }

    // - - - - -
    // - - B C -
    // - D E F -
    // - G H I -
    // - - - - -

    let b = input_pixels[1][2];
    let c = input_pixels[1][3];
    let d = input_pixels[2][1];
    let e = input_pixels[2][2];
    let f = input_pixels[2][3];
    let g = input_pixels[3][1];
    let h = input_pixels[3][2];
    let i = input_pixels[3][3];

    var do_line_blend = true;
    if blend.br != BLEND_DOMINANT {
        //make sure there is no second blending in an adjacent rotation for this pixel: handles insular pixels, mario eyes
        if blend.tr != BLEND_NONE && !equal_within_tolerance(e, g) { //but support double-blending for 90° corners
            do_line_blend = false;
        }
        if blend.bl != BLEND_NONE && !equal_within_tolerance(e, c) {
            do_line_blend = false;
        }

        //no full blending for L-shapes; blend corner only (handles "mario mushroom eyes")
        if !equal_within_tolerance(e, i)
            && equal_within_tolerance(g, h)
            && equal_within_tolerance(h, i)
            && equal_within_tolerance(i, f)
            && equal_within_tolerance(f, c)
        {
            do_line_blend = false;
        }
    }

    let blend_color = select(h, f, color_distance(e, f) <= color_distance(e, h)); //choose most similar color

    if do_line_blend {
        let fg = color_distance(f, g);
        let hc = color_distance(h, c);

        let shallow_line = steep_direction_threshold * fg <= hc && any(e != g) && any(d != g);
        let steep_line   = steep_direction_threshold * hc <= fg && any(e != c) && any(b != c);

        if shallow_line && steep_line {
            blend_steep_and_shallow_line(blend_color);
        } else if shallow_line {
            blend_shallow_line(blend_color);
        } else if steep_line {
            blend_steep_line(blend_color);
        } else {
            blend_diagonal(blend_color);
        }
    } else {
        blend_corner(blend_color);
    }
}

// Alpha blend into output pixel at [i][j], in linear color space
fn alpha_blend(i: u32, j: u32, color: vec3f, alpha: f32) {
    let back = srgb_to_linear(output_pixels[i][j]);
    let front = srgb_to_linear(color);
    let blended = (1.0 - alpha) * back + alpha * front;
    output_pixels[i][j] = linear_to_srgb(blended);
}

// https://en.wikipedia.org/wiki/SRGB#Transfer_function_(%22gamma%22)
fn srgb_to_linear(c: vec3f) -> vec3f {
    return select(
        pow((c + 0.055) / 1.055, vec3f(2.4)),
        c / 12.92,
        c <= vec3f(0.04045),
    );
}

fn linear_to_srgb(c: vec3f) -> vec3f {
    return select(
        1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055,
        c * 12.92,
        c <= vec3f(0.0031308),
    );
}

fn blend_shallow_line(c: vec3f) {
    switch (scale_factor) {
        case 2: {
            alpha_blend(1, 0, c, 1.0 / 4.0);
            alpha_blend(1, 1, c, 3.0 / 4.0);
        }
        case 3: {
            alpha_blend(2, 0, c, 1.0 / 4.0);
            alpha_blend(1, 2, c, 1.0 / 4.0);

            alpha_blend(2, 1, c, 3.0 / 4.0);
            output_pixels[2][2] = c;
        }
        case 4: {
            alpha_blend(3, 0, c, 1.0 / 4.0);
            alpha_blend(2, 2, c, 1.0 / 4.0);

            alpha_blend(3, 1, c, 3.0 / 4.0);
            alpha_blend(2, 3, c, 3.0 / 4.0);

            output_pixels[3][2] = c;
            output_pixels[3][3] = c;
        }
        case 5: {
            alpha_blend(4, 0, c, 1.0 / 4.0);
            alpha_blend(3, 2, c, 1.0 / 4.0);
            alpha_blend(2, 4, c, 1.0 / 4.0);

            alpha_blend(4, 1, c, 3.0 / 4.0);
            alpha_blend(3, 3, c, 3.0 / 4.0);

            output_pixels[4][2] = c;
            output_pixels[4][3] = c;
            output_pixels[4][4] = c;
            output_pixels[3][4] = c;
        }
        case 6: {
            alpha_blend(5, 0, c, 1.0 / 4.0);
            alpha_blend(4, 2, c, 1.0 / 4.0);
            alpha_blend(3, 4, c, 1.0 / 4.0);

            alpha_blend(5, 1, c, 3.0 / 4.0);
            alpha_blend(4, 3, c, 3.0 / 4.0);
            alpha_blend(3, 5, c, 3.0 / 4.0);

            output_pixels[5][2] = c;
            output_pixels[5][3] = c;
            output_pixels[5][4] = c;
            output_pixels[5][5] = c;

            output_pixels[4][4] = c;
            output_pixels[4][5] = c;
        }
        default: {}
    }
}

fn blend_steep_line(c: vec3f) {
    switch (scale_factor) {
        case 2: {
            alpha_blend(0, 1, c, 1.0 / 4.0);
            alpha_blend(1, 1, c, 3.0 / 4.0);
        }
        case 3: {
            alpha_blend(0, 2, c, 1.0 / 4.0);
            alpha_blend(2, 1, c, 1.0 / 4.0);

            alpha_blend(1, 2, c, 3.0 / 4.0);
            output_pixels[2][2] = c;
        }
        case 4: {
            alpha_blend(0, 3, c, 1.0 / 4.0);
            alpha_blend(2, 2, c, 1.0 / 4.0);

            alpha_blend(1, 3, c, 3.0 / 4.0);
            alpha_blend(3, 2, c, 3.0 / 4.0);

            output_pixels[2][3] = c;
            output_pixels[3][3] = c;
        }
        case 5: {
            alpha_blend(0, 4, c, 1.0 / 4.0);
            alpha_blend(2, 3, c, 1.0 / 4.0);
            alpha_blend(4, 2, c, 1.0 / 4.0);

            alpha_blend(1, 4, c, 3.0 / 4.0);
            alpha_blend(3, 3, c, 3.0 / 4.0);

            output_pixels[2][4] = c;
            output_pixels[3][4] = c;
            output_pixels[4][4] = c;
            output_pixels[4][3] = c;
        }
        case 6: {
            alpha_blend(0, 5, c, 1.0 / 4.0);
            alpha_blend(2, 4, c, 1.0 / 4.0);
            alpha_blend(4, 3, c, 1.0 / 4.0);

            alpha_blend(1, 5, c, 3.0 / 4.0);
            alpha_blend(3, 4, c, 3.0 / 4.0);
            alpha_blend(5, 3, c, 3.0 / 4.0);

            output_pixels[2][5] = c;
            output_pixels[3][5] = c;
            output_pixels[4][5] = c;
            output_pixels[5][5] = c;

            output_pixels[4][4] = c;
            output_pixels[5][4] = c;
        }
        default: {}
    }
}

fn blend_steep_and_shallow_line(c: vec3f) {
    switch (scale_factor) {
        case 2: {
            alpha_blend(1, 0, c, 1.0 / 4.0);
            alpha_blend(0, 1, c, 1.0 / 4.0);
            alpha_blend(1, 1, c, 5.0 / 6.0);
        }
        case 3: {
            alpha_blend(2, 0, c, 1.0 / 4.0);
            alpha_blend(0, 2, c, 1.0 / 4.0);
            alpha_blend(2, 1, c, 3.0 / 4.0);
            alpha_blend(1, 2, c, 3.0 / 4.0);
            output_pixels[2][2] = c;
        }
        case 4: {
            alpha_blend(3, 1, c, 3.0 / 4.0);
            alpha_blend(1, 3, c, 3.0 / 4.0);
            alpha_blend(3, 0, c, 1.0 / 4.0);
            alpha_blend(0, 3, c, 1.0 / 4.0);

            alpha_blend(2, 2, c, 1.0 / 3.0);

            output_pixels[3][3] = c;
            output_pixels[3][2] = c;
            output_pixels[2][3] = c;
        }
        case 5: {
            alpha_blend(0, 4, c, 1.0 / 4.0);
            alpha_blend(2, 3, c, 1.0 / 4.0);
            alpha_blend(1, 4, c, 3.0 / 4.0);

            alpha_blend(4, 0, c, 1.0 / 4.0);
            alpha_blend(3, 2, c, 1.0 / 4.0);
            alpha_blend(4, 1, c, 3.0 / 4.0);

            alpha_blend(3, 3, c, 2.0 / 3.0);

            output_pixels[2][4] = c;
            output_pixels[3][4] = c;
            output_pixels[4][4] = c;

            output_pixels[4][2] = c;
            output_pixels[4][3] = c;
        }
        case 6: {
            alpha_blend(0, 5, c, 1.0 / 4.0);
            alpha_blend(2, 4, c, 1.0 / 4.0);
            alpha_blend(1, 5, c, 3.0 / 4.0);
            alpha_blend(3, 4, c, 3.0 / 4.0);

            alpha_blend(5, 0, c, 1.0 / 4.0);
            alpha_blend(4, 2, c, 1.0 / 4.0);
            alpha_blend(5, 1, c, 3.0 / 4.0);
            alpha_blend(4, 3, c, 3.0 / 4.0);

            output_pixels[2][5] = c;
            output_pixels[3][5] = c;
            output_pixels[4][5] = c;
            output_pixels[5][5] = c;

            output_pixels[4][4] = c;
            output_pixels[5][4] = c;

            output_pixels[5][2] = c;
            output_pixels[5][3] = c;
        }
        default: {}
    }
}

fn blend_diagonal(c: vec3f) {
    switch (scale_factor) {
        case 2: {
            alpha_blend(1, 1, c, 1.0 / 2.0);
        }
        case 3: {
            alpha_blend(1, 2, c, 1.0 / 8.0);
            alpha_blend(2, 1, c, 1.0 / 8.0);
            alpha_blend(2, 2, c, 7.0 / 8.0);
        }
        case 4: {
            alpha_blend(3, 2, c, 1.0 / 2.0);
            alpha_blend(2, 3, c, 1.0 / 2.0);
            output_pixels[3][3] = c;
        }
        case 5: {
            alpha_blend(4, 2, c, 1.0 / 8.0);
            alpha_blend(3, 3, c, 1.0 / 8.0);
            alpha_blend(2, 4, c, 1.0 / 8.0);

            alpha_blend(4, 3, c, 7.0 / 8.0);
            alpha_blend(3, 4, c, 7.0 / 8.0);

            output_pixels[4][4] = c;
        }
        case 6: {
            alpha_blend(5, 3, c, 1.0 / 2.0);
            alpha_blend(4, 4, c, 1.0 / 2.0);
            alpha_blend(3, 5, c, 1.0 / 2.0);

            output_pixels[4][5] = c;
            output_pixels[5][5] = c;
            output_pixels[5][4] = c;
        }
        default: {}
    }
}

fn blend_corner(c: vec3f) {
    switch (scale_factor) {
        case 2: {
            alpha_blend(1, 1, c, 0.2146018366);
        }
        case 3: {
            alpha_blend(2, 2, c, 0.4545939598);
        }
        case 4: {
            alpha_blend(3, 3, c, 0.6848532563);
            alpha_blend(3, 2, c, 0.08677704501);
            alpha_blend(2, 3, c, 0.08677704501);
        }
        case 5: {
            alpha_blend(4, 4, c, 0.8631434088);
            alpha_blend(4, 3, c, 0.2306749731);
            alpha_blend(3, 4, c, 0.2306749731);
        }
        case 6: {
            alpha_blend(5, 5, c, 0.9711013910);
            alpha_blend(4, 5, c, 0.4236372243);
            alpha_blend(5, 4, c, 0.4236372243);
            alpha_blend(5, 3, c, 0.05652034508);
            alpha_blend(3, 5, c, 0.05652034508);
        }
        default: {}
    }
}