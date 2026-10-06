@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var<uniform> crop: vec4<u32>;
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(positions[index],0.0,1.0);
}
@fragment fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = textureLoad(image, vec2<i32>(position.xy) + vec2<i32>(crop.xy), 0);
    // Linear premultiplied Rec.709 over black, followed by SDR sRGB encoding.
    let linear = clamp(pixel.rgb, vec3(0.0), vec3(1.0));
    let encoded = select(1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055,
                         12.92 * linear, linear <= vec3(0.0031308));
    return vec4(encoded, 1.0);
}
