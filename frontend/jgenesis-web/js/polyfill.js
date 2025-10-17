// Stub polyfill for TextDecoder and TextEncoder based on:
//   https://github.com/wasm-bindgen/wasm-bindgen/pull/3017/files
//
// According to docs this shouldn't be necessary in the latest version of wasm-bindgen, but at least on
// wasm-bindgen v0.2.104 the audio worklet fails to initialize without this polyfill.

if (!globalThis.TextDecoder) {
    globalThis.TextDecoder = class TextDecoder {
        decode(arg) {
            if (typeof arg !== "undefined") {
                throw Error("TextDecoder stub called");
            } else {
                return "";
            }
        }
    };
}

if (!globalThis.TextEncoder) {
    globalThis.TextEncoder = class TextEncoder {
        encode(arg) {
            if (typeof arg !== "undefined") {
                throw Error("TextEncoder stub called");
            } else {
                return new Uint8Array(0);
            }
        }
    };
}

export function run_polyfill() {}