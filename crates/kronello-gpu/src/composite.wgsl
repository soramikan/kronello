struct Params {
    size: vec2<f32>, translation: vec2<f32>,
    rotation: vec2<f32>, input_space: u32, working_space: u32,
    pixel_scale: vec2<f32>, padding: vec2<f32>,
}
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var previous: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var<uniform> params: Params;
fn decode(v: f32) -> f32 {
    if v <= 0.04045 { return v / 12.92; }
    return pow((v + 0.055) / 1.055, 2.4);
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dimensions = textureDimensions(output);
    if any(id.xy >= dimensions) { return; }
    let position = vec2<i32>(id.xy);
    var result = textureLoad(previous, position, 0);
    let d = (vec2<f32>(id.xy) + vec2<f32>(0.5)) * params.pixel_scale - params.translation;
    let c = params.rotation.x; let s = params.rotation.y;
    let local = vec2<f32>(c*d.x + s*d.y, -s*d.x + c*d.y);
    if all(local >= vec2<f32>(0.0)) && all(local < params.size) {
        let sample_position = vec2<i32>(local / params.size * vec2<f32>(textureDimensions(source)));
        let p = textureLoad(source, sample_position, 0);
        var rgb = p.rgb;
        if params.input_space == 0u { rgb = vec3<f32>(decode(rgb.r), decode(rgb.g), decode(rgb.b)); }
        let input_2020 = params.input_space == 2u;
        let output_2020 = params.working_space == 1u;
        if !input_2020 && output_2020 {
            rgb = vec3<f32>(dot(rgb, vec3<f32>(0.627404,0.329282,0.0433136)), dot(rgb,vec3<f32>(0.0690973,0.9195404,0.0113623)),dot(rgb,vec3<f32>(0.0163914,0.0880133,0.8955953)));
        } else if input_2020 && !output_2020 {
            rgb = vec3<f32>(dot(rgb,vec3<f32>(1.660491,-0.5876411,-0.0728499)),dot(rgb,vec3<f32>(-0.1245505,1.1328999,-0.0083494)),dot(rgb,vec3<f32>(-0.0181508,-0.1005789,1.1187297)));
        }
        var premultiplied = vec4<f32>(rgb*p.a,p.a);
        if p.a == 0.0 { premultiplied = vec4<f32>(0.0); }
        result = premultiplied + result * (1.0-p.a);
    }
    if result.a == 0.0 { result = vec4<f32>(0.0); }
    textureStore(output, position, result);
}
