struct Params { config:vec4<u32>, offset:vec4<f32>, color:vec4<f32> }
@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var original:texture_2d<f32>;
@group(0) @binding(2) var output:texture_storage_2d<rgba16float,write>;
@group(0) @binding(3) var<uniform> params:Params;
@group(0) @binding(4) var<storage,read> weights:array<f32>;
@group(0) @binding(5) var<storage,read_write> validation:atomic<u32>;
fn load(p:vec2<i32>)->vec4<f32> {
    if any(p<vec2<i32>(0)) || any(p>=vec2<i32>(textureDimensions(source))) {return vec4<f32>(0.0);}
    return textureLoad(source,p,0);
}
fn bilinear(p:vec2<f32>)->vec4<f32> {
    let base=vec2<i32>(floor(p)); let f=p-vec2<f32>(base);
    let a=load(base); let b=load(base+vec2<i32>(1,0));
    let c=load(base+vec2<i32>(0,1)); let d=load(base+vec2<i32>(1,1));
    let top=a+(b-a)*f.x; let bottom=c+(d-c)*f.x;
    return top+(bottom-top)*f.y;
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=textureDimensions(output)) {return;}
    let p=vec2<i32>(id.xy); var result=vec4<f32>(0.0);
    if params.config.x==0u {
        let radius=i32(params.config.y); var norm=0.0;
        for (var i=-radius; i<=radius; i++) {
            var delta=vec2<i32>(i,0); if params.config.z==1u {delta=vec2<i32>(0,i);}
            let w=weights[u32(i+radius)]; result+=load(p+delta)*w; norm+=w;
        }
        result/=norm;
    } else {
        let s=textureLoad(original,p,0);
        let shadow=params.color*bilinear(vec2<f32>(p)-params.offset.xy).a;
        result=s+shadow*(1.0-s.a);
    }
    if any(abs(result.rgb)>vec3<f32>(65504.0)) || any(result!=result) || result.a<0.0 || result.a>1.0 {atomicStore(&validation,1u);}
    if result.a<=1.0/33554432.0 {result=vec4<f32>(0.0);}
    textureStore(output,p,result);
}
