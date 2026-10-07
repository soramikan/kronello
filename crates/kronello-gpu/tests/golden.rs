//! Explicit Apple Silicon + Metal comparison. UPDATE produces candidates only.
mod common;
use kronello_gpu::{color::srgb_encode, *};
use kronello_testkit::{FrameDescriptor, LinearFrame, PixelTolerance, compare_pixels};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn command(program: &str, args: &[&str], root: &Path) -> Result<String> {
    let result = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "{program} {args:?} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(result.stdout)?.trim_end().to_owned())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write_json(path: impl AsRef<Path>, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
/// The scene manifest is the largest golden artifact; keeping it compact
/// preserves the ADR-0088 per-file size cap without changing its content
/// (baseline validation compares parsed values, not bytes layout).
fn write_json_compact(path: impl AsRef<Path>, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec(value)?)?;
    Ok(())
}
struct Scene {
    id: &'static str,
    width: u32,
    height: u32,
    space: WorkingSpace,
    layers: Vec<Layer>,
    draw: Option<DrawScene>,
    output: Option<OutputTransform>,
}
fn scenes(pam: Image) -> Vec<Scene> {
    let mut rotated = Layer::rectangle([2.0, 3.0], [0.5, 0.25, 0.8, 0.5], InputSpace::Srgb);
    rotated.translation = [5.0, 1.0];
    rotated.rotation_degrees = 90.0;
    let hdr = vec![Layer::rectangle(
        [1.0, 1.0],
        [4.0, 2.0, -0.125, 1.0],
        InputSpace::LinearRec2020,
    )];
    let mut scenes = vec![
        Scene {
            draw: None,
            output: None,
            id: "source-over",
            width: 8,
            height: 8,
            space: WorkingSpace::LinearRec709,
            layers: vec![
                Layer::rectangle([8.0, 8.0], [0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
                Layer::rectangle([8.0, 8.0], [1.0, 0.0, 0.0, 0.5], InputSpace::LinearRec709),
            ],
        },
        Scene {
            draw: None,
            output: None,
            id: "srgb-pam",
            width: pam.width,
            height: pam.height,
            space: WorkingSpace::LinearRec709,
            layers: vec![Layer {
                size: [pam.width as f32, pam.height as f32],
                image: pam,
                translation: [0.0; 2],
                rotation_degrees: 0.0,
            }],
        },
        Scene {
            draw: None,
            output: None,
            id: "translated-rotation",
            width: 8,
            height: 8,
            space: WorkingSpace::LinearRec709,
            layers: vec![rotated],
        },
        Scene {
            draw: None,
            output: None,
            id: "rec2020-conversion",
            width: 4,
            height: 4,
            space: WorkingSpace::LinearRec2020,
            layers: vec![Layer::rectangle(
                [4.0, 4.0],
                [0.75, 0.25, 0.5, 0.5],
                InputSpace::Srgb,
            )],
        },
        Scene {
            draw: None,
            output: None,
            id: "hdr-no-clamp",
            width: 1,
            height: 1,
            space: WorkingSpace::LinearRec2020,
            layers: hdr,
        },
        Scene {
            draw: None,
            output: None,
            id: "alpha-boundary",
            width: 1,
            height: 1,
            space: WorkingSpace::LinearRec709,
            layers: vec![Layer::rectangle(
                [1.0, 1.0],
                [1.0, 0.0, 0.0, color::ALPHA_EPSILON],
                InputSpace::LinearRec709,
            )],
        },
    ];
    scenes.extend(
        common::scenes()
            .into_iter()
            .map(|(id, n, space, draw)| Scene {
                id,
                width: n,
                height: n,
                space,
                layers: vec![],
                draw: Some(draw),
                output: None,
            }),
    );
    scenes.push(Scene {
        id: "srgb-output-roundtrip",
        width: 8,
        height: 8,
        space: WorkingSpace::LinearRec2020,
        layers: vec![],
        draw: Some(common::edges()),
        output: Some(OutputTransform {
            space: InputSpace::Srgb,
            alpha: OutputAlpha::Straight,
        }),
    });
    scenes
}
fn descriptor(scene: &Scene) -> FrameDescriptor {
    FrameDescriptor {
        width: scene.width,
        height: scene.height,
        origin: [0; 2],
        time: [0, 1],
        working_space: if scene.space == WorkingSpace::LinearRec709 {
            kronello_testkit::WorkingSpace::LinearRec709
        } else {
            kronello_testkit::WorkingSpace::LinearRec2020
        },
        color_pipeline_id: if scene.draw.is_some() {
            "vec003-grid4-v2"
        } else {
            "gpu001-linear-v1"
        }
        .into(),
        samples_per_frame: if scene.draw.is_some() { 16 } else { 1 },
        seed: 0,
    }
}
fn gradient_manifest(g: &GradientPaint) -> Value {
    let geometry = match g.geometry {
        GradientGeometry::Linear { start, end } => json!({"kind":"linear","start":start,"end":end}),
        GradientGeometry::Radial { center, radius } => {
            json!({"kind":"radial","center":center,"radius":radius})
        }
        GradientGeometry::FocalRadial {
            center,
            radius,
            focal,
            focal_radius,
        } => {
            json!({"kind":"focal_radial","center":center,"radius":radius,"focal":focal,"focal_radius":focal_radius})
        }
        GradientGeometry::Conic {
            center,
            start_angle,
            sweep_angle,
        } => {
            json!({"kind":"conic","center":center,"start_angle":start_angle,"sweep_angle":sweep_angle})
        }
    };
    json!({"geometry":geometry,"spread":format!("{:?}",g.spread),"interpolation":format!("{:?}",g.interpolation),"interpolation_version":g.interpolation_version,"transform":g.transform,"equal_offsets":"last wins at offset","stops":g.stops.iter().map(|s|json!({"offset":s.offset,"rgba":s.paint.rgba,"space":format!("{:?}",s.paint.space)})).collect::<Vec<_>>()})
}
fn draw_manifest(scene: &DrawScene) -> Value {
    json!({"roots":scene.roots,"nodes":scene.nodes.iter().map(|node| match node {
        DrawNode::GpuRaster(_) => panic!("golden fixtures must use explicit serializable image inputs"),
        DrawNode::Raster(pixels)=>json!({"kind":"raster","pixels":pixels}),
        DrawNode::Path(p)=>json!({"kind":"path","stroke_geometry_version":p.stroke_geometry.as_ref().map_or(kronello_model::LEGACY_STROKE_VERSION,|g|g.version.as_str()),"local_stroke":p.stroke_geometry.as_ref().map(|g|json!({"version":g.version,"alignment":g.alignment,"fill_rule":format!("{:?}",g.fill_rule),"inverse":g.output_to_local,"dash_array":g.dash_array,"dash_offset":g.dash_offset,"contours":g.contours.iter().map(|c|json!({"points":c.points,"closed":c.closed})).collect::<Vec<_>>()})),"fill_gradient":p.fill_gradient.as_deref().map(gradient_manifest),"stroke_gradient":p.stroke_gradient.as_deref().map(gradient_manifest),"paint_transform":p.paint_transform,"contours":p.contours.iter().map(|c| json!({"points":c.points,"closed":c.closed})).collect::<Vec<_>>(),"fill":p.fill.map(|f| json!({"rgba":f.paint.rgba,"space":format!("{:?}",f.paint.space),"rule":format!("{:?}",f.rule)})),"stroke":p.stroke.map(|s| json!({"rgba":s.paint.rgba,"space":format!("{:?}",s.paint.space),"width":s.width,"cap":format!("{:?}",s.cap),"join":format!("{:?}",s.join),"miter_limit":s.miter_limit}))}),
        DrawNode::Blend {source,backdrop,mode}=>json!({"kind":"blend","source":source,"backdrop":backdrop,"mode":mode}),
        DrawNode::Group {children,opacity}=>json!({"kind":"isolated-group","children":children,"opacity":opacity}),
        DrawNode::Effect {source,effect}=>json!({"kind":"effect","source":source,"effect":effect,"kernel_version":effect.kernel_version(),"semantic_version":effect.semantic_version()}),
        DrawNode::Masked {source,matte,kind}=>json!({"kind":"masked","source":source,"matte":matte,"mode":format!("{:?}",kind)})
    }).collect::<Vec<_>>()})
}
fn manifest(scenes: &[Scene], fixture_hash: &str, font_hash: &str) -> Value {
    json!({"schema_version":3,"affine_effect_kernel_version":kronello_render::AFFINE_EFFECT_KERNEL_VERSION,"affine_effect_semantic_version":kronello_model::AFFINE_EFFECT_VERSION,"effect_kernel_version":EFFECT_KERNEL_VERSION,"effect_semantic_versions":{kronello_model::GAUSSIAN_BLUR_ID:kronello_model::EFFECT_VERSION,kronello_model::DROP_SHADOW_ID:kronello_model::EFFECT_VERSION,kronello_model::KEYING_CHROMA_ID:kronello_model::STANDARD_EFFECT_VERSION,kronello_model::KEYING_LUMA_ID:kronello_model::STANDARD_EFFECT_VERSION,kronello_model::GLOW_ID:kronello_model::STANDARD_EFFECT_VERSION,kronello_model::SHARPEN_ID:kronello_model::STANDARD_EFFECT_VERSION,kronello_model::VIGNETTE_ID:kronello_model::STANDARD_EFFECT_VERSION,kronello_model::CORNER_PIN_ID:kronello_model::STANDARD_EFFECT_VERSION},"stroke_geometry_version":kronello_render::STROKE_GEOMETRY_VERSION,"gradient_interpolation_version":kronello_render::GRADIENT_INTERPOLATION_VERSION,"comparison_version":1,"rgb_absolute":1.0/1024.0,"rgb_relative":1.0/1024.0,"alpha_absolute":1.0/1024.0,"fixture_hash":fixture_hash,"font_hash":font_hash,"flatten_tolerance_px":0.02,"catalog":serde_json::from_str::<Value>(include_str!("../../../tests/golden/apple-silicon-metal/scenes.json")).unwrap(),"scenes":scenes.iter().map(|s| json!({"id":s.id,"sample_id":"frame-0","size":[s.width,s.height],"design_extent":[s.width,s.height],"origin":[0,0],"time":{"num":"0","den":"1"},"working_space":format!("{:?}",s.space),"alpha":"premultiplied","output_transform":s.output.map(|o| format!("{:?}",o)),"comparison_space":"linear working-space premultiplied; external output decoded back before comparison","display_transform":"external-unpremultiply-then-srgb-clamp; visualization only","color_pipeline_id":if s.draw.is_some() {"vec003-grid4-v2"} else {"gpu001-linear-v1"},"samples_per_frame":if s.draw.is_some() {16} else {1},"seed":0,"draw":s.draw.as_ref().map(draw_manifest),"layers":s.layers.iter().map(|l| json!({"size":l.size,"translation":l.translation,"rotation_degrees":l.rotation_degrees,"input_space":format!("{:?}",l.image.space),"input_size":[l.image.width,l.image.height],"straight_pixels":l.image.pixels})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}

// Hardware model, OS, driver and dependency versions are provenance only.
fn eligible_environment(target: &str, backend: &str) -> bool {
    (target == "aarch64-apple-darwin" && backend == "Metal")
        || (target.ends_with("-linux-gnu") && backend == "Vulkan")
        || (target.ends_with("-windows-msvc") && backend == "Dx12")
}

fn artifact_manifest(
    dir: &Path,
    settings: &Value,
    environment: &Value,
    provenance: &Value,
) -> Result<Value> {
    let scenes = settings["scenes"].as_array().ok_or("missing scenes")?;
    if scenes.is_empty() {
        return Err("zero scenes".into());
    }
    let mut names = vec![
        "manifest.json".to_owned(),
        "environment.json".to_owned(),
        "provenance.json".to_owned(),
    ];
    let mut scene_settings = Vec::new();
    for scene in scenes {
        let id = scene["id"].as_str().ok_or("missing scene id")?;
        // IDs come from the compiled scene catalog, never from arbitrary paths.
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("invalid scene id".into());
        }
        names.push(format!("{id}/frame-0.rgba16f"));
        names.push(format!("{id}/frame-0.png"));
        if !scene["output_transform"].is_null() {
            names.push(format!("{id}/external-srgb-straight.rgba16f"));
        }
        scene_settings.push(json!({"id":id,"size":scene["size"],"sample_id":scene["sample_id"],"samples_per_frame":scene["samples_per_frame"],"time":scene["time"],"working_space":scene["working_space"],"alpha":scene["alpha"]}));
    }
    let mut files = serde_json::Map::new();
    for name in names {
        let bytes = fs::read(dir.join(&name))?;
        files.insert(name, json!({"sha256":hash(&bytes),"bytes":bytes.len()}));
    }
    Ok(
        json!({"schema_version":1,"comparison_version":settings["comparison_version"],"rgb_absolute":settings["rgb_absolute"],"rgb_relative":settings["rgb_relative"],"alpha_absolute":settings["alpha_absolute"],"scene_settings":scene_settings,"environment":environment,"provenance":provenance,"files":files}),
    )
}

fn validate_baseline(dir: &Path, settings: &Value) -> Result<()> {
    let old: Value = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?;
    if old != *settings {
        return Err("scene manifest mismatch; review inputs and comparison version".into());
    }
    let environment: Value = serde_json::from_slice(&fs::read(dir.join("environment.json"))?)?;
    let provenance: Value = serde_json::from_slice(&fs::read(dir.join("provenance.json"))?)?;
    let adoption: Value = serde_json::from_slice(&fs::read(dir.join("adoption.json"))?)?;
    if adoption != artifact_manifest(dir, settings, &environment, &provenance)? {
        return Err("baseline artifact hashes or adoption manifest mismatch".into());
    }
    Ok(())
}

fn png(path: &Path, scene: &Scene, pixels: &[[f32; 4]]) -> Result<()> {
    // Display aid only: convert straight RGB to Rec.709 and encode sRGB. Negative
    // and HDR values remain untouched in the .rgba16f comparison artifact.
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|&p| {
            let p = color::unpremultiply_external(p);
            let rgb = color::convert_primaries(
                [p[0], p[1], p[2]],
                scene.space,
                WorkingSpace::LinearRec709,
            );
            [
                srgb_encode(rgb[0]).clamp(0.0, 1.0),
                srgb_encode(rgb[1]).clamp(0.0, 1.0),
                srgb_encode(rgb[2]).clamp(0.0, 1.0),
                p[3],
            ]
            .map(|v| (v * 255.0).round() as u8)
        })
        .collect();
    let mut encoder = png::Encoder::new(fs::File::create(path)?, scene.width, scene.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(())
}
fn run(output: &Path) -> Result<Value> {
    if std::env::var("KRONELLO_GOLDEN").as_deref() != Ok("1") {
        return Err("KRONELLO_GOLDEN=1 required".into());
    }
    let (target, profile, required_backend) =
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            ("aarch64-apple-darwin", "apple-silicon-metal", "metal")
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            ("x86_64-unknown-linux-gnu", "linux-vulkan", "vulkan")
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            ("x86_64-pc-windows-msvc", "windows-dx12", "dx12")
        } else {
            return Err("unsupported golden platform".into());
        };
    if std::env::var("WGPU_BACKEND").as_deref() != Ok(required_backend) {
        return Err("explicit platform WGPU_BACKEND required".into());
    }
    let update = match std::env::var("KRONELLO_GOLDEN_UPDATE") {
        Ok(v) if v == "1" => true,
        Err(std::env::VarError::NotPresent) => false,
        _ => return Err("KRONELLO_GOLDEN_UPDATE must be unset or 1".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let baseline = root.join("tests/golden").join(profile);
    let gpu = GpuContext::new()?;
    if !eligible_environment(target, &format!("{:?}", gpu.adapter_info.backend)) {
        return Err("selected adapter backend differs from golden platform".into());
    }
    let metadata: Value = serde_json::from_str(&command(
        "cargo",
        &["metadata", "--locked", "--format-version", "1"],
        &root,
    )?)?;
    let dependencies: Value = metadata["packages"]
        .as_array()
        .ok_or("missing packages")?
        .iter()
        .filter(|p| {
            [
                "wgpu",
                "wgpu-hal",
                "wgpu-core",
                "naga",
                "objc2-metal",
                "objc2-io-surface",
                "objc2-core-foundation",
            ]
            .contains(&p["name"].as_str().unwrap_or(""))
        })
        .map(|p| (p["name"].as_str().unwrap().to_owned(), p["version"].clone()))
        .collect::<serde_json::Map<_, _>>()
        .into();
    let info = &gpu.adapter_info;
    let (hardware, architecture, os, platform_details) = if cfg!(target_os = "macos") {
        (
            json!({"model":command("sysctl",&["-n","hw.model"],&root)?,"cpu":command("sysctl",&["-n","machdep.cpu.brand_string"],&root)?,"memory":command("sysctl",&["-n","hw.memsize"],&root)?}),
            command("uname", &["-m"], &root)?,
            command("sw_vers", &[], &root)?,
            serde_json::from_str::<Value>(&command(
                "system_profiler",
                &["SPDisplaysDataType", "-json"],
                &root,
            )?)?,
        )
    } else if cfg!(target_os = "windows") {
        (
            json!({"computer":std::env::var("COMPUTERNAME").unwrap_or_default(),"processor":std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default()}),
            std::env::consts::ARCH.to_owned(),
            command("cmd", &["/C", "ver"], &root)?,
            json!({"api":"Direct3D12"}),
        )
    } else {
        (
            json!({"cpuinfo":fs::read_to_string("/proc/cpuinfo")?,"memory":fs::read_to_string("/proc/meminfo")?}),
            command("uname", &["-m"], &root)?,
            command("uname", &["-a"], &root)?,
            json!({"api":"Vulkan","icd":std::env::var("VK_DRIVER_FILES").ok()}),
        )
    };
    let environment = json!({"schema_version":1,"target":target,"baseline_profile":profile,"hardware":hardware,"architecture":architecture,"os":os,"platform_details":platform_details,"rust":command("rustc",&["--version","--verbose"],&root)?,"cargo":command("cargo",&["--version"],&root)?,"dependencies":dependencies,"adapter":{"name":info.name,"backend":format!("{:?}",info.backend),"device_type":format!("{:?}",info.device_type),"software_adapter":info.device_type==wgpu::DeviceType::Cpu,"vendor":info.vendor,"device":info.device,"driver":info.driver,"driver_info":info.driver_info},"required_features":format!("{:?}",gpu.device.features()),"required_limits":format!("{:?}",gpu.device.limits())});
    let pam_path = kronello_testkit::resolve_fixture("alpha")?;
    let pam_bytes = fs::read(pam_path)?;
    let scenes = scenes(Image::from_pam(&pam_bytes)?);
    if scenes.is_empty() {
        return Err("zero scenes".into());
    }
    let font_hash = hash(&fs::read(kronello_testkit::resolve_fixture(
        "noto-sans-cjk-jp",
    )?)?);
    let manifest = manifest(&scenes, &hash(&pam_bytes), &font_hash);
    let ids: Vec<_> = scenes.iter().map(|s| s.id).collect();
    let catalog: Value = serde_json::from_str(include_str!(
        "../../../tests/golden/apple-silicon-metal/scenes.json"
    ))?;
    if catalog["scene_ids"] != json!(ids) {
        return Err("scene catalog differs from harness".into());
    }
    let provenance = json!({"revision":command("git",&["rev-parse","HEAD"],&root)?,"status":command("git",&["status","--short"],&root)?,"shader_sha256":hash(SHADER.as_bytes()),"scene_shader_sha256":hash(SCENE_SHADER.as_bytes()),"effect_shader_sha256":hash(EFFECT_SHADER.as_bytes()),"effect_reference_code_sha256":hash(include_bytes!("../src/effect.rs")),"effect_kernel_code_sha256":hash(include_bytes!("../../kronello-render/src/effect.rs")),"scene_gpu_code_sha256":hash(include_bytes!("../src/scene_gpu.rs")),"scene_reference_code_sha256":hash(include_bytes!("../src/scene.rs")),"draw_fixture_code_sha256":hash(include_bytes!("common/mod.rs")),"cargo_lock_sha256":hash(&fs::read(root.join("Cargo.lock"))?),"fixture_manifest_sha256":hash(&fs::read(root.join("tests/fixtures/manifest.json"))?),"scene_code_sha256":hash(include_bytes!("golden.rs")),"comparison_code_sha256":hash(&fs::read(root.join("crates/kronello-testkit/src/lib.rs"))?),"renderer_code_sha256":hash(include_bytes!("../src/renderer.rs")),"color_code_sha256":hash(include_bytes!("../src/color.rs"))});
    fs::write(
        output.join("working-tree.patch"),
        command("git", &["diff", "--binary", "HEAD"], &root)?,
    )?;
    write_json(output.join("environment.json"), &environment)?;
    write_json(output.join("provenance.json"), &provenance)?;
    write_json_compact(output.join("manifest.json"), &manifest)?;
    let destination = if update {
        output.join("candidate")
    } else {
        output.join("actual")
    };
    fs::create_dir_all(&destination)?;
    write_json(destination.join("environment.json"), &environment)?;
    write_json_compact(destination.join("manifest.json"), &manifest)?;
    write_json(destination.join("provenance.json"), &provenance)?;
    if baseline.join("environment.json").exists() {
        let old: Value = serde_json::from_slice(&fs::read(baseline.join("environment.json"))?)?;
        write_json(
            output.join("environment-diff.json"),
            &json!({"baseline":old,"actual":environment,"equal":old==environment}),
        )?;
        // Hardware/OS generations are provenance; a software adapter must not
        // silently consume a hardware baseline (or vice versa).
        if !update && old["adapter"]["device_type"] != environment["adapter"]["device_type"] {
            return Err(
                "baseline adapter class differs; explicit separate adoption required".into(),
            );
        }
    } else if !update {
        return Err("baseline environment.json missing; comparison cannot pass".into());
    }
    if !update {
        validate_baseline(&baseline, &manifest)?;
    }
    let mut reports = Vec::new();
    let mut failed = false;
    for scene in &scenes {
        let size = RenderSize::pixels(scene.width, scene.height);
        let mut actual = if let Some(draw) = &scene.draw {
            gpu.render_scene(size, draw, scene.space)?
        } else {
            gpu.render(size, &scene.layers, scene.space)?
        };
        let d = descriptor(scene);
        // Even UPDATE validates every pixel against the independent CPU oracle.
        let mut oracle = if let Some(draw) = &scene.draw {
            render_scene_reference(size, draw, scene.space)?
        } else {
            render_reference(size, &scene.layers, scene.space)?
        };
        let mut external_bytes = None;
        if let Some(transform) = scene.output {
            let external = gpu.render_scene_output(
                size,
                scene.draw.as_ref().unwrap(),
                scene.space,
                transform,
            )?;
            external_bytes = Some(external.rgba16f);
            actual.pixels = external
                .pixels
                .into_iter()
                .map(|p| color::to_working(p, InputSpace::Srgb, scene.space))
                .collect();
            actual.rgba16f = actual
                .pixels
                .iter()
                .flatten()
                .flat_map(|&v| half::f16::from_f32(v).to_le_bytes())
                .collect();
            oracle = oracle
                .into_iter()
                .map(|p| {
                    convert_output_reference(p, scene.space, transform)
                        .map(|q| color::to_working(q, InputSpace::Srgb, scene.space))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        if scene.id == "isolated-nested-overlap" {
            let wrong = color::source_over([0.0, 0.0, 0.25, 0.25], [0.25, 0.0, 0.0, 0.25]);
            if (actual.pixels[27][3] - wrong[3]).abs() <= 0.1 {
                return Err("isolated group indistinguishable from distributed opacity".into());
            }
        }
        compare_pixels(
            LinearFrame {
                descriptor: &d,
                pixels: &oracle,
            },
            LinearFrame {
                descriptor: &d,
                pixels: &actual.pixels,
            },
            PixelTolerance::default(),
        )?;
        let dir = destination.join(scene.id);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("frame-0.rgba16f"), &actual.rgba16f)?;
        png(&dir.join("frame-0.png"), scene, &actual.pixels)?;
        if let Some(bytes) = external_bytes {
            fs::write(dir.join("external-srgb-straight.rgba16f"), bytes)?;
        }
        if !update {
            let expected =
                decode_rgba16f(&fs::read(baseline.join(scene.id).join("frame-0.rgba16f"))?)?;
            let result = compare_pixels(
                LinearFrame {
                    descriptor: &d,
                    pixels: &expected,
                },
                LinearFrame {
                    descriptor: &d,
                    pixels: &actual.pixels,
                },
                PixelTolerance::default(),
            );
            let report = match &result {
                Ok(r) => r.clone(),
                Err(kronello_testkit::PixelError::Mismatch(r)) => r.clone(),
                Err(e) => return Err(e.clone().into()),
            };
            failed |= result.is_err();
            let differences: Vec<[f32; 4]> = expected
                .iter()
                .zip(&actual.pixels)
                .map(|(e, a)| std::array::from_fn(|i| a[i] - e[i]))
                .collect();
            // Signed float32 differences avoid pretending they are premultiplied color.
            fs::write(
                dir.join("difference.rgba32f"),
                differences
                    .iter()
                    .flatten()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )?;
            let visual: Vec<[f32; 4]> = differences
                .iter()
                .map(|p| [p[0].abs() * 64.0, p[1].abs() * 64.0, p[2].abs() * 64.0, 1.0])
                .collect();
            png(&dir.join("difference.png"), scene, &visual)?;
            reports.push(json!({"scene":scene.id,"pixels":report.compared_pixels,"mismatched_pixels":report.mismatched_pixels,"max_rgb_error":report.max_rgb_error,"max_alpha_error":report.max_alpha_error,"result":if result.is_ok() {"pass"} else {"fail"}}));
        } else {
            reports.push(json!({"scene":scene.id,"pixels":actual.pixels.len(),"result":"candidate; CPU oracle validated","transfers":format!("{:?}",actual.transfers)}));
        }
    }
    if update {
        write_json(
            destination.join("adoption.json"),
            &artifact_manifest(&destination, &manifest, &environment, &provenance)?,
        )?;
    }
    let report = json!({"test_count":1,"scene_count":scenes.len(),"frame_count":scenes.len(),"status":if update {"candidate-only; no baseline comparison"} else if failed {"fail"} else {"pass"},"eligible_reference_hardware":info.device_type!=wgpu::DeviceType::Cpu,"eligible_comparison_environment":true,"candidate_may_be_adopted":update && provenance["status"] == "","provenance":provenance,"scenes":reports});
    write_json(output.join("report.json"), &report)?;
    if failed {
        return Err("golden pixel comparison failed; see report and differences".into());
    }
    Ok(report)
}
#[test]
#[ignore = "requires explicit platform GPU execution; UPDATE is candidate-only"]
fn fixed_environment_golden() -> Result<()> {
    if std::env::var("KRONELLO_GOLDEN").as_deref() != Ok("1") {
        return Err("KRONELLO_GOLDEN=1 required".into());
    }
    let output = PathBuf::from(
        std::env::var_os("KRONELLO_GOLDEN_OUTPUT").ok_or("KRONELLO_GOLDEN_OUTPUT required")?,
    );
    if !output.is_absolute() {
        return Err("KRONELLO_GOLDEN_OUTPUT must be absolute".into());
    }
    fs::create_dir_all(&output)?;
    // Do not allow artifacts to overwrite reviewed baselines or other repository inputs.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let output = output.canonicalize()?;
    if !output.starts_with(root.join("target/golden")) {
        return Err("golden output must be beneath target/golden".into());
    }
    if output.join("report.json").exists()
        || output.join("candidate").exists()
        || output.join("actual").exists()
    {
        return Err("use a fresh golden output directory".into());
    }
    match run(&output) {
        Ok(report) => {
            eprintln!("{report}");
            Ok(())
        }
        Err(error) => {
            let manifest = fs::read(output.join("manifest.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            let scenes = manifest.as_ref().and_then(|m| m["scenes"].as_array());
            let scene_count = scenes.map_or(0, Vec::len);
            let frame_count = scenes.map_or(0, |scenes| {
                scenes
                    .iter()
                    .filter(|scene| {
                        scene["id"].as_str().is_some_and(|id| {
                            ["candidate", "actual"].iter().any(|dir| {
                                output.join(dir).join(id).join("frame-0.rgba16f").is_file()
                            })
                        })
                    })
                    .count()
            });
            let failure = json!({"status":"fail","error":error.to_string(),"test_count":1,"scene_count":scene_count,"frame_count":frame_count});
            write_json(output.join("failure.json"), &failure)?;
            // Preserve a detailed pixel-comparison report if run() already wrote one.
            if !output.join("report.json").exists() {
                write_json(output.join("report.json"), &failure)?;
            }
            Err(error)
        }
    }
}

#[test]
fn cpu_catalog_and_vec003_manifest_match_harness() {
    let bytes = fs::read(kronello_testkit::resolve_fixture("alpha").unwrap()).unwrap();
    let scenes = scenes(Image::from_pam(&bytes).unwrap());
    let catalog: Value = serde_json::from_str(include_str!(
        "../../../tests/golden/apple-silicon-metal/scenes.json"
    ))
    .unwrap();
    assert_eq!(
        catalog["scene_ids"],
        json!(scenes.iter().map(|s| s.id).collect::<Vec<_>>())
    );
    assert_eq!(scenes.len(), 56);
    let m = manifest(&scenes, "fixture-test", "font-test");
    assert_eq!(
        m["stroke_geometry_version"],
        kronello_render::STROKE_GEOMETRY_VERSION
    );
    assert_eq!(
        m["gradient_interpolation_version"],
        kronello_render::GRADIENT_INTERPOLATION_VERSION
    );
    for id in ["gradient-linear-fill-stroke", "gradient-radial-fill-stroke"] {
        let scene = m["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        let path = &scene["draw"]["nodes"][0];
        assert_eq!(path["fill_gradient"]["stops"].as_array().unwrap().len(), 4);
        assert_eq!(
            path["stroke_gradient"]["stops"].as_array().unwrap().len(),
            4
        );
        assert_eq!(path["stroke"]["miter_limit"], 4.0);
        assert_eq!(
            path["stroke_geometry_version"],
            kronello_model::LEGACY_STROKE_VERSION
        );
        assert!(path["paint_transform"].is_array());
    }
    for (id, alignment) in [
        ("stroke-dashes", "center"),
        ("stroke-inside-evenodd", "inside"),
        ("stroke-outside-nonzero", "outside"),
        ("stroke-affine-reflected", "center"),
    ] {
        let scene = m["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        let path = &scene["draw"]["nodes"][0];
        assert_eq!(
            path["stroke_geometry_version"],
            kronello_model::EXTENDED_STROKE_VERSION
        );
        assert_eq!(
            path["local_stroke"]["version"],
            kronello_model::EXTENDED_STROKE_VERSION
        );
        assert_eq!(path["local_stroke"]["alignment"], alignment);
        assert!(path["local_stroke"]["inverse"].is_array());
        assert!(path["local_stroke"]["contours"].is_array());
        assert_eq!(path["local_stroke"]["dash_offset"], 0.371);
        assert!(path["local_stroke"]["dash_array"].is_array());
    }
}

#[test]
fn cpu_environment_gate_ignores_provenance() {
    assert!(eligible_environment("aarch64-apple-darwin", "Metal"));
    assert!(eligible_environment("x86_64-unknown-linux-gnu", "Vulkan"));
    assert!(eligible_environment("x86_64-pc-windows-msvc", "Dx12"));
    assert!(!eligible_environment("x86_64-apple-darwin", "Metal"));
    assert!(!eligible_environment("aarch64-unknown-linux-gnu", "Metal"));
    assert!(!eligible_environment("aarch64-apple-darwin", "Vulkan"));
}

#[test]
fn cpu_baseline_integrity_rejects_missing_and_modified_artifacts() {
    let dir =
        std::env::temp_dir().join(format!("kronello-golden-integrity-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let settings = json!({"comparison_version":1,"rgb_absolute":1.0/1024.0,"rgb_relative":1.0/1024.0,"alpha_absolute":1.0/1024.0,"scenes":[{"id":"test","size":[1,1],"sample_id":"frame-0","samples_per_frame":1,"time":{"num":"0","den":"1"},"working_space":"LinearRec709","alpha":"premultiplied","output_transform":null}]});
    let environment = json!({"hardware":{"model":"arbitrary"},"os":"arbitrary","adapter":{"name":"arbitrary","backend":"Metal"}});
    let provenance = json!({"revision":"historical-baseline-revision"});
    write_json(dir.join("manifest.json"), &settings).unwrap();
    write_json(dir.join("environment.json"), &environment).unwrap();
    write_json(dir.join("provenance.json"), &provenance).unwrap();
    fs::create_dir(dir.join("test")).unwrap();
    fs::write(dir.join("test/frame-0.rgba16f"), [0_u8; 8]).unwrap();
    fs::write(dir.join("test/frame-0.png"), b"synthetic display artifact").unwrap();
    let adoption = artifact_manifest(&dir, &settings, &environment, &provenance).unwrap();
    assert!(validate_baseline(&dir, &settings).is_err());
    write_json(dir.join("adoption.json"), &adoption).unwrap();
    assert!(validate_baseline(&dir, &settings).is_ok());
    let changed = json!({"scenes":[]});
    assert!(artifact_manifest(&dir, &changed, &environment, &provenance).is_err());
    assert!(validate_baseline(&dir, &changed).is_err());
    fs::write(dir.join("test/frame-0.rgba16f"), [1_u8; 8]).unwrap();
    assert!(validate_baseline(&dir, &settings).is_err());
    fs::remove_file(dir.join("test/frame-0.rgba16f")).unwrap();
    assert!(validate_baseline(&dir, &settings).is_err());
    fs::remove_dir_all(dir).unwrap();
}
