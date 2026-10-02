struct Params { values: vec4<f32> }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba16float,write>;
@group(0) @binding(2) var<uniform> params: Params;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(output)) { return; }
    var result = textureLoad(source,vec2<i32>(id.xy),0) * params.values.x;
    if result.a == 0.0 { result = vec4<f32>(0.0); }
    textureStore(output,vec2<i32>(id.xy),result);
}
