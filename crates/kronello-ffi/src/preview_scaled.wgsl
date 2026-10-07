@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var<uniform> transform: vec4<f32>;
@group(0) @binding(2) var image_sampler: sampler;
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(positions[index],0.0,1.0);
}
@fragment fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // transform.xy scales surface pixels into rendered-region pixels and
    // transform.zw is the rendered-buffer crop offset in texture pixels.
    let dims = vec2<f32>(textureDimensions(image));
    let uv = (position.xy * transform.xy + transform.zw + vec2<f32>(0.5)) / dims;
    let pixel = textureSampleLevel(image, image_sampler, uv, 0.0);
    // Linear premultiplied Rec.709 over black, followed by SDR sRGB encoding.
    let linear = clamp(pixel.rgb, vec3(0.0), vec3(1.0));
    let encoded = select(1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055,
                         12.92 * linear, linear <= vec3(0.0031308));
    return vec4(encoded, 1.0);
}
