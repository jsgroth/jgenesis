struct Vertex {
    @location(0) position: vec2f,
}

@vertex
fn vs_main(input: Vertex) -> @builtin(position) vec4f {
    return vec4f(input.position, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return vec4f(0.0, 0.0, 0.0, 0.8);
}