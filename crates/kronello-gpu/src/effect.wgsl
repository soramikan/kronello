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
    return bilinear_parts(base,f);
}
fn bilinear_parts(base:vec2<i32>, f:vec2<f32>)->vec4<f32> {
    let a=load(base); let b=load(base+vec2<i32>(1,0));
    let c=load(base+vec2<i32>(0,1)); let d=load(base+vec2<i32>(1,1));
    let top=a+(b-a)*f.x; let bottom=c+(d-c)*f.x;
    return top+(bottom-top)*f.y;
}
// Explicit IEEE binary16 RNE prevents backend storage conversion from choosing
// a different rounding mode. The returned f32 is exactly half-representable.
fn half_rne(value:f32)->f32 {
    let bits=bitcast<u32>(value); let sign=bits & 0x80000000u; let magnitude=bits & 0x7fffffffu;
    if magnitude>=0x38800000u {
        let rounded=(magnitude+0xfffu+((magnitude>>13u)&1u))&0xffffe000u;
        return bitcast<f32>(sign|rounded);
    }
    let scaled=abs(value)*16777216.0; let base=u32(floor(scaled)); let fraction=scaled-f32(base);
    let increment=select(0u,1u,fraction>0.5 || (fraction==0.5 && (base&1u)==1u));
    let rounded=f32(base+increment)/16777216.0;
    return select(rounded,-rounded,sign!=0u);
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
    } else if params.config.x==2u {
        var norm=0.0;
        for (var i=0u; i<params.config.y; i++) {
            let delta=vec2<i32>(i32(weights[3u*i]),i32(weights[3u*i+1u]));
            let w=weights[3u*i+2u]; result+=load(p+delta)*w; norm+=w;
        }
        result/=norm;
    } else {
        let s=textureLoad(original,p,0);
        var alpha=0.0;
        if params.config.x==3u {
            let shift=floor(-params.offset.xy);
            alpha=bilinear_parts(p+vec2<i32>(shift),-params.offset.xy-shift).a;
        } else { alpha=bilinear(vec2<f32>(p)-params.offset.xy).a; }
        let shadow=params.color*alpha;
        result=s+shadow*(1.0-s.a);
    }
    if any(abs(result.rgb)>vec3<f32>(65504.0)) || any(result!=result) || result.a<0.0 || result.a>1.0 {atomicStore(&validation,1u);}
    result=vec4<f32>(half_rne(result.r),half_rne(result.g),half_rne(result.b),half_rne(result.a));
    if result.a==0.0 {result=vec4<f32>(0.0);}
    textureStore(output,p,result);
}
