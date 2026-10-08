struct Params { config:vec4<u32>, offset:vec4<f32>, color:vec4<f32> }
@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var original:texture_2d<f32>;
@group(0) @binding(2) var output:texture_storage_2d<rgba16float,write>;
@group(0) @binding(3) var<uniform> params:Params;
@group(0) @binding(4) var<storage,read> weights:array<f32>;
@group(0) @binding(5) var<storage,read_write> validation:atomic<u32>;
// FX-008 (ADR-0137): second input surface bound by the scene pass. Only the
// displace op (config.x==21u) samples it; every other pass binds the source.
@group(0) @binding(6) var displace_map:texture_2d<f32>;
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
fn load_map(p:vec2<i32>)->vec4<f32> {
    if any(p<vec2<i32>(0)) || any(p>=vec2<i32>(textureDimensions(displace_map))) {return vec4<f32>(0.0);}
    return textureLoad(displace_map,p,0);
}
// FX-008 grain hash (ADR-0137): lowbias32, word-for-word the CPU mix.
fn mix32(h_in:u32)->u32 {
    var h=h_in;
    h^=h>>16u; h*=0x7feb352du; h^=h>>15u; h*=0x846ca68bu; h^=h>>16u;
    return h;
}
fn grain_noise(cell:vec3<i32>,seed:u32)->f32 {
    var h=seed;
    h=mix32(h^u32(cell.x));
    h=mix32(h^u32(cell.y));
    h=mix32(h^u32(cell.z));
    return f32(mix32(h))/4294967296.0;
}
// Displace map channel select: 0-2 stored RGB, 3 alpha, 4 straight luma.
fn map_channel(m:vec4<f32>,luma:f32,ch:u32)->f32 {
    if ch==0u { return m.r; }
    if ch==1u { return m.g; }
    if ch==2u { return m.b; }
    if ch==3u { return m.a; }
    return luma;
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
    }
    // FX-005/FX-006 standard ops (ADR-0115). offset.xyz carries the working
    // luma weights for ops that need luminance/chroma; color.xy carries the
    // precomputed key chroma for chroma keying.
    else if params.config.x==5u {
        // Keying matte extraction: alpha holds the binary matte. config.y
        // selects 0 luma (offset.w key_luma, color.x tolerance) or 1 chroma
        // (offset.w cutoff distance, color.xy key Cb/Cr).
        let v=textureLoad(source,p,0);
        let s=v.rgb/max(v.a,1e-30);
        let w=params.offset.xyz;
        var m=1.0;
        if params.config.y==0u {
            let l=dot(s,w);
            if abs(l-params.offset.w)<params.color.x { m=0.0; }
        } else {
            let y=dot(s,w);
            let c=vec2<f32>((s.b-y)*(0.5/(1.0-w.b)),(s.r-y)*(0.5/(1.0-w.r)));
            if length(c-params.color.xy)<params.offset.w { m=0.0; }
        }
        result=vec4<f32>(0.0,0.0,0.0,m);
    } else if params.config.x==6u {
        // Matte erosion: Chebyshev min filter along axis config.z, lerped
        // between floor(shrink) and floor(shrink)+1.
        let s=params.offset.x;
        if s<=0.0 { result=vec4<f32>(0.0,0.0,0.0,load(p).a); }
        else {
            let inner=i32(floor(s)); let fr=s-f32(inner); let outer=inner+1;
            var mi=1e30; var mo=1e30;
            for (var d=-outer; d<=outer; d++) {
                var q=p+vec2<i32>(d,0); if params.config.z==1u { q=p+vec2<i32>(0,d); }
                let a=load(q).a; mo=min(mo,a);
                if abs(d)<=inner { mi=min(mi,a); }
            }
            result=vec4<f32>(0.0,0.0,0.0,mi*(1.0-fr)+mo*fr);
        }
    } else if params.config.x==7u {
        // Key composite: matte is source.a, pixels are the original input.
        // config.y 0 luma (no despill), 1 chroma (axis despill scaled by
        // offset.w spill).
        let v=textureLoad(original,p,0);
        let m=textureLoad(source,p,0).a;
        let a=v.a*m;
        var s=v.rgb/max(v.a,1e-30);
        if params.config.y==1u {
            let w=params.offset.xyz; let spill=params.offset.w;
            let kc=params.color.xy; let kmag=length(kc);
            if kmag>1e-6 {
                let y=dot(s,w);
                let cb=(s.b-y)*(0.5/(1.0-w.b)); let cr=(s.r-y)*(0.5/(1.0-w.r));
                let axis=kc/kmag;
                let excess=max(dot(vec2<f32>(cb,cr),axis)-kmag,0.0)*spill;
                let cbn=cb-axis.x*excess; let crn=cr-axis.y*excess;
                let db=(cbn-cb)*2.0*(1.0-w.b); let dr=(crn-cr)*2.0*(1.0-w.r);
                s=vec3<f32>(s.r+dr,s.g-(w.r*dr+w.b*db)/w.g,s.b+db);
            }
        }
        result=vec4<f32>(s*a,a);
    } else if params.config.x==8u {
        // Glow extraction: keep the premultiplied pixel when its straight
        // luminance exceeds offset.w threshold.
        let v=textureLoad(source,p,0);
        let s=v.rgb/max(v.a,1e-30);
        if dot(s,params.offset.xyz)>params.offset.w { result=v; }
    } else if params.config.x==9u {
        // Glow additive composite: rgb adds, alpha source-overs the bloom
        // coverage so the halation lights transparent surroundings.
        let b=textureLoad(source,p,0); let o=textureLoad(original,p,0);
        let i=params.offset.x;
        let rgb=clamp(o.rgb+b.rgb*i,vec3<f32>(-65504.0),vec3<f32>(65504.0));
        let cov=min(b.a*i,1.0);
        result=vec4<f32>(rgb,clamp(o.a+cov*(1.0-o.a),0.0,1.0));
    } else if params.config.x==10u {
        // Unsharp mask: original + amount*(original-blur) on all channels;
        // alpha stays in [0,1], rgb clamps to the RGBA16F range.
        let b=textureLoad(source,p,0); let o=textureLoad(original,p,0);
        let amt=params.offset.x;
        let rgb=clamp(o.rgb+amt*(o.rgb-b.rgb),vec3<f32>(-65504.0),vec3<f32>(65504.0));
        result=vec4<f32>(rgb,clamp(o.a+amt*(o.a-b.a),0.0,1.0));
    } else if params.config.x==11u {
        // Vignette: smoothstep corner falloff multiplies premultiplied rgb;
        // alpha is preserved. offset = amount, midpoint, feather, roundness.
        let v=textureLoad(source,p,0);
        let dims=vec2<f32>(textureDimensions(output));
        let n=(vec2<f32>(p)+0.5)/dims*2.0-1.0;
        let rect=max(abs(n.x),abs(n.y));
        let el=length(n)/sqrt(2.0);
        let d=mix(rect,el,params.offset.w);
        let t=clamp((d-params.offset.y)/max(params.offset.z,1e-6),0.0,1.0);
        let s=t*t*(3.0-2.0*t);
        result=vec4<f32>(v.rgb*(1.0-params.offset.x*s),v.a);
    } else if params.config.x==12u {
        // Corner pin inverse warp: weights[0..9] is the row-major 3x3 mapping
        // dest edge coordinates to source edge coordinates; weights[9..13]
        // are the source rect min/max for the coverage clip.
        if params.config.w==0u {
            let f=vec2<f32>(p)+0.5;
            let h=vec3<f32>(
                weights[0]*f.x+weights[1]*f.y+weights[2],
                weights[3]*f.x+weights[4]*f.y+weights[5],
                weights[6]*f.x+weights[7]*f.y+weights[8]);
            let e=h.xy/h.z;
            if h.z>0.0 && e.x>=weights[9] && e.x<=weights[11] && e.y>=weights[10] && e.y<=weights[12] {
                result=bilinear(e-vec2<f32>(0.5));
            }
        }
    } else if params.config.x==13u {
        // TRACK-002 (ADR-0122) stabilize inverse warp. weights[0..6] is the
        // row-major 2x3 `frame` map (output texel center -> corrected source
        // extent position); weights[6..12] is the 2x3 `unmap` map (resolved
        // source position -> input raster position); weights[12..13] is the
        // source extent. config.y selects the border policy (0 fill, 1
        // replicate, 2 reflect); config.z selects sampling (0 nearest,
        // 1 bilinear); offset is the premultiplied working-space fill.
        let f=vec2<f32>(p)+0.5;
        var s=vec2<f32>(
            weights[0]*f.x+weights[1]*f.y+weights[2],
            weights[3]*f.x+weights[4]*f.y+weights[5]);
        let sz=vec2<f32>(weights[12],weights[13]);
        let inside=all(s>=vec2<f32>(0.0)) && all(s<=sz);
        if params.config.y==0u && !inside {
            result=params.offset;
        } else {
            if params.config.y==1u { s=clamp(s,vec2<f32>(0.0),sz); }
            else if params.config.y==2u {
                let m=s-2.0*sz*floor(s/(2.0*sz));
                s=select(m,2.0*sz-m,m>sz);
            }
            let r=vec2<f32>(
                weights[6]*s.x+weights[7]*s.y+weights[8],
                weights[9]*s.x+weights[10]*s.y+weights[11]);
            if params.config.z==0u {
                result=load(vec2<i32>(floor(r)));
            } else {
                result=bilinear(r-vec2<f32>(0.5));
            }
        }
    }
    // FX-008 remaining standard effects (ADR-0137). Ops 14-22 mirror the
    // CPU oracle expression-for-expression; surfaces stay RGBA16F.
    else if params.config.x==14u {
        // Grain: deterministic cell hash on the output lattice; straight
        // working RGB gains amount*(noise-0.5); alpha is preserved.
        // offset = (amount, cell size, seed bits); config.y selects monochrome.
        let v=textureLoad(source,p,0);
        let s=v.rgb/max(v.a,1e-30);
        let cell=vec3<i32>(
            i32(clamp(floor((f32(p.x)+0.5)/params.offset.y),-2147000000.0,2147000000.0)),
            i32(clamp(floor((f32(p.y)+0.5)/params.offset.y),-2147000000.0,2147000000.0)),
            0);
        let seed=bitcast<u32>(params.offset.z);
        var rgb=vec3<f32>(0.0);
        if params.config.y==1u {
            let n=grain_noise(cell,seed);
            rgb=s+vec3<f32>(params.offset.x*(n-0.5));
        } else {
            for (var c=0;c<3;c++) {
                rgb[c]=s[c]+params.offset.x*(grain_noise(cell+vec3<i32>(0,0,c),seed)-0.5);
            }
        }
        result=vec4<f32>(rgb*v.a,v.a);
    } else if params.config.x==15u {
        // Mosaic: block basis 0 samples the block-center texel, 1 the
        // top-left edge texel; lattice indices clamp before i32 conversion.
        let bs=params.offset.x;
        let b=vec2<f32>(floor(f32(p.x)/bs),floor(f32(p.y)/bs));
        var t=vec2<f32>(0.0);
        if params.config.y==0u { t=b*bs+vec2<f32>(bs*0.5); }
        else { t=b*bs; }
        result=load(vec2<i32>(
            i32(clamp(floor(t.x),-2147000000.0,2147000000.0)),
            i32(clamp(floor(t.y),-2147000000.0,2147000000.0))));
    } else if params.config.x==16u {
        // Invert on straight working channels; config.y 0 rgb, 1-3 single
        // channel, 4 alpha (straight RGB is kept and re-associated).
        let v=textureLoad(source,p,0);
        let s=v.rgb/max(v.a,1e-30);
        if params.config.y==4u {
            result=vec4<f32>(s*(1.0-v.a),1.0-v.a);
        } else {
            var o=s;
            if params.config.y==0u { o=vec3<f32>(1.0)-s; }
            else { o[params.config.y-1u]=1.0-s[params.config.y-1u]; }
            result=vec4<f32>(o*v.a,v.a);
        }
    } else if params.config.x==17u {
        // Channel mixer: the premultiplied vec4 passes through the row-major
        // 4x4 matrix held in weights[0..16].
        let v=textureLoad(source,p,0);
        var o=vec4<f32>(0.0);
        for (var r=0u;r<4u;r++) {
            o[r]=weights[4u*r]*v.x+weights[4u*r+1u]*v.y+weights[4u*r+2u]*v.z+weights[4u*r+3u]*v.w;
        }
        result=o;
    } else if params.config.x==18u {
        // Tint: straight working luma maps between the authored black/white
        // colors, blended by amount; alpha is preserved.
        // offset = (amount, luma w.xyz); color = map_black; weights = map_white.
        let v=textureLoad(source,p,0);
        let s=v.rgb/max(v.a,1e-30);
        let l=dot(s,params.offset.yzw);
        let mw=vec3<f32>(weights[0],weights[1],weights[2]);
        let mapped=params.color.rgb+l*(mw-params.color.rgb);
        let o=s+(mapped-s)*params.offset.x;
        result=vec4<f32>(o*v.a,v.a);
    } else if params.config.x==19u {
        // Directional blur: ceil(length) midpoint taps along the normalized
        // direction; offset = (dir.x, dir.y, length). A zero length samples
        // the texel itself and is the identity.
        let dir=params.offset.xy; let len=params.offset.z;
        let taps=max(1u,u32(ceil(len)));
        var acc=vec4<f32>(0.0);
        for (var i=0u;i<taps;i++) {
            let t=-len*0.5+(f32(i)+0.5)*len/f32(taps);
            acc+=bilinear(vec2<f32>(p)+dir*t);
        }
        result=acc/f32(taps);
    } else if params.config.x==20u {
        // Radial blur: 64 midpoint taps; mode 0 spins degrees around the
        // center over [-amount/2, amount/2], mode 1 scales toward the center
        // over (1-amount, 1]. offset = (amount, center.x, center.y).
        let amount=params.offset.x; let center=params.offset.yz;
        let rel=vec2<f32>(p)+vec2<f32>(0.5)-center;
        var acc=vec4<f32>(0.0);
        for (var i=0u;i<64u;i++) {
            let t=(f32(i)+0.5)/64.0;
            var pos=center;
            if params.config.y==0u {
                let angle=(-amount*0.5+amount*t)*0.017453292519943295;
                let cs=cos(angle); let sn=sin(angle);
                pos=center+vec2<f32>(rel.x*cs-rel.y*sn,rel.x*sn+rel.y*cs);
            } else {
                pos=center+rel*((1.0-amount)+amount*t);
            }
            acc+=bilinear(pos-vec2<f32>(0.5));
        }
        result=acc/64.0;
    } else if params.config.x==21u {
        // Displace: the map surface (binding 6) at the output texel supplies
        // channel values v in [0,1]; (2v-1) passes through the 2x2
        // displacement matrix in weights[0..4] (row-major) to offset the
        // bilinear source sample. weights[4..7] hold the luma weights.
        let m=load_map(p);
        let luma=dot(m.rgb/max(m.a,1e-30),vec3<f32>(weights[4],weights[5],weights[6]));
        let vx=map_channel(m,luma,params.config.y);
        let vy=map_channel(m,luma,params.config.z);
        let off=vec2<f32>(
            (2.0*vx-1.0)*weights[0]+(2.0*vy-1.0)*weights[1],
            (2.0*vx-1.0)*weights[2]+(2.0*vy-1.0)*weights[3]);
        result=bilinear(vec2<f32>(p)+off);
    } else if params.config.x==22u {
        // Generate: procedural coverage of the output surface; colors arrive
        // as straight working RGBA. offset = (point_a, point_b); color =
        // color_a; weights = color_b, cell size, line width.
        let f=vec2<f32>(p)+vec2<f32>(0.5);
        let pa=params.offset.xy; let pb=params.offset.zw;
        let ca=params.color;
        let cb=vec4<f32>(weights[0],weights[1],weights[2],weights[3]);
        let cell=weights[4]; let lw=weights[5];
        var t=0.0;
        if params.config.y==0u {
            let d=pb-pa; let dd=dot(d,d);
            if dd>0.0 { t=clamp(dot(f-pa,d)/dd,0.0,1.0); }
        } else if params.config.y==1u {
            let r=length(pb-pa);
            if r>0.0 { t=clamp(length(f-pa)/r,0.0,1.0); } else { t=1.0; }
        } else if params.config.y==2u {
            let cx=i32(clamp(floor(f.x/cell),-2147000000.0,2147000000.0));
            let cy=i32(clamp(floor(f.y/cell),-2147000000.0,2147000000.0));
            if ((cx+cy)&1)==1 { t=1.0; }
        } else {
            let dx=abs(f.x-round(f.x/cell)*cell);
            let dy=abs(f.y-round(f.y/cell)*cell);
            if dx*2.0<=lw || dy*2.0<=lw { t=1.0; }
        }
        let straight=ca+(cb-ca)*t;
        result=vec4<f32>(straight.rgb*straight.a,straight.a);
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
