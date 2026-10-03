struct Params {
    config: vec4<u32>, // mode, edge count, fill rule, working space
    scale: vec4<f32>, // pixel scale, opacity, stroke width
    fill: vec4<f32>,
    stroke: vec4<f32>,
    spaces: vec4<u32>, // fill space, stroke space, mask kind, output space
    boundary: vec4<u32>, // output alpha association
}
struct Edge { points: vec4<f32>, flags: vec4<u32> }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var previous: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba16float,write>;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var<storage,read> edges: array<Edge>;
@group(0) @binding(5) var<storage,read_write> validation: atomic<u32>;
fn decode(v: f32) -> f32 {
    if v <= 0.04045 { return v/12.92; }
    return pow((v+0.055)/1.055,2.4);
}
fn encode(v: f32) -> f32 {
    if v <= 0.0031308 { return v*12.92; }
    return 1.055*pow(v,1.0/2.4)-0.055;
}
fn primaries(rgb: vec3<f32>, from2020: bool, to2020: bool) -> vec3<f32> {
    if !from2020 && to2020 {
        return vec3<f32>(dot(rgb,vec3<f32>(0.627404,0.329282,0.0433136)),dot(rgb,vec3<f32>(0.0690973,0.9195404,0.0113623)),dot(rgb,vec3<f32>(0.0163914,0.0880133,0.8955953)));
    }
    if from2020 && !to2020 {
        return vec3<f32>(dot(rgb,vec3<f32>(1.660491,-0.5876411,-0.0728499)),dot(rgb,vec3<f32>(-0.1245505,1.1328999,-0.0083494)),dot(rgb,vec3<f32>(-0.0181508,-0.1005789,1.1187297)));
    }
    return rgb;
}
fn paint(p: vec4<f32>, space: u32) -> vec4<f32> {
    if p.a == 0.0 { return vec4<f32>(0.0); }
    var rgb = p.rgb;
    if space == 0u { rgb = vec3<f32>(decode(rgb.r),decode(rgb.g),decode(rgb.b)); }
    return vec4<f32>(primaries(rgb,space==2u,params.config.w==1u)*p.a,p.a);
}
fn over(s: vec4<f32>, d: vec4<f32>) -> vec4<f32> { return s+d*(1.0-s.a); }
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(output)) { return; }
    let position = vec2<i32>(id.xy);
    var result = vec4<f32>(0.0);
    switch params.config.x {
        case 0u: {
            var fill_hits = 0u;
            var stroke_hits = 0u;
            for (var sy=0u; sy<4u; sy++) {
                for (var sx=0u; sx<4u; sx++) {
                    let p = (vec2<f32>(id.xy)+(vec2<f32>(f32(sx),f32(sy))+0.5)/4.0)*params.scale.xy;
                    var winding = 0i;
                    var stroked = false;
                    for (var e=0u; e<params.config.y; e++) {
                        let a = edges[e].points.xy;
                        let b = edges[e].points.zw;
                        let d = b-a;
                        let q = p-a;
                        let cross = d.x*q.y-d.y*q.x;
                        if a.y<=p.y && b.y>p.y && cross>0.0 { winding++; }
                        if b.y<=p.y && a.y>p.y && cross<0.0 { winding--; }
                        if edges[e].flags.x==1u && params.scale.w>0.0 {
                            let length = dot(d,d);
                            var t = 0.0;
                            if length>0.0 { t=clamp(dot(q,d)/length,0.0,1.0); }
                            let v=q-t*d;
                            stroked = stroked || dot(v,v)<=params.scale.w*params.scale.w*0.25;
                        }
                    }
                    if (params.config.z==0u && winding!=0i) || (params.config.z==1u && winding%2i!=0i) { fill_hits++; }
                    if stroked { stroke_hits++; }
                }
            }
            let f=paint(params.fill,params.spaces.x)*f32(fill_hits)/16.0;
            let s=paint(params.stroke,params.spaces.y)*f32(stroke_hits)/16.0;
            result=over(s,f);
        }
        case 1u: { result=over(textureLoad(source,position,0),textureLoad(previous,position,0)); }
        case 2u: { result=textureLoad(source,position,0)*params.scale.z; }
        case 3u: {
            let matte=textureLoad(previous,position,0);
            var coverage=matte.a;
            if params.spaces.z==1u {
                var weights=vec3<f32>(0.2126,0.7152,0.0722);
                if params.config.w==1u { weights=vec3<f32>(0.2627,0.6780,0.0593); }
                coverage=clamp(dot(matte.rgb,weights),0.0,1.0);
            }
            result=textureLoad(source,position,0)*coverage;
        }
        case 4u: {
            let p=textureLoad(source,position,0);
            var rgb=vec3<f32>(0.0);
            if p.a>1.0/65536.0 { rgb=p.rgb/p.a; }
            rgb=primaries(rgb,params.config.w==1u,params.spaces.w==2u);
            if params.spaces.w==0u { rgb=vec3<f32>(encode(rgb.r),encode(rgb.g),encode(rgb.b)); }
            if params.boundary.x==1u { rgb=rgb*p.a; }
            result=vec4<f32>(rgb,p.a);
        }
        default: {}
    }
    // Detect overflow before backend half conversion can saturate to a finite
    // maximum. A shared sticky flag preserves errors hidden by later overdraw.
    if any(abs(result.rgb)>vec3<f32>(65504.0)) || any(result!=result) || result.a<0.0 || result.a>1.0 {
        atomicStore(&validation,1u);
    }
    // Normalize RGB if binary16 alpha underflows, without applying the external
    // epsilon to positive internal alpha. No RGB clamp in this pipeline.
    if result.a<=1.0/33554432.0 { result=vec4<f32>(0.0); }
    textureStore(output,position,result);
}
