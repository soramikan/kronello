struct Params {
    config: vec4<u32>, // mode, edge count, fill rule, working space
    scale: vec4<f32>, // pixel scale, opacity, stroke width
    fill: vec4<f32>,
    stroke: vec4<f32>,
    spaces: vec4<u32>, // fill space, stroke space, mask kind, output space
    boundary: vec4<u32>, // output alpha association
    fill_gradient: vec4<u32>,
    fill_geometry: vec4<f32>,
    fill_extra: vec4<f32>,
    fill_x: vec4<f32>,
    fill_y: vec4<f32>,
    stroke_gradient: vec4<u32>,
    stroke_geometry: vec4<f32>,
    stroke_extra: vec4<f32>,
    stroke_x: vec4<f32>,
    stroke_y: vec4<f32>,
    paint_x: vec4<f32>,
    paint_y: vec4<f32>,
    local_stroke_x: vec4<f32>,
    local_stroke_y: vec4<f32>,
}
struct Edge { points: vec4<f32>, flags: vec4<u32>, extra: vec4<f32> }
struct Stop { rgba: vec4<f32>, offset: vec4<f32>, space: vec4<u32> }
@group(0) @binding(6) var<storage,read> stops: array<Stop>;
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
fn cross2(a:vec2<f32>,b:vec2<f32>)->f32 { return a.x*b.y-a.y*b.x; }
fn primitive_hit(p:vec2<f32>,e:Edge)->bool {
    let a=e.points.xy; let b=e.points.zw; let c=e.extra.xy;
    if e.flags.y==2u { let d=p-a; return dot(d,d)<=e.extra.x*e.extra.x; }
    if cross2(b-a,c-a)==0.0 { return false; }
    let x=cross2(b-a,p-a); let y=cross2(c-b,p-b); let z=cross2(a-c,p-c);
    return (x>=0.0 && y>=0.0 && z>=0.0)||(x<=0.0 && y<=0.0 && z<=0.0);
}
fn gradient_prepare(s:Stop, mode:u32)->vec4<f32> {
    // Premultiplied modes discard hidden RGB before any conversion overflow.
    if (mode==0u || mode==3u) && s.rgba.a==0.0 { return vec4<f32>(0.0); }
    var rgb=s.rgba.rgb;
    if s.space.x==0u { rgb=vec3<f32>(decode(rgb.r),decode(rgb.g),decode(rgb.b)); }
    rgb=primaries(rgb,s.space.x==2u,mode<2u && params.config.w==1u);
    if mode>=2u { rgb=vec3<f32>(encode(rgb.r),encode(rgb.g),encode(rgb.b)); }
    if mode==0u || mode==3u { rgb=rgb*s.rgba.a; }
    return vec4<f32>(rgb,s.rgba.a);
}
fn gradient_finish(value:vec4<f32>, mode:u32)->vec4<f32> {
    if mode==0u { return value; }
    if value.a==0.0 { return vec4<f32>(0.0); }
    if mode==1u { return vec4<f32>(value.rgb*value.a,value.a); }
    var rgb=value.rgb;
    if mode==3u {
        rgb=rgb/value.a;
    }
    rgb=vec3<f32>(decode(rgb.r),decode(rgb.g),decode(rgb.b));
    return vec4<f32>(primaries(rgb,false,params.config.w==1u)*value.a,value.a);
}
fn finite_parameter(v:f32)->bool {
    return (bitcast<u32>(v)&0x7f800000u)!=0x7f800000u;
}
fn gradient_paint(solid:vec4<f32>,space:u32,g:vec4<u32>,geometry:vec4<f32>,extra:vec4<f32>,gx:vec4<f32>,gy:vec4<f32>,p:vec2<f32>)->vec4<f32> {
    if g.x==0u { return paint(solid,space); }
    let object=vec2<f32>(dot(params.paint_x.xy,p)+params.paint_x.z,dot(params.paint_y.xy,p)+params.paint_y.z);
    let local=vec2<f32>(dot(gx.xy,object)+gx.z,dot(gy.xy,object)+gy.z);
    if !finite_parameter(local.x) || !finite_parameter(local.y) {
        atomicStore(&validation,1u); return vec4<f32>(0.0);
    }
    var t=0.0;
    if g.x==1u { let d=geometry.zw-geometry.xy; t=dot(local-geometry.xy,d)/dot(d,d); }
    else if g.x==2u { t=length(local-geometry.xy)/geometry.z; }
    else if g.x==3u {
        let q=local-extra.xy; let d=geometry.xy-extra.xy; let dr=geometry.z-extra.z;
        let a=dr*dr-dot(d,d); let b=dot(q,d)+extra.z*dr; let c=dot(q,q)-extra.z*extra.z;
        if c>0.0 {
            let root=sqrt(b*b+a*c);
            if !finite_parameter(root) { atomicStore(&validation,1u); return vec4<f32>(0.0); }
            if b>=0.0 { t=c/(root+b); } else { t=(root-b)/a; }
        }
    } else {
        let q=local-geometry.xy;
        if any(q!=vec2<f32>(0.0)) {
            let angle=atan2(q.y,q.x)-geometry.z;
            let tau=6.283185307179586;
            t=(angle-tau*floor(angle/tau))/geometry.w;
        }
    }
    let spread=g.w&3u; let mode=g.w>>2u;
    // Preserve legacy pad endpoints at infinity; periodic phase is undefined.
    if !finite_parameter(t) && (spread!=0u || t!=t) { atomicStore(&validation,1u); return vec4<f32>(0.0); }
    if spread==1u { t=t-floor(t); }
    else if spread==2u { let u=t-2.0*floor(t/2.0); t=select(u,2.0-u,u>1.0); }
    var previous=stops[g.y];
    if t<previous.offset.x { return gradient_finish(gradient_prepare(previous,mode),mode); }
    for (var i=1u;i<g.z;i++) {
        let stop=stops[g.y+i];
        if t<stop.offset.x {
            let f=(t-previous.offset.x)/(stop.offset.x-previous.offset.x);
            let a=gradient_prepare(previous,mode); let b=gradient_prepare(stop,mode);
            return gradient_finish(a+(b-a)*f,mode);
        }
        previous=stop;
    }
    return gradient_finish(gradient_prepare(previous,mode),mode);
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(output)) { return; }
    let position = vec2<i32>(id.xy);
    var result = vec4<f32>(0.0);
    switch params.config.x {
        case 0u: {
            var fill_color=vec4<f32>(0.0);
            var stroke_color=vec4<f32>(0.0);
            for (var sy=0u; sy<4u; sy++) {
                for (var sx=0u; sx<4u; sx++) {
                    let p = (vec2<f32>(id.xy)+(vec2<f32>(f32(sx),f32(sy))+0.5)/4.0)*params.scale.xy;
                    var winding = 0i;
                    var stroked = false;
                    let stroke_p = vec2<f32>(dot(params.local_stroke_x.xy,p)+params.local_stroke_x.z,dot(params.local_stroke_y.xy,p)+params.local_stroke_y.z);
                    for (var e=0u; e<params.config.y; e++) {
                        if edges[e].flags.y!=0u {
                            stroked=stroked || primitive_hit(stroke_p,edges[e]);
                        } else {
                            let a=edges[e].points.xy; let b=edges[e].points.zw;
                            let cross=cross2(b-a,p-a);
                            if a.y<=p.y && b.y>p.y && cross>0.0 { winding++; }
                            if b.y<=p.y && a.y>p.y && cross<0.0 { winding--; }
                        }
                    }
                    if (params.config.z==0u && winding!=0i) || (params.config.z==1u && winding%2i!=0i) { fill_color+=gradient_paint(params.fill,params.spaces.x,params.fill_gradient,params.fill_geometry,params.fill_extra,params.fill_x,params.fill_y,p)/16.0; }
                    let inside = (params.boundary.z==0u && winding!=0i) || (params.boundary.z==1u && winding%2i!=0i);
                    let aligned = params.boundary.y==0u || (params.boundary.y==1u && inside) || (params.boundary.y==2u && !inside);
                    if stroked && aligned { stroke_color+=gradient_paint(params.stroke,params.spaces.y,params.stroke_gradient,params.stroke_geometry,params.stroke_extra,params.stroke_x,params.stroke_y,p)/16.0; }
                }
            }
            result=over(stroke_color,fill_color);
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
