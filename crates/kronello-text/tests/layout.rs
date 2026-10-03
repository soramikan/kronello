use kronello_model::*;
use kronello_testkit::resolve_fixture;
use kronello_text::*;
use serde_json::Value;
use std::sync::OnceLock;

fn f(value: f64) -> FiniteF64 {
    FiniteF64::new(value).unwrap()
}
fn font() -> &'static (Vec<u8>, FontRef) {
    static FONT: OnceLock<(Vec<u8>, FontRef)> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = std::fs::read(resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
        let identity = pin_font(&bytes, 0).unwrap();
        (bytes, identity)
    })
}
fn input(text: &str, width: f64) -> ResolvedText {
    ResolvedText {
        layout_version: LAYOUT_VERSION,
        text: text.into(),
        styles: if text.is_empty() {
            vec![]
        } else {
            vec![ResolvedTextStyle {
                range: TextRange {
                    start: 0,
                    end: text.len(),
                },
                font: font().1.clone(),
                size: f(20.0),
                fill: Color::from_srgb8([0; 3], None),
            }]
        },
        direction: TextDirection::Horizontal,
        ruby: vec![],
        wrap_width: f(width),
        line_height: f(30.0),
        alignment: TextAlignment::Start,
    }
}
fn run(text: &ResolvedText) -> Result<LayoutResult, LayoutError> {
    layout(
        text,
        &[FontData {
            identity: &font().1,
            bytes: &font().0,
        }],
    )
}
fn cases() -> Value {
    serde_json::from_slice(&std::fs::read(resolve_fixture("japanese").unwrap()).unwrap()).unwrap()
}
fn case(id: &str) -> String {
    cases()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .into()
}
fn assert_mapping(text: &str, result: &LayoutResult) {
    let mut end = 0;
    for g in &result.graphemes {
        assert_eq!(g.source.start, end);
        assert!(text.is_char_boundary(g.source.start) && text.is_char_boundary(g.source.end));
        assert!(!g.shaping_clusters.is_empty());
        for index in g.shaping_clusters.clone() {
            let c = &result.shaping_clusters[index];
            assert!(c.source.start <= g.source.start && c.source.end >= g.source.end);
        }
        end = g.source.end;
    }
    assert_eq!(end, text.len());
    let mut cluster_end = 0;
    let mut glyph_end = 0;
    for (index, c) in result.shaping_clusters.iter().enumerate() {
        assert_eq!(c.source.start, cluster_end);
        assert_eq!(c.glyphs.start, glyph_end);
        for glyph in &result.glyphs[c.glyphs.clone()] {
            assert_eq!(glyph.source, c.source);
            assert_eq!(glyph.shaping_cluster, index);
            assert_eq!(glyph.graphemes, c.graphemes);
            assert!(glyph.glyph_id > 0);
        }
        cluster_end = c.source.end;
        glyph_end = c.glyphs.end;
    }
    assert_eq!(cluster_end, text.len());
    assert_eq!(glyph_end, result.glyphs.len());
    let mut next_cluster = 0;
    for unit in &result.animation_units {
        assert_eq!(unit.shaping_clusters.start, next_cluster);
        assert_eq!(
            result.shaping_clusters[unit.shaping_clusters.start]
                .source
                .start,
            unit.source.start
        );
        assert_eq!(
            result.shaping_clusters[unit.shaping_clusters.end - 1]
                .source
                .end,
            unit.source.end
        );
        next_cluster = unit.shaping_clusters.end;
    }
    assert_eq!(next_cluster, result.shaping_clusters.len());
}

#[test]
fn fixed_font_identity_matches_manifest_and_font_names() {
    assert_eq!(font().1.family, "Noto Sans CJK JP");
    assert_eq!(font().1.postscript_name, "NotoSansCJKjp-Regular");
    let manifest: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/manifest.json")).unwrap();
    let entry = manifest["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "noto-sans-cjk-jp")
        .unwrap();
    assert_eq!(font().1.sha256, entry["sha256"]);
}
#[test]
fn combining_dakuten_and_ivs_preserve_original_source_and_glyph_selection() {
    for id in ["combining", "ivs"] {
        let text = case(id);
        let result = run(&input(&text, 200.0)).unwrap();
        assert_mapping(&text, &result);
        assert_eq!(result.graphemes.len(), 1);
        assert_eq!(result.shaping_clusters.len(), 1);
        assert_eq!(result.glyphs.len(), 1);
        assert_eq!(result.animation_units.len(), 1);
        assert_eq!(
            result.glyphs[0].source,
            TextRange {
                start: 0,
                end: text.len()
            }
        );
    }
    let decomposed = run(&input(&case("combining"), 200.0)).unwrap();
    let composed = run(&input("が", 200.0)).unwrap();
    assert_eq!(decomposed.glyphs[0].glyph_id, composed.glyphs[0].glyph_id);
    let ivs = run(&input(&case("ivs"), 200.0)).unwrap();
    let plain = run(&input("葛", 200.0)).unwrap();
    assert_ne!(ivs.glyphs[0].glyph_id, plain.glyphs[0].glyph_id);
}
#[test]
fn one_grapheme_maps_to_multiple_glyphs_as_one_animation_unit() {
    let text = case("multi-glyph-grapheme");
    let result = run(&input(&text, 200.0)).unwrap();
    assert_mapping(&text, &result);
    assert_eq!(result.graphemes.len(), 1);
    assert_eq!(result.glyphs.len(), 2);
    assert_eq!(result.animation_units[0].glyphs, 0..2);
}
#[test]
fn ligature_maps_multiple_graphemes_to_one_shaping_cluster() {
    let text = case("ligature");
    let result = run(&input(&text, 200.0)).unwrap();
    assert_mapping(&text, &result);
    assert_eq!(result.graphemes.len(), 6);
    assert_eq!(result.glyphs.len(), 4);
    let ligature = result
        .shaping_clusters
        .iter()
        .find(|c| c.graphemes.len() == 3)
        .unwrap();
    assert_eq!(ligature.source, TextRange { start: 1, end: 4 });
    assert_eq!(ligature.glyphs.len(), 1);
    assert!(
        result
            .animation_units
            .iter()
            .any(|u| u.source.start <= 1 && u.source.end >= 4)
    );
    for g in &result.graphemes[1..4] {
        assert_eq!(g.shaping_clusters, 1..2);
    }
}
#[test]
fn horizontal_japanese_wraps_with_basic_kinsoku() {
    let text = case("kinsoku");
    let result = run(&input(&text, 60.0)).unwrap();
    assert_mapping(&text, &result);
    let lines: Vec<_> = result
        .lines
        .iter()
        .map(|l| &text[l.source.start..l.source.end])
        .collect();
    assert_eq!(lines, ["「日本", "語」、", "句読", "点。"]);
    assert!(
        result
            .lines
            .iter()
            .all(|l| l.advance <= 60.0 && !l.overflow)
    );
    assert_eq!(
        result.layout_bounds,
        Bounds {
            min: [0.0; 2],
            max: [60.0, 120.0]
        }
    );
    for line in lines {
        assert!(!prohibited_line_start(line.chars().next().unwrap()));
        assert!(!prohibited_line_end(line.chars().next_back().unwrap()));
    }
}
#[test]
fn small_kana_long_mark_and_parentheses_do_not_create_prohibited_soft_breaks() {
    let text = "あ（いう）ぁーえお";
    let result = run(&input(text, 40.0)).unwrap();
    assert_mapping(text, &result);
    assert!(result.lines.len() > 1);
    for line in &result.lines {
        let slice = &text[line.source.start..line.source.end];
        assert!(!prohibited_line_start(slice.chars().next().unwrap()));
        assert!(!prohibited_line_end(slice.chars().next_back().unwrap()));
    }
    assert!(result.lines.iter().any(|l| l.overflow));
}
#[test]
fn indivisible_segments_report_overflow_without_breaking_ligatures() {
    let text = case("ligature");
    let result = run(&input(&text, 1.0)).unwrap();
    assert_mapping(&text, &result);
    assert_eq!(result.lines.len(), 1);
    assert!(result.lines[0].overflow);
    assert_eq!(result.glyphs.len(), 4);
}
#[test]
fn newlines_crlf_empty_lines_and_spaces_preserve_mapping() {
    let text = "あ\r\n\nい\u{2028}";
    let result = run(&input(text, 200.0)).unwrap();
    assert_mapping(text, &result);
    assert_eq!(result.lines.len(), 4);
    assert_eq!(
        result
            .lines
            .iter()
            .map(|l| l.hard_break)
            .collect::<Vec<_>>(),
        [true, true, true, false]
    );
    assert_eq!(
        result.lines.last().unwrap().source,
        TextRange {
            start: text.len(),
            end: text.len()
        }
    );
    let empty = run(&input("", 100.0)).unwrap();
    assert_eq!(empty.lines.len(), 1);
    assert_eq!(empty.ink_bounds, None);
    assert_eq!(empty.layout_bounds.max, [100.0, 30.0]);
    let spaces = run(&input("  ", 100.0)).unwrap();
    assert!(spaces.glyphs.iter().all(|g| g.outline.segments.is_empty()));
    assert_eq!(spaces.ink_bounds, None);
    assert!(spaces.lines[0].advance > 0.0);
}
#[test]
fn alignment_moves_glyphs_ink_and_outlines_in_design_space() {
    let start = run(&input("日", 100.0)).unwrap();
    for (alignment, shift) in [(TextAlignment::Center, 40.0), (TextAlignment::End, 80.0)] {
        let mut text = input("日", 100.0);
        text.alignment = alignment;
        let result = run(&text).unwrap();
        assert_eq!(
            result.glyphs[0].position[0] - start.glyphs[0].position[0],
            shift
        );
        assert_eq!(
            result.ink_bounds.unwrap().min[0] - start.ink_bounds.unwrap().min[0],
            shift
        );
        let x = |g: &PositionedGlyph| match g.outline.segments[0] {
            PathSegment::MoveTo(p) => p[0].get(),
            _ => panic!("outline must begin with move"),
        };
        assert_eq!(x(&result.glyphs[0]) - x(&start.glyphs[0]), shift);
        assert_eq!(result.layout_bounds, start.layout_bounds);
    }
}
#[test]
fn multiple_styles_use_evaluated_sizes_colors_and_font_metrics() {
    let mut text = input("日本", 100.0);
    text.styles[0].range.end = 3;
    let mut second = text.styles[0].clone();
    second.range = TextRange { start: 3, end: 6 };
    second.size = f(40.0);
    second.fill = Color::new(ColorSpace::LinearRec709, [2.0, -0.5, 0.0], 0.3).unwrap();
    text.styles.push(second);
    let result = run(&text).unwrap();
    assert_mapping(&text.text, &result);
    assert_eq!(result.lines[0].advance, 60.0);
    assert_eq!(result.lines[0].baseline, 46.4);
    assert_eq!(result.glyphs[1].style_index, 1);
    assert_eq!(result.glyphs[1].fill, text.styles[1].fill);
}
#[test]
fn paths_and_ink_bounds_are_valid_and_distinct_from_line_boxes() {
    let result = run(&input("か", 200.0)).unwrap();
    let ink = result.ink_bounds.unwrap();
    assert!(ink.min[0] > 0.0 && ink.max[0] < 20.0);
    assert!(ink.min[1] > 0.0 && ink.max[1] < 30.0);
    assert_ne!(ink, result.layout_bounds);
    for glyph in &result.glyphs {
        validate_path(&glyph.outline).unwrap();
        assert!(!glyph.outline.segments.is_empty());
    }
}
#[test]
fn missing_font_hash_mismatch_metadata_and_face_mismatch_are_typed() {
    let text = input("日", 100.0);
    assert!(
        matches!(layout(&text,&[]),Err(LayoutError::MissingFont { font: expected }) if expected == font().1)
    );
    let mut corrupt = font().0.clone();
    corrupt[100] ^= 1;
    assert!(matches!(
        layout(
            &text,
            &[FontData {
                identity: &font().1,
                bytes: &corrupt
            }]
        ),
        Err(LayoutError::FontHashMismatch { .. })
    ));
    let mut wrong = text.clone();
    wrong.styles[0].font.family = "Different Family".into();
    assert!(matches!(
        layout(
            &wrong,
            &[FontData {
                identity: &wrong.styles[0].font,
                bytes: &font().0
            }]
        ),
        Err(LayoutError::FontIdentityMismatch { .. })
    ));
    wrong.styles[0].font = font().1.clone();
    wrong.styles[0].font.face_index = 1;
    assert!(matches!(
        layout(
            &wrong,
            &[FontData {
                identity: &wrong.styles[0].font,
                bytes: &font().0
            }]
        ),
        Err(LayoutError::InvalidFont { face_index: 1 })
    ));
    assert!(matches!(
        pin_font(b"not a font", 0),
        Err(LayoutError::InvalidFont { .. })
    ));
}
#[test]
fn missing_emoji_and_unmapped_ivs_list_whole_source_clusters_without_fallback() {
    let emoji = case("emoji");
    let text = format!("日{emoji}\u{10ffff}");
    let Err(LayoutError::MissingGlyphs { clusters }) = run(&input(&text, 200.0)) else {
        panic!("missing glyphs must fail final layout")
    };
    assert_eq!(clusters.len(), 2);
    assert_eq!(clusters[0].text, emoji);
    assert_eq!(clusters[0].source, TextRange { start: 3, end: 14 });
    assert_eq!(clusters[1].text, "\u{10ffff}");
    assert!(clusters.iter().all(|c| c.font == font().1));
    let ivs = "葛\u{e01ef}";
    let Err(LayoutError::MissingGlyphs { clusters }) = run(&input(ivs, 200.0)) else {
        panic!("unmapped IVS must not silently drop its selector")
    };
    assert_eq!(clusters[0].text, ivs);
    assert_eq!(clusters[0].source, TextRange { start: 0, end: 7 });
}
#[test]
fn variants_fixture_is_supported_by_the_pinned_font() {
    let text = case("variants");
    let result = run(&input(&text, 200.0)).unwrap();
    assert_mapping(&text, &result);
    assert_eq!(result.glyphs.len(), 2);
}
#[test]
fn ruby_vertical_future_versions_and_controls_fail_explicitly() {
    let mut vertical = input(&case("vertical"), 200.0);
    vertical.direction = TextDirection::VerticalRl;
    assert!(matches!(
        run(&vertical),
        Err(LayoutError::UnsupportedFeature {
            feature: "vertical text"
        })
    ));
    let mut ruby = input(&case("ruby"), 200.0);
    ruby.ruby.push(RubyAssociation {
        base: TextRange { start: 0, end: 6 },
        text: "かんじ".into(),
    });
    assert!(matches!(
        run(&ruby),
        Err(LayoutError::UnsupportedFeature { feature: "ruby" })
    ));
    let mut version = input("日", 200.0);
    version.layout_version = 999;
    assert_eq!(
        run(&version).unwrap_err(),
        LayoutError::UnsupportedVersion { version: 999 }
    );
    assert!(matches!(
        run(&input("a\tb", 200.0)),
        Err(LayoutError::UnsupportedFeature { .. })
    ));
    assert!(matches!(
        run(&input("a\u{202e}b", 200.0)),
        Err(LayoutError::UnsupportedFeature { .. })
    ));
}
#[test]
fn invalid_dimensions_spans_and_budget_cannot_bypass_resolved_validation() {
    let mut text = input("か\u{3099}", 100.0);
    text.styles[0].range.end = 3;
    assert_eq!(
        run(&text).unwrap_err(),
        LayoutError::Text(TextError::InvalidSpans)
    );
    text = input("日", 100.0);
    text.wrap_width = f(0.0);
    assert_eq!(
        run(&text).unwrap_err(),
        LayoutError::Text(TextError::InvalidDimension)
    );
    text = input("日", 100.0);
    text.styles[0].size = f(-1.0);
    assert_eq!(
        run(&text).unwrap_err(),
        LayoutError::Text(TextError::InvalidDimension)
    );
    assert_eq!(
        run(&input(&"a".repeat(65_537), 100.0)).unwrap_err(),
        LayoutError::BudgetExceeded
    );
    let mut extreme = input("日\n日", 100.0);
    extreme.line_height = f(f64::MAX);
    assert_eq!(run(&extreme).unwrap_err(), LayoutError::NonFiniteGeometry);
}
#[test]
fn layout_is_repeatable_and_has_no_input_history_or_mutation() {
    let text = input("日本語 office か\u{3099}", 100.0);
    let before = text.clone();
    let first = run(&text).unwrap();
    run(&input("違う文", 50.0)).unwrap();
    let second = run(&text).unwrap();
    assert_eq!(text, before);
    assert_eq!(first, second);
    assert_mapping(&text.text, &first);
}

#[test]
fn extended_graphemes_remain_atomic_even_when_shaper_unicode_segmentation_differs() {
    let flag = "🇯🇵";
    let Err(LayoutError::MissingGlyphs { clusters }) = run(&input(flag, 200.0)) else {
        panic!("regional indicators are absent in the pinned font")
    };
    assert_eq!(clusters.len(), 1);
    assert_eq!(
        clusters[0].source,
        TextRange {
            start: 0,
            end: flag.len()
        }
    );
    assert_eq!(clusters[0].text, flag);
    let hangul = "\u{1100}\u{1161}\u{11a8}";
    let result = run(&input(hangul, 200.0)).unwrap();
    assert_mapping(hangul, &result);
    assert_eq!(result.graphemes.len(), 1);
    assert_eq!(result.shaping_clusters.len(), 1);
    assert_eq!(result.animation_units.len(), 1);
}
#[test]
fn mixed_size_lines_keep_absolute_baseline_spacing() {
    let mut text = input("日\n本", 100.0);
    text.styles[0].range.end = 4;
    let mut second = text.styles[0].clone();
    second.range = TextRange { start: 4, end: 7 };
    second.size = f(40.0);
    text.styles.push(second);
    let result = run(&text).unwrap();
    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].baseline, 46.4);
    assert_eq!(result.lines[1].baseline, result.lines[0].baseline + 30.0);
    assert_mapping(&text.text, &result);
}
#[test]
fn duplicate_font_sources_are_rejected_independent_of_source_order() {
    let text = input("日", 100.0);
    let source = FontData {
        identity: &font().1,
        bytes: &font().0,
    };
    assert_eq!(
        layout(&text, &[source, source]).unwrap_err(),
        LayoutError::UnsupportedFeature {
            feature: "duplicate font source"
        }
    );
}
