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
// FX-003 W3C separable/non-separable blend functions on straight channels
// (ADR-0109). HDR and negative values are never clamped. Operation ids are the
// fixed table: separable 5..=19, non-separable 20..=23.
fn blend_dodge(b:f32,s:f32)->f32 { if s>=1.0 {return 1.0;} return min(b/(1.0-s),1.0); }
fn blend_burn(b:f32,s:f32)->f32 { if s<=0.0 {return 0.0;} return 1.0-min(1.0-b,s)/s; }
fn blend_soft(b:f32,s:f32)->f32 {
    if s<=0.5 { return b-(1.0-2.0*s)*b*(1.0-b); }
    var d=b;
    if b<=0.25 { d=((16.0*b-12.0)*b+4.0)*b; } else { d=sqrt(b); }
    return b+(2.0*s-1.0)*(d-b);
}
fn blend_overlay(b:f32,s:f32)->f32 {
    if b<=0.5 { return 2.0*b*s; }
    return 1.0-2.0*(1.0-b)*(1.0-s);
}
// FXC (DX12) cannot compile switch statements nested inside the main
// dispatch switch, so the channel dispatch stays an if/else chain.
fn blend_channel(b:f32,s:f32,op:u32)->f32 {
    if op==5u { return b*s; }
    if op==6u { return b+s-b*s; }
    if op==7u { return min(b,s); }
    if op==8u { return max(b,s); }
    if op==9u { return blend_dodge(b,s); }
    if op==10u { return blend_burn(b,s); }
    if op==11u { return blend_overlay(s,b); }
    if op==12u { return blend_soft(b,s); }
    if op==13u { return abs(b-s); }
    if op==14u { return b+s-2.0*b*s; }
    if op==15u { return blend_overlay(b,s); }
    if op==16u { return b+s; }
    if op==17u { return b+s-1.0; }
    if op==18u { if s<=0.5 { return blend_burn(b,2.0*s); } return blend_dodge(b,2.0*(s-0.5)); }
    return b+2.0*s-1.0;
}
fn blend_lum(c:vec3<f32>)->f32 { return c.r*0.3+c.g*0.59+c.b*0.11; }
fn blend_set_lum(c:vec3<f32>,l:f32)->vec3<f32> { return c+vec3<f32>(l-blend_lum(c)); }
fn blend_sat(c:vec3<f32>)->f32 { return max(c.r,max(c.g,c.b))-min(c.r,min(c.g,c.b)); }
fn blend_set_sat(c:vec3<f32>,s:f32)->vec3<f32> {
    // First minimum, last maximum: identical index selection to color.rs.
    // Dynamic vector indexing is avoided for FXC (DX12) compatibility.
    var lo_v=c.r; var lo=0u;
    if c.g<lo_v { lo=1u; lo_v=c.g; }
    if c.b<lo_v { lo=2u; lo_v=c.b; }
    var hi_v=c.r; var hi=0u;
    if c.g>=hi_v { hi=1u; hi_v=c.g; }
    if c.b>=hi_v { hi=2u; hi_v=c.b; }
    let mid=3u-lo-hi;
    var out=vec3<f32>(0.0);
    if hi_v>lo_v {
        let mid_v=select(select(c.r,c.g,mid==1u),c.b,mid==2u);
        let out_mid=(mid_v-lo_v)*s/(hi_v-lo_v);
        if mid==0u { out.r=out_mid; } else if mid==1u { out.g=out_mid; } else { out.b=out_mid; }
        if hi==0u { out.r=s; } else if hi==1u { out.g=s; } else { out.b=s; }
    }
    return out;
}
fn blend_rgb(cb:vec3<f32>,cs:vec3<f32>,op:u32)->vec3<f32> {
    // If/else chain instead of switch: see blend_channel.
    if op==20u { return blend_set_lum(blend_set_sat(cs,blend_sat(cb)),blend_lum(cb)); }
    if op==21u { return blend_set_lum(blend_set_sat(cb,blend_sat(cs)),blend_lum(cb)); }
    if op==22u { return blend_set_lum(cs,blend_lum(cb)); }
    if op==23u { return blend_set_lum(cb,blend_lum(cs)); }
    return vec3<f32>(blend_channel(cb.r,cs.r,op),blend_channel(cb.g,cs.g,op),blend_channel(cb.b,cs.b,op));
}
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
            // Write every pixel, including pooled surface pixels outside the path.
            // Bounds are conservative; uncertain inputs keep the legacy loop.
            let origin=vec2<f32>(id.xy)*params.scale.xy;
            if params.boundary.w==1u && (any(origin<params.fill_extra.xy) || any(origin>params.fill_extra.zw)) {
                textureStore(output,position,vec4<f32>(0.0));
                return;
            }
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
        case 5u, 6u, 7u, 8u, 9u, 10u, 11u, 12u, 13u, 14u, 15u, 16u, 17u, 18u, 19u, 20u, 21u, 22u, 23u: {
            let s=textureLoad(source,position,0);
            let d=textureLoad(previous,position,0);
            // Straight operands; zero alpha defines its color as zero.
            var cb=vec3<f32>(0.0);
            var cs=vec3<f32>(0.0);
            if d.a>0.0 { cb=d.rgb/d.a; }
            if s.a>0.0 { cs=s.rgb/s.a; }
            let b=blend_rgb(cb,cs,params.config.x);
            result=vec4<f32>(s.rgb*(1.0-d.a)+d.rgb*(1.0-s.a)+s.a*d.a*b,s.a+d.a*(1.0-s.a));
        }
        case 2u: { result=textureLoad(source,position,0)*params.scale.z; }
        case 3u: {
            let matte=textureLoad(previous,position,0);
            var coverage=matte.a;
            if params.spaces.z==1u || params.spaces.z==3u {
                var weights=vec3<f32>(0.2126,0.7152,0.0722);
                if params.config.w==1u { weights=vec3<f32>(0.2627,0.6780,0.0593); }
                coverage=clamp(dot(matte.rgb,weights),0.0,1.0);
            }
            if params.spaces.z>=2u { coverage=1.0-coverage; }
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
