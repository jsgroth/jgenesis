/*
   Copyright 2020 Morgan McGuire & Mara Gagiu.
   Provided under the Open Source MIT license https://opensource.org/licenses/MIT
*/

// WGSL port of MMPX GLSL
// Copyright (C) 2026 James Groth

// MMPX pixel art magnification algorithm: https://casual-effects.com/research/McGuire2021PixelArt/index.html

@group(0) @binding(0) var input_frame: texture_2d<f32>;
@group(0) @binding(1) var output_frame: texture_storage_2d<rgba8unorm, write>;

var<private> input_size: vec2i;

fn load_input(position: vec2i) -> vec3f {
    return textureLoad(
        input_frame,
        clamp(position, vec2i(0), input_size - 1),
        0,
    ).rgb;
}

// Coefficients used for Y in RGB-to-YUV conversion
const RGB_TO_Y: vec3f = vec3f(0.299, 0.587, 0.114);

fn luma(c: vec3f) -> f32 {
    return dot(c, RGB_TO_Y);
}

fn eq(a: vec3f, b: vec3f) -> bool {
    return all(a == b);
}

fn ne(a: vec3f, b: vec3f) -> bool {
    return any(a != b);
}

fn all_eq2(B: vec3f, A0: vec3f, A1: vec3f) -> bool {
    return all((B == A0) & (B == A1));
}

fn all_eq3(B: vec3f, A0: vec3f, A1: vec3f, A2: vec3f) -> bool {
    return all((B == A0) & (B == A1) & (B == A2));
}

fn all_eq4(B: vec3f, A0: vec3f, A1: vec3f, A2: vec3f, A3: vec3f) -> bool {
    return all((B == A0) & (B == A1) & (B == A2) & (B == A3));
}

fn any_eq3(B: vec3f, A0: vec3f, A1: vec3f, A2: vec3f) -> bool {
    return eq(B, A0) || eq(B, A1) || eq(B, A2);
}

fn none_eq2(B: vec3f, A0: vec3f, A1: vec3f) -> bool {
    return ne(B, A0) && ne(B, A1);
}

fn none_eq4(B: vec3f, A0: vec3f, A1: vec3f, A2: vec3f, A3: vec3f) -> bool {
    return ne(B, A0) && ne(B, A1) && ne(B, A2) && ne(B, A3);
}

@compute @workgroup_size(16, 16, 1)
fn mmpx(@builtin(global_invocation_id) invocation: vec3u) {
    let in_pos = vec2i(invocation.xy);
    input_size = vec2i(textureDimensions(input_frame));
    if any(in_pos >= input_size) {
        return;
    }

    // Input pixels (5x5, E is pixel at current position):
    //   - - P - -
    //   - A B C -
    //   Q D E F R
    //   - G H I -
    //   - - S - -
    // Certain branches load additional pixels beyond these

    let A = load_input(in_pos + vec2i(-1, -1));
    let B = load_input(in_pos + vec2i( 0, -1));
    let C = load_input(in_pos + vec2i( 1, -1));
    let D = load_input(in_pos + vec2i(-1,  0));
    let E = load_input(in_pos + vec2i( 0,  0));
    let F = load_input(in_pos + vec2i( 1,  0));
    let G = load_input(in_pos + vec2i(-1,  1));
    let H = load_input(in_pos + vec2i( 0,  1));
    let I = load_input(in_pos + vec2i( 1,  1));

    // Output pixels (2x2):
    //   J K
    //   L M

    var J = E;
    var K = E;
    var L = E;
    var M = E;

    if any((A != E) | (B != E) | (C != E) | (D != E) | (F != E) | (G != E) | (H != E) | (I != E)) {
        let P = load_input(in_pos + vec2i( 0, -2));
        let S = load_input(in_pos + vec2i( 0,  2));
        let Q = load_input(in_pos + vec2i(-2,  0));
        let R = load_input(in_pos + vec2i( 2,  0));

        let Bl = luma(B);
        let Dl = luma(D);
        let El = luma(E);
        let Fl = luma(F);
        let Hl = luma(H);

        // 1:1 slope rules
        if (eq(D, B) && ne(D, H) && ne(D, F))
            && (El >= Dl || eq(E, A))
            && any_eq3(E, A, C, G)
            && ((El < Dl) || ne(A, D) || ne(E, P) || ne(E, Q))
        {
            J = D;
        }
        if (eq(B, F) && ne(B, D) && ne(B, H))
            && (El >= Bl || eq(E, C))
            && any_eq3(E, A, C, I)
            && ((El < Bl) || ne(C, B) || ne(E, P) || ne(E, R))
        {
            K = B;
        }
        if (eq(H, D) && ne(H, F) && ne(H, B))
            && (El >= Hl || eq(E, G))
            && any_eq3(E, A, G, I)
            && ((El < Hl) || ne(G, H) || ne(E, S) || ne(E, Q))
        {
            L = H;
        }
        if (eq(F, H) && ne(F, B) && ne(F, D))
            && (El >= Fl || eq(E, I))
            && any_eq3(E, C, G, I)
            && ((El < Fl) || ne(I, H) || ne(E, R) || ne(E, S))
        {
            M = F;
        }

        // Intersection rules
        if ne(E, F) && all_eq4(E, C, I, D, Q) && all_eq2(F, B, H) && ne(F, load_input(in_pos + vec2i(3, 0))) {
            M = F;
            K = F;
        }
        if ne(E, D) && all_eq4(E, A, G, F, R) && all_eq2(D, B, H) && ne(D, load_input(in_pos + vec2i(-3, 0))) {
            L = D;
            J = D;
        }
        if ne(E, H) && all_eq4(E, G, I, B, P) && all_eq2(H, D, F) && ne(H, load_input(in_pos + vec2i(0, 3))) {
            M = H;
            L = H;
        }
        if ne(E, B) && all_eq4(E, A, C, H, S) && all_eq2(B, D, F) && ne(B, load_input(in_pos + vec2i(0, -3))) {
            K = B;
            J = B;
        }
        if Bl < El && all_eq4(E, G, H, I, S) && none_eq4(E, A, D, C, F) {
            K = B;
            J = B;
        }
        if Hl < El && all_eq4(E, A, B, C, P) && none_eq4(E, D, G, I, F) {
            M = H;
            L = H;
        }
        if Fl < El && all_eq4(E, A, D, G, Q) && none_eq4(E, B, C, I, H) {
            M = F;
            K = F;
        }
        if Dl < El && all_eq4(E, C, F, I, R) && none_eq4(E, B, A, G, H) {
            L = D;
            J = D;
        }

        // 2:1 slope rules
        if ne(H, B) {
            if ne(H, A) && ne(H, E) && ne(H, C) {
                if all_eq3(H, G, F, R) && none_eq2(H, D, load_input(in_pos + vec2i(2, -1))) {
                    L = M;
                }
                if all_eq3(H, I, D, Q) && none_eq2(H, F, load_input(in_pos + vec2i(-2, -1))) {
                    M = L;
                }
            }

            if ne(B, I) && ne(B, G) && ne(B, E) {
                if all_eq3(B, A, F, R) && none_eq2(B, D, load_input(in_pos + vec2i(2, 1))) {
                    J = K;
                }
                if all_eq3(B, C, D, Q) && none_eq2(B, F, load_input(in_pos + vec2i(-2, 1))) {
                    K = J;
                }
            }
        } // H !== B

        if ne(F, D) {
            if ne(D, I) && ne(D, E) && ne(D, C) {
                if all_eq3(D, A, H, S) && none_eq2(D, B, load_input(in_pos + vec2i(1, 2))) {
                    J = L;
                }
                if all_eq3(D, G, B, P) && none_eq2(D, H, load_input(in_pos + vec2i(1, -2))) {
                    L = J;
                }
            }

            if ne(F, E) && ne(F, A) && ne(F, G) {
                if all_eq3(F, C, H, S) && none_eq2(F, B, load_input(in_pos + vec2i(-1, 2))) {
                    K = M;
                }
                if all_eq3(F, I, B, P) && none_eq2(F, H, load_input(in_pos + vec2i(-1, -2))) {
                    M = K;
                }
            }
        } // F !== D
    } // not constant

    // Write four pixels at once
    let out_pos_tl = 2 * in_pos;
    textureStore(output_frame, out_pos_tl + vec2i(0, 0), vec4f(J, 1.0));
    textureStore(output_frame, out_pos_tl + vec2i(1, 0), vec4f(K, 1.0));
    textureStore(output_frame, out_pos_tl + vec2i(0, 1), vec4f(L, 1.0));
    textureStore(output_frame, out_pos_tl + vec2i(1, 1), vec4f(M, 1.0));
}