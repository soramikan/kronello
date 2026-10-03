//! Explicit fixed-environment comparison. UPDATE produces candidates only.
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
            "gpu002-grid4-v1"
        } else {
            "gpu001-linear-v1"
        }
        .into(),
        samples_per_frame: if scene.draw.is_some() { 16 } else { 1 },
        seed: 0,
    }
}
fn draw_manifest(scene: &DrawScene) -> Value {
    json!({"roots":scene.roots,"nodes":scene.nodes.iter().map(|node| match node {
        DrawNode::Path(p)=>json!({"kind":"path","contours":p.contours.iter().map(|c| json!({"points":c.points,"closed":c.closed})).collect::<Vec<_>>(),"fill":p.fill.map(|f| json!({"rgba":f.paint.rgba,"space":format!("{:?}",f.paint.space),"rule":format!("{:?}",f.rule)})),"stroke":p.stroke.map(|s| json!({"rgba":s.paint.rgba,"space":format!("{:?}",s.paint.space),"width":s.width,"cap":"round","join":"round"}))}),
        DrawNode::Group {children,opacity}=>json!({"kind":"isolated-group","children":children,"opacity":opacity}),
        DrawNode::Masked {source,matte,kind}=>json!({"kind":"masked","source":source,"matte":matte,"mode":format!("{:?}",kind)})
    }).collect::<Vec<_>>()})
}
fn manifest(scenes: &[Scene], fixture_hash: &str, font_hash: &str) -> Value {
    json!({"schema_version":2,"comparison_version":1,"rgb_absolute":1.0/1024.0,"rgb_relative":1.0/1024.0,"alpha_absolute":1.0/1024.0,"fixture_hash":fixture_hash,"font_hash":font_hash,"flatten_tolerance_px":0.02,"catalog":serde_json::from_str::<Value>(include_str!("../../../tests/golden/m4-macos-metal/scenes.json")).unwrap(),"scenes":scenes.iter().map(|s| json!({"id":s.id,"sample_id":"frame-0","size":[s.width,s.height],"design_extent":[s.width,s.height],"origin":[0,0],"time":{"num":"0","den":"1"},"working_space":format!("{:?}",s.space),"alpha":"premultiplied","output_transform":s.output.map(|o| format!("{:?}",o)),"comparison_space":"linear working-space premultiplied; external output decoded back before comparison","display_transform":"external-unpremultiply-then-srgb-clamp; visualization only","color_pipeline_id":if s.draw.is_some() {"gpu002-grid4-v1"} else {"gpu001-linear-v1"},"samples_per_frame":if s.draw.is_some() {16} else {1},"seed":0,"draw":s.draw.as_ref().map(draw_manifest),"layers":s.layers.iter().map(|l| json!({"size":l.size,"translation":l.translation,"rotation_degrees":l.rotation_degrees,"input_space":format!("{:?}",l.image.space),"input_size":[l.image.width,l.image.height],"straight_pixels":l.image.pixels})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
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
    if !cfg!(all(target_os = "macos", target_arch = "aarch64"))
        || std::env::var("WGPU_BACKEND").as_deref() != Ok("metal")
    {
        return Err("golden requires native aarch64 macOS and WGPU_BACKEND=metal".into());
    }
    let update = match std::env::var("KRONELLO_GOLDEN_UPDATE") {
        Ok(v) if v == "1" => true,
        Err(std::env::VarError::NotPresent) => false,
        _ => return Err("KRONELLO_GOLDEN_UPDATE must be unset or 1".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let baseline = root.join("tests/golden/m4-macos-metal");
    let gpu = GpuContext::new()?;
    if gpu.adapter_info.backend != wgpu::Backend::Metal {
        return Err("selected adapter is not Metal".into());
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
    let hardware = json!({"model":command("sysctl",&["-n","hw.model"],&root)?,"cpu":command("sysctl",&["-n","machdep.cpu.brand_string"],&root)?,"memory":command("sysctl",&["-n","hw.memsize"],&root)?});
    let environment = json!({"schema_version":1,"hardware":hardware,"architecture":command("uname",&["-m"],&root)?,"os":command("sw_vers",&[],&root)?,"metal":serde_json::from_str::<Value>(&command("system_profiler",&["SPDisplaysDataType","-json"],&root)?)?,"rust":command("rustc",&["--version","--verbose"],&root)?,"cargo":command("cargo",&["--version"],&root)?,"dependencies":dependencies,"adapter":{"name":info.name,"backend":format!("{:?}",info.backend),"device_type":format!("{:?}",info.device_type),"vendor":info.vendor,"device":info.device,"driver":info.driver,"driver_info":info.driver_info,"driver_version":"Metal driver separately unavailable; macOS build in os"},"required_features":format!("{:?}",gpu.device.features()),"required_limits":format!("{:?}",gpu.device.limits())});
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
        "../../../tests/golden/m4-macos-metal/scenes.json"
    ))?;
    if catalog["scene_ids"] != json!(ids) {
        return Err("scene catalog differs from harness".into());
    }
    let provenance = json!({"revision":command("git",&["rev-parse","HEAD"],&root)?,"status":command("git",&["status","--short"],&root)?,"shader_sha256":hash(SHADER.as_bytes()),"scene_shader_sha256":hash(SCENE_SHADER.as_bytes()),"scene_gpu_code_sha256":hash(include_bytes!("../src/scene_gpu.rs")),"scene_reference_code_sha256":hash(include_bytes!("../src/scene.rs")),"draw_fixture_code_sha256":hash(include_bytes!("common/mod.rs")),"cargo_lock_sha256":hash(&fs::read(root.join("Cargo.lock"))?),"fixture_manifest_sha256":hash(&fs::read(root.join("tests/fixtures/manifest.json"))?),"scene_code_sha256":hash(include_bytes!("golden.rs")),"comparison_code_sha256":hash(&fs::read(root.join("crates/kronello-testkit/src/lib.rs"))?),"renderer_code_sha256":hash(include_bytes!("../src/renderer.rs")),"color_code_sha256":hash(include_bytes!("../src/color.rs"))});
    fs::write(
        output.join("working-tree.patch"),
        command("git", &["diff", "--binary", "HEAD"], &root)?,
    )?;
    write_json(output.join("environment.json"), &environment)?;
    write_json(output.join("provenance.json"), &provenance)?;
    write_json(output.join("manifest.json"), &manifest)?;
    let destination = if update {
        output.join("candidate")
    } else {
        output.join("actual")
    };
    fs::create_dir_all(&destination)?;
    write_json(destination.join("environment.json"), &environment)?;
    write_json(destination.join("manifest.json"), &manifest)?;
    write_json(destination.join("provenance.json"), &provenance)?;
    if baseline.join("environment.json").exists() {
        let old: Value = serde_json::from_slice(&fs::read(baseline.join("environment.json"))?)?;
        write_json(
            output.join("environment-diff.json"),
            &json!({"baseline":old,"actual":environment,"equal":old==environment}),
        )?;
        if !update && old != environment {
            return Err("environment fingerprint mismatch".into());
        }
    } else if !update {
        return Err("baseline environment.json missing; comparison cannot pass".into());
    }
    if !update {
        if hardware["cpu"] != "Apple M4"
            || hardware["memory"] != "34359738368"
            || hardware["model"] != "Mac16,10"
        {
            return Err("comparison requires M4 Mac mini 32GB reference machine".into());
        }
        let old: Value = serde_json::from_slice(&fs::read(baseline.join("manifest.json"))?)?;
        if old != manifest {
            return Err("scene manifest mismatch; review inputs and comparison version".into());
        }
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
    let report = json!({"test_count":1,"scene_count":scenes.len(),"frame_count":scenes.len(),"status":if update {"candidate-only; no baseline comparison"} else if failed {"fail"} else {"pass"},"eligible_reference_hardware":hardware["cpu"]=="Apple M4" && hardware["memory"]=="34359738368" && hardware["model"]=="Mac16,10","candidate_may_be_adopted":false,"provenance":provenance,"scenes":reports});
    write_json(output.join("report.json"), &report)?;
    if failed {
        return Err("golden pixel comparison failed; see report and differences".into());
    }
    Ok(report)
}
#[test]
#[ignore = "requires explicit golden environment and fixed Metal reference; UPDATE is candidate-only"]
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
