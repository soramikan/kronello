//! Pure, resolution-independent bounds stages. Each LayoutValue uses one
//! caller-selected coordinate space in design_px; Scene IR uses Composition space.
use crate::{RenderError, SceneContent, SceneNodeIr};
use kronello_eval::{Affine2, NodeKey};
use kronello_model::{BoundsStage, ResolvedEffect, ResolvedGeometry, StrokeCap, StrokeJoin};
use kronello_text::LayoutResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesignBounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}
impl DesignBounds {
    pub fn checked(min: [f64; 2], max: [f64; 2]) -> Result<Self, RenderError> {
        if !min.into_iter().chain(max).all(f64::is_finite) || (0..2).any(|i| min[i] > max[i]) {
            return Err(RenderError::InvalidInput("invalid design bounds".into()));
        }
        Ok(Self { min, max })
    }
    pub fn transform(self, transform: Affine2) -> Result<Self, RenderError> {
        let points = [
            self.min,
            [self.min[0], self.max[1]],
            self.max,
            [self.max[0], self.min[1]],
        ]
        .map(|p| transform.transform_point(p));
        Self::checked(
            [0, 1].map(|i| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)),
            [0, 1].map(|i| {
                points
                    .iter()
                    .map(|p| p[i])
                    .fold(f64::NEG_INFINITY, f64::max)
            }),
        )
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            min: [0, 1].map(|i| self.min[i].min(other.min[i])),
            max: [0, 1].map(|i| self.max[i].max(other.max[i])),
        }
    }
    fn expand(self, halo: f64) -> Result<Self, RenderError> {
        Self::checked(self.min.map(|x| x - halo), self.max.map(|x| x + halo))
    }
}

/// Semantic bounds supplied by the upper compiler, without backend objects or
/// evaluation history. None denotes no geometry at that stage, never a fallback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutValue {
    pub layout_bounds: Option<DesignBounds>,
    pub ink_bounds: Option<DesignBounds>,
    pub visual_bounds: Option<DesignBounds>,
}
impl LayoutValue {
    pub fn select(self, stage: BoundsStage) -> Option<DesignBounds> {
        match stage {
            BoundsStage::Layout => self.layout_bounds,
            BoundsStage::Ink => self.ink_bounds,
            BoundsStage::Visual => self.visual_bounds,
        }
    }
    fn union(self, other: Self) -> Self {
        let union = |a: Option<DesignBounds>, b: Option<DesignBounds>| match (a, b) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        };
        Self {
            layout_bounds: union(self.layout_bounds, other.layout_bounds),
            ink_bounds: union(self.ink_bounds, other.ink_bounds),
            visual_bounds: union(self.visual_bounds, other.visual_bounds),
        }
    }
}

pub(crate) fn apply_effects(
    mut bounds: Option<DesignBounds>,
    effects: &[ResolvedEffect],
) -> Result<Option<DesignBounds>, RenderError> {
    for effect in effects {
        let Some(input) = bounds else { break };
        // Raster execution rounds this support outwards to its output lattice.
        bounds = Some(match effect {
            ResolvedEffect::AffineGaussianBlur { sigma, linear }
            | ResolvedEffect::AffineDropShadow { sigma, linear, .. } => {
                crate::validate_affine_linear(*linear)?;
                let halo = linear.map(|r| 3.0 * sigma * r[0].hypot(r[1]));
                let expanded = DesignBounds::checked(
                    [0, 1].map(|i| input.min[i] - halo[i]),
                    [0, 1].map(|i| input.max[i] + halo[i]),
                )?;
                if let ResolvedEffect::AffineDropShadow { offset, .. } = effect {
                    input.union(DesignBounds::checked(
                        [0, 1].map(|i| expanded.min[i] + offset[i]),
                        [0, 1].map(|i| expanded.max[i] + offset[i]),
                    )?)
                } else {
                    expanded
                }
            }
            ResolvedEffect::GaussianBlur { sigma } => input.expand(3.0 * sigma)?,
            ResolvedEffect::DropShadow { sigma, offset, .. } => {
                let shadow = input.expand(3.0 * sigma)?;
                input.union(DesignBounds::checked(
                    [0, 1].map(|i| shadow.min[i] + offset[i]),
                    [0, 1].map(|i| shadow.max[i] + offset[i]),
                )?)
            }
            // COLOR-002/COLOR-003 pointwise operations never change geometry
            // bounds.
            ResolvedEffect::ColorExposure { .. }
            | ResolvedEffect::ColorLevels { .. }
            | ResolvedEffect::ColorCurves { .. }
            | ResolvedEffect::ColorHsl { .. }
            | ResolvedEffect::ColorLut { .. } => input,
            // FX-005/FX-006 (ADR-0115): keying mattes and vignette keep the
            // input extent; glow/sharpen grow by the 3-sigma kernel support;
            // corner pin replaces them with the destination quad hull.
            ResolvedEffect::ChromaKey { .. }
            | ResolvedEffect::LumaKey { .. }
            | ResolvedEffect::Vignette { .. } => input,
            // TRACK-002 (ADR-0122): the inverse warp resamples within the
            // node's own coverage; the corrected frame keeps the clip bounds.
            ResolvedEffect::Stabilize { .. } => input,
            ResolvedEffect::Glow { radius, .. } | ResolvedEffect::Sharpen { radius, .. } => {
                input.expand(3.0 * radius)?
            }
            ResolvedEffect::CornerPin { corners } => DesignBounds::checked(
                [0, 1].map(|i| corners.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)),
                [0, 1].map(|i| {
                    corners
                        .iter()
                        .map(|p| p[i])
                        .fold(f64::NEG_INFINITY, f64::max)
                }),
            )?,
        });
    }
    Ok(bounds)
}

pub(crate) fn text_bounds(
    layout: &LayoutResult,
    transform: Affine2,
    effects: &[ResolvedEffect],
) -> Result<LayoutValue, RenderError> {
    let map = |b: kronello_text::Bounds| DesignBounds::checked(b.min, b.max)?.transform(transform);
    let layout_bounds = Some(map(layout.layout_bounds)?);
    let ink_bounds = layout.ink_bounds.map(map).transpose()?;
    let effects = effects
        .iter()
        .map(|e| crate::dag::map_effect(e, transform))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LayoutValue {
        layout_bounds,
        ink_bounds,
        visual_bounds: apply_effects(ink_bounds, &effects)?,
    })
}

pub(crate) fn check_overflow(node: &NodeKey, layout: &LayoutResult) -> Result<(), RenderError> {
    if let Some((line, value)) = layout.lines.iter().enumerate().find(|(_, l)| l.overflow) {
        return Err(RenderError::LayoutOverflow {
            node: node.node,
            instance_path: node.instance_path.clone(),
            line,
            advance: value.advance,
            wrap_width: layout.layout_bounds.max[0],
        });
    }
    Ok(())
}

/// Shared conservative cap/join support for semantic and output-pixel bounds.
pub(crate) fn stroke_halo(width: f64, join: StrokeJoin, cap: StrokeCap, limit: f64) -> f64 {
    let join = if join == StrokeJoin::Miter {
        limit
    } else {
        1.0
    };
    let cap = if cap == StrokeCap::Square {
        std::f64::consts::SQRT_2
    } else {
        1.0
    };
    width * 0.5 * join.max(cap)
}

pub(crate) fn derive_scene_bounds(nodes: &mut [SceneNodeIr]) -> Result<(), RenderError> {
    let indices: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.key.clone(), i))
        .collect();
    for i in (0..nodes.len()).rev() {
        let n = &nodes[i];
        let mut value = match &n.content {
            SceneContent::Caption(caption) => {
                // Text-local glyphs plus the placement translation into root
                // space. Background fills the whole block; the outline ring and
                // synthesized bold/italic expand the ink conservatively.
                let to_root =
                    Affine2([[1.0, 0.0, caption.origin[0]], [0.0, 1.0, caption.origin[1]]]);
                let transform = n.world_transform.compose(to_root);
                let map = |b: kronello_text::Bounds| DesignBounds::checked(b.min, b.max);
                let layout = map(caption.layout.layout_bounds)?;
                let mut ink = caption.layout.ink_bounds.map(map).transpose()?;
                let mut halo = caption.outline.as_ref().map_or(0.0, |o| o.width.get());
                if caption.span_flags.iter().any(|f| f.bold) {
                    halo += caption.bold_width;
                }
                if let Some(bounds) = ink {
                    let mut bounds = bounds.expand(halo)?;
                    if caption.span_flags.iter().any(|f| f.italic) {
                        let lean = kronello_model::CAPTION_ITALIC_SHEAR
                            * (bounds.max[1] - bounds.min[1]).max(0.0);
                        bounds = DesignBounds::checked(
                            bounds.min,
                            [bounds.max[0] + lean, bounds.max[1]],
                        )?;
                    }
                    ink = Some(bounds);
                }
                if caption.background.is_some() {
                    ink = Some(match ink {
                        Some(b) => b.union(layout),
                        None => layout,
                    });
                }
                let map_t = |b: Option<DesignBounds>| b.map(|b| b.transform(transform)).transpose();
                LayoutValue {
                    layout_bounds: map_t(Some(layout))?,
                    ink_bounds: map_t(ink)?,
                    visual_bounds: map_t(ink)?,
                }
            }
            SceneContent::Text(layout) => text_bounds(layout, n.world_transform, &[])?,
            SceneContent::Shape { resolved, .. } => {
                let geometry = kronello_vector::geometry_bounds(&resolved.geometry)?
                    .map(|(min, max)| DesignBounds::checked(min, max))
                    .transpose()?;
                let halo = resolved.stroke.as_ref().map_or(0.0, |s| {
                    if let Some(o) = &s.options {
                        let factor = match o.alignment {
                            kronello_model::StrokeAlignment::Center => 1.0,
                            kronello_model::StrokeAlignment::Inside => 0.0,
                            kronello_model::StrokeAlignment::Outside => 2.0,
                        };
                        return factor
                            * stroke_halo(s.width.get(), s.join, s.cap, s.miter_limit.get());
                    }
                    // Rectangles/ellipses stay in their size box with half-width support.
                    if matches!(
                        resolved.geometry,
                        ResolvedGeometry::BezierPath(_) | ResolvedGeometry::TrimmedPath { .. }
                    ) {
                        stroke_halo(s.width.get(), s.join, s.cap, s.miter_limit.get())
                    } else {
                        s.width.get() * 0.5
                    }
                });
                let ink = if resolved.fill.is_some()
                    || resolved
                        .stroke
                        .as_ref()
                        .is_some_and(|s| s.width.get() > 0.0)
                {
                    geometry.map(|b| b.expand(halo)).transpose()?
                } else {
                    None
                };
                let map = |b: DesignBounds| b.transform(n.world_transform);
                LayoutValue {
                    layout_bounds: geometry.map(map).transpose()?,
                    ink_bounds: ink.map(map).transpose()?,
                    visual_bounds: ink.map(map).transpose()?,
                }
            }
            SceneContent::Video { extent, .. } => {
                let bounds =
                    Some(DesignBounds::checked([0.0; 2], *extent)?.transform(n.world_transform)?);
                LayoutValue {
                    layout_bounds: bounds,
                    ink_bounds: bounds,
                    visual_bounds: bounds,
                }
            }
            // FX-007: the node itself draws nothing; a post-pass below unions
            // the lower-track composite and applies its effect halo.
            SceneContent::Adjustment => LayoutValue::default(),
            SceneContent::Empty => LayoutValue::default(),
        };
        for child in nodes
            .iter()
            .filter(|child| child.parent.as_ref() == Some(&n.key))
        {
            value = value.union(child.bounds);
        }
        let effects = n
            .effects
            .iter()
            .map(|e| crate::dag::map_effect(e, n.world_transform))
            .collect::<Result<Vec<_>, _>>()?;
        value.visual_bounds = apply_effects(value.visual_bounds, &effects)?;
        nodes[i].bounds = value;
        // Bounds derivation relies on the compiler's containment pre-order.
        if nodes[i].parent.as_ref().is_some_and(|p| indices[p] >= i) {
            return Err(RenderError::InvalidInput(
                "invalid bounds containment order".into(),
            ));
        }
    }
    // FX-007: an adjustment node's coverage is the composited visual bounds of
    // every root sibling below it, expanded by its own effect stack. Scene
    // nodes retain authored order, so the running union tracks "lower" roots.
    let mut lower = LayoutValue::default();
    for n in nodes.iter_mut() {
        if n.parent.is_some() {
            continue;
        }
        if matches!(n.content, SceneContent::Adjustment) {
            let effects = n
                .effects
                .iter()
                .map(|e| crate::dag::map_effect(e, n.world_transform))
                .collect::<Result<Vec<_>, _>>()?;
            n.bounds = LayoutValue {
                layout_bounds: lower.layout_bounds,
                ink_bounds: lower.ink_bounds,
                visual_bounds: apply_effects(lower.visual_bounds, &effects)?,
            };
        }
        lower = lower.union(n.bounds);
    }
    Ok(())
}

#[cfg(test)]
mod affine_tests {
    use super::*;
    #[test]
    fn fx002_analytic_support_and_offset_follow_shear() {
        let effect = ResolvedEffect::AffineDropShadow {
            sigma: 2.0,
            linear: [[1.0, 0.0], [0.0, 1.0]],
            offset: [2.0, -1.0],
            color: kronello_model::Color::from_srgb8([0; 3], None),
            opacity: 0.5,
        };
        let mapped =
            crate::dag::map_effect(&effect, Affine2([[2.0, 1.0, 50.0], [0.0, 1.0, 99.0]])).unwrap();
        let ResolvedEffect::AffineDropShadow { offset, linear, .. } = mapped else {
            panic!()
        };
        assert_eq!(offset, [3.0, -1.0]);
        assert_eq!(linear, [[2.0, 1.0], [0.0, 1.0]]);
        let input = DesignBounds {
            min: [0.0; 2],
            max: [10.0; 2],
        };
        let b = apply_effects(Some(input), &[mapped]).unwrap().unwrap();
        assert_eq!(b.min, [3.0 - 6.0 * 5.0_f64.sqrt(), -7.0]);
        assert_eq!(b.max, [13.0 + 6.0 * 5.0_f64.sqrt(), 15.0]);
    }
}
