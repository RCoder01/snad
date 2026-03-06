struct Uniforms {
    width: u32,
    height: u32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var<storage, read> snad: array<u32>;

struct VSOut {
    @builtin(position) pos: vec4f,
    @location(0) coord: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VSOut {
    let lut = array<vec2f, 6>(
        vec2(-1, -1),
        vec2(-1, 1),
        vec2(1, -1),

        vec2(1, -1),
        vec2(-1, 1),
        vec2(1, 1),
    );
    let x = lut[index].x;
    let y = lut[index].y;
    let coord = vec2(f32((x+1) * f32(uniforms.width) / 2.0), f32((y+1) * f32(uniforms.height) / 2.0));
    return VSOut(vec4f(x, y, 0.0, 1.0), coord);
}

struct FSIn {
    @location(0) coord: vec2<f32>,
}

@fragment
fn fs_main(in: FSIn) -> @location(0) vec4<f32> {
    let coords = vec2(u32(in.coord.x), u32(in.coord.y));
    let index = coords.y * uniforms.width + coords.x;
    let grain = snad[index];
    if grain == 0 {
        return vec4f(1.0, 1.0, 0.0, 0.0);
    } else if grain == 1 {
        return vec4f(0.0, 0.0, 1.0, 0.0);
    } else if grain == 2 {
        return vec4f(0.0, 0.0, 0.0, 0.0);
    } else {
        return vec4f(1.0, 0.0, 0.0, 0.0);
    }
}

