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
// COLOR-002/COLOR-003 pointwise corrections (ADR-0108/ADR-0113).
// config.x==4u selects the pass, config.y the op (1 exposure, 2 levels,
// 3 curves, 4 HSL, 5 LUT), config.z the curve point count or LUT edge size.
// Alpha is preserved; HDR and negative values are never clamped for ops 1-4.
// Op 5 samples straight working RGB through tetrahedral interpolation with
// domain normalization and endpoint clamping, mirroring CubeLut::sample.
fn color_levels(v:f32)->f32 {
    let n=(v-params.offset.x)/(params.offset.y-params.offset.x);
    var g=n;
    if n<0.0 { g=-pow(-n,1.0/params.offset.z); } else { g=pow(n,1.0/params.offset.z); }
    return params.offset.w+g*(params.color.x-params.offset.w);
}
fn curve_eval(x:f32)->f32 {
    let n=params.config.z;
    var xs:array<f32,64>; var ys:array<f32,64>; var d:array<f32,64>;
    var h:array<f32,63>; var s:array<f32,63>;
    for (var i=0u;i<n;i++) { xs[i]=weights[2u*i]; ys[i]=weights[2u*i+1u]; }
    for (var i=0u;i+1u<n;i++) { h[i]=xs[i+1u]-xs[i]; s[i]=(ys[i+1u]-ys[i])/h[i]; }
    if n==2u {
        d[0]=s[0]; d[1]=s[0];
    } else {
        // Fritsch-Carlson endpoints; mirrors color::monotone_cubic_tangents.
        var de=((2.0*h[0]+h[1])*s[0]-h[0]*s[1])/(h[0]+h[1]);
        if sign(de)!=sign(s[0]) { de=0.0; }
        else if sign(s[0])!=sign(s[1]) && abs(de)>3.0*abs(s[0]) { de=3.0*s[0]; }
        d[0]=de;
        de=((2.0*h[n-2u]+h[n-3u])*s[n-2u]-h[n-2u]*s[n-3u])/(h[n-2u]+h[n-3u]);
        if sign(de)!=sign(s[n-2u]) { de=0.0; }
        else if sign(s[n-2u])!=sign(s[n-3u]) && abs(de)>3.0*abs(s[n-2u]) { de=3.0*s[n-2u]; }
        d[n-1u]=de;
        for (var i=1u;i+1u<n;i++) {
            if s[i-1u]*s[i]<=0.0 { d[i]=0.0; }
            else {
                let w1=2.0*h[i]+h[i-1u]; let w2=h[i]+2.0*h[i-1u];
                d[i]=(w1+w2)/(w1/s[i-1u]+w2/s[i]);
            }
        }
    }
    if x<=xs[0] { return ys[0]+d[0]*(x-xs[0]); }
    if x>=xs[n-1u] { return ys[n-1u]+d[n-1u]*(x-xs[n-1u]); }
    // Segment i satisfies xs[i] <= x < xs[i+1].
    var i=0u;
    for (var k=1u;k+1u<n;k++) { if x>=xs[k] { i=k; } }
    let t=(x-xs[i])/h[i];
    let t2=t*t; let t3=t2*t;
    let a=2.0*t3-3.0*t2+1.0; let b=t3-2.0*t2+t; let c=-2.0*t3+3.0*t2; let e=t3-t2;
    return a*ys[i]+b*h[i]*d[i]+c*ys[i+1u]+e*h[i]*d[i+1u];
}
fn rgb_to_hsl(rgb:vec3<f32>)->vec3<f32> {
    let lo=min(rgb.r,min(rgb.g,rgb.b)); let hi=max(rgb.r,max(rgb.g,rgb.b));
    let l=0.5*(lo+hi);
    if hi==lo { return vec3<f32>(0.0,0.0,l); }
    let delta=hi-lo;
    let denom=1.0-abs(2.0*l-1.0);
    var s=0.0; if denom!=0.0 { s=delta/denom; }
    var h=0.0;
    if hi==rgb.r { h=(rgb.g-rgb.b)/delta; }
    else if hi==rgb.g { h=(rgb.b-rgb.r)/delta+2.0; }
    else { h=(rgb.r-rgb.g)/delta+4.0; }
    return vec3<f32>((h-6.0*floor(h/6.0))*60.0,s,l);
}
fn hsl_to_rgb(hsl:vec3<f32>)->vec3<f32> {
    let c=hsl.y*(1.0-abs(2.0*hsl.z-1.0));
    let m=hsl.z-0.5*c;
    if c==0.0 { return vec3<f32>(hsl.z); }
    let h=(hsl.x/60.0)-6.0*floor(hsl.x/360.0);
    let x=c*(1.0-abs(h-2.0*floor(h/2.0)-1.0));
    var rgb:vec3<f32>;
    if h<1.0 { rgb=vec3<f32>(c,x,0.0); }
    else if h<2.0 { rgb=vec3<f32>(x,c,0.0); }
    else if h<3.0 { rgb=vec3<f32>(0.0,c,x); }
    else if h<4.0 { rgb=vec3<f32>(0.0,x,c); }
    else if h<5.0 { rgb=vec3<f32>(x,0.0,c); }
    else { rgb=vec3<f32>(c,0.0,x); }
    return rgb+vec3<f32>(m);
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=textureDimensions(output)) {return;}
    let p=vec2<i32>(id.xy); var result=vec4<f32>(0.0);
    if params.config.x==4u {
        let v=textureLoad(source,p,0);
        var rgb=v.rgb;
        let op=params.config.y;
        if op==1u { rgb=v.rgb*exp2(params.offset.x)+vec3<f32>(params.offset.y); }
        else if op==2u {
            rgb=vec3<f32>(color_levels(v.r),color_levels(v.g),color_levels(v.b));
        }
        else if op==3u {
            rgb=vec3<f32>(curve_eval(v.r),curve_eval(v.g),curve_eval(v.b));
        }
        else if op==4u {
            let hsl=rgb_to_hsl(v.rgb);
            rgb=hsl_to_rgb(vec3<f32>(hsl.x+params.offset.x,hsl.y*params.offset.y,hsl.z+params.offset.z));
        }
        else if op==5u {
            let n=params.config.z;
            var straight=vec3<f32>(0.0);
            if v.a>0.0000152587890625 { straight=v.rgb/v.a; }
            let pos=clamp((straight-params.offset.rgb)/(params.color.rgb-params.offset.rgb),vec3<f32>(0.0),vec3<f32>(1.0))*f32(n-1u);
            let base=min(vec3<u32>(floor(pos)),vec3<u32>(n-2u));
            let f=pos-vec3<f32>(base);
            // .cube lattice index, red fastest: ((b*n+g)*n+r)*3.
            let base_index=((base.z*n+base.y)*n+base.x)*3u;
            let stride_g=3u*n;
            let stride_b=3u*n*n;
            var off0=0u; var off1=0u; var off2=0u; var off3=0u;
            var w=vec4<f32>(0.0);
            // Tetrahedral branch order mirrors CubeLut::sample exactly so CPU
            // and GPU select the same sub-tetrahedron on boundary ties.
            if f.x>=f.y {
                if f.y>=f.z {
                    off1=3u; off2=3u+stride_g; off3=3u+stride_g+stride_b;
                    w=vec4<f32>(1.0-f.x,f.x-f.y,f.y-f.z,f.z);
                } else if f.x>=f.z {
                    off1=3u; off2=3u+stride_b; off3=3u+stride_g+stride_b;
                    w=vec4<f32>(1.0-f.x,f.x-f.z,f.z-f.y,f.y);
                } else {
                    off1=stride_b; off2=stride_b+3u; off3=stride_b+3u+stride_g;
                    w=vec4<f32>(1.0-f.z,f.z-f.x,f.x-f.y,f.y);
                }
            } else if f.z>=f.y {
                off1=stride_b; off2=stride_b+stride_g; off3=stride_b+stride_g+3u;
                w=vec4<f32>(1.0-f.z,f.z-f.y,f.y-f.x,f.x);
            } else if f.x>=f.z {
                off1=stride_g; off2=stride_g+3u; off3=stride_g+3u+stride_b;
                w=vec4<f32>(1.0-f.y,f.y-f.x,f.x-f.z,f.z);
            } else {
                off1=stride_g; off2=stride_g+stride_b; off3=stride_g+stride_b+3u;
                w=vec4<f32>(1.0-f.y,f.y-f.z,f.z-f.x,f.x);
            }
            let c0=vec3<f32>(weights[base_index+off0],weights[base_index+off0+1u],weights[base_index+off0+2u]);
            let c1=vec3<f32>(weights[base_index+off1],weights[base_index+off1+1u],weights[base_index+off1+2u]);
            let c2=vec3<f32>(weights[base_index+off2],weights[base_index+off2+1u],weights[base_index+off2+2u]);
            let c3=vec3<f32>(weights[base_index+off3],weights[base_index+off3+1u],weights[base_index+off3+2u]);
            let mapped=c0*w.x+c1*w.y+c2*w.z+c3*w.w;
            // straight + (mapped - straight) * intensity mirrors the CPU
            // arithmetic exactly; mix() would evaluate x*(1-a)+y*a instead.
            rgb=(straight+(mapped-straight)*params.offset.w)*v.a;
        }
        result=vec4<f32>(rgb,v.a);
    }
    else if params.config.x==0u {
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
