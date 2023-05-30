// Vertex shader

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) uv: vec2<f32>
};

struct PanZoomUniform {
    pan: vec2<f32>,
    zoom: vec2<f32>
}
@group(0) @binding(0)
var<uniform> pan_zoom: PanZoomUniform;

@vertex
fn vs_main(
    vertex: VertexInput,
    @location(2) instance: u32
) -> VertexOutput {
    var coords = vec2<f32>(hilbert_decode(instance, N));
    coords = coords / f32(1u << 16u) * 2. - 1.;
    var pos = vertex.position.xy;
    pos = pos / f32(1u << 16u) + coords;
    pos += pan_zoom.pan;
    pos *= pan_zoom.zoom;
    var out: VertexOutput;
    out.uv = vertex.uv;
    out.clip_position = vec4<f32>(pos, 1., 1.0);
    return out;
}

// Fragment shader

const N: u32 = 16u;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let side_length = f32(1u << N);
    var x = u32(in.uv.x * side_length);
    var y = u32(in.uv.y * side_length);
    let hilb = hilbert_encode(x, y, N);
    let color = 1.;
    // let color = f32_norm(hilb);
    return vec4<f32>(color, color, color, 1.0);
}

fn f32_norm(v: u32) -> f32 {
    return f32(v / 4u) / f32(1u << 30u);
}

fn hilbert_encode(x: u32, y: u32, bits: u32) -> u32 {
    var x = x;
    var y = y;
    var d: u32 = 0u;
    for (var s: u32 = (1u << bits) ; s > 0u; s /= 2u) {
        var rx = 0u;
        var ry = 0u;
        if (x & s) > 0u {rx = 1u;}
        if (y & s) > 0u {ry = 1u;}
        d += s * s * ((3u * rx) ^ ry);
        if ry == 0u {
            if rx == 1u {
                x = s - 1u - x;
                y = s - 1u - y;
            }
            let tmp = x;
            x = y;
            y = tmp;
        }
    }
    return d;
}

fn hilbert_decode(d: u32, bits: u32) -> vec2<u32> {
    var out = vec2<u32>(0u, 0u);
    var d = d;
    for (var s: u32 = 1u ; s < (1u << bits); s *= 2u) {
        var rx = 1u & (d / 2u);
        var ry = 1u & (d ^ rx);
        if ry == 0u {
            if rx == 1u {
                out.x = s - 1u - out.x;
                out.y = s - 1u - out.y;
            }
            let tmp = out.x;
            out.x = out.y;
            out.y = tmp;
        }
        out.x += s * rx;
        out.y += s * ry;
        d /= 4u;
    }
    return out;
}