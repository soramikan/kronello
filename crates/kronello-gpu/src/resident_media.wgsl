struct Mapping { x: vec4<f32>, y: vec4<f32>, mode: vec4<u32> }
@group(0) @binding(0) var first: texture_2d<f32>;
@group(0) @binding(1) var second: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var<uniform> m: Mapping;
fn linear(v: vec3<f32>) -> vec3<f32> {
    if m.mode.z == 1u { return select(pow(max((v + 0.055)/1.055, vec3<f32>(0)), vec3<f32>(2.4)), v/12.92, v <= vec3<f32>(0.04045)); }
    return select(pow(max((v + 0.099)/1.099, vec3<f32>(0)), vec3<f32>(1.0/0.45)), v/4.5, v < vec3<f32>(0.081));
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(output)) { return; }
    let center = vec3<f32>(vec2<f32>(id.xy) + 0.5, 1);
    let local = vec2<f32>(dot(m.x.xyz, center), dot(m.y.xyz, center));
    let extent = vec2<f32>(m.x.w, m.y.w);
    if any(local < vec2<f32>(0)) || any(local >= extent) { textureStore(output, vec2<i32>(id.xy), vec4<f32>(0)); return; }
    let p = vec2<i32>(floor(local * vec2<f32>(textureDimensions(first)) / extent));
    var rgba = textureLoad(first, p, 0);
    if m.mode.x == 1u {
        let y = (rgba.r * 255.0 - 16.0)/219.0;
        let uv = (textureLoad(second, p/2, 0).rg * 255.0 - vec2<f32>(128.0))/224.0;
        // The software path clamps at its 8-bit RGB conversion stage, so the
        // decoded triangle's legal span is [0,1]; out-of-range bitstream
        // excursions clamp the same way here.
        rgba = vec4<f32>(clamp(vec3<f32>(y + 1.5748*uv.y, y - 0.187324*uv.x - 0.468124*uv.y, y + 1.8556*uv.x), vec3<f32>(0.0), vec3<f32>(1.0)), 1);
    }
    var rgb = linear(rgba.rgb);
    if m.mode.y == 1u {
        rgb = vec3<f32>(dot(rgb, vec3<f32>(0.6274039,0.3292830,0.0433131)), dot(rgb, vec3<f32>(0.0690973,0.9195404,0.0113623)), dot(rgb, vec3<f32>(0.0163914,0.0880133,0.8955953)));
    }
    textureStore(output, vec2<i32>(id.xy), vec4<f32>(rgb*rgba.a, rgba.a));
}
