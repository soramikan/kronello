//! FX-004 clip-local Bezier masks (ADR-0114). Masks rasterize into the owning
//! clip's alpha after content drawing and before clip effects; unlike document
//! mattes they are a per-clip stack, never an inter-node relation.
use crate::{
    DescriptorDefinition, DescriptorId, FiniteF64, MaskId, NumericRange, Path, Property,
    PropertyDescriptor, PropertyId, SchemaKey, SchemaRegistry, Unit, Value, ValueRange,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const MASK_VERSION: u32 = 1;
/// Authored mask stack limits; rasterizer work stays bounded per clip.
pub const MASKS_PER_CLIP_MAX: usize = 64;
/// Anchor/control points in one mask path and across a clip's complete stack.
pub const MASK_PATH_POINTS_MAX: usize = 1024;
pub const CLIP_MASK_POINTS_MAX: usize = 8192;

/// Fixed UUID v4 identity for `kronello.mask.path`.
pub const MASK_PATH_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xf0000000_0010_4400_8000_000000000001));
/// Fixed UUID v4 identity for `kronello.mask.feather`.
pub const MASK_FEATHER_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xf0000000_0010_4400_8000_000000000002));
/// Fixed UUID v4 identity for `kronello.mask.expansion`.
pub const MASK_EXPANSION_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xf0000000_0010_4400_8000_000000000003));
/// Fixed UUID v4 identity for `kronello.mask.opacity`.
pub const MASK_OPACITY_ID: DescriptorId =
    DescriptorId::from_uuid(Uuid::from_u128(0xf0000000_0010_4400_8000_000000000004));

pub const MASK_PATH_KEY: &str = "kronello.mask.path";
pub const MASK_FEATHER_KEY: &str = "kronello.mask.feather";
pub const MASK_EXPANSION_KEY: &str = "kronello.mask.expansion";
pub const MASK_OPACITY_KEY: &str = "kronello.mask.opacity";

/// Stack combine order is authored order; coverage composes over the clip's
/// accumulated alpha. `Add` unions coverage, `Subtract` removes it, `Intersect`
/// keeps the overlap, and `Difference` keeps the symmetric difference.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    #[default]
    Add,
    Subtract,
    Intersect,
    Difference,
}

fn mask_closed_default() -> bool {
    true
}

/// One authored mask row. Every parameter is a Property reference so paths and
/// scalars share the existing constant/curve/expression evaluation, evaluated
/// in sequence time on the clip's lowered node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    pub id: MaskId,
    /// Bezier path in clip-local design_px; `ValueType::Path` property.
    pub path: PropertyId,
    pub mode: MaskMode,
    /// Edge blur sigma in clip-local design_px; nonnegative scalar.
    pub feather: PropertyId,
    /// Outward (positive) or inward (negative) path offset in clip-local
    /// design_px.
    pub expansion: PropertyId,
    /// Coverage multiplier in [0,1].
    pub opacity: PropertyId,
    /// Inverts this mask's own coverage before the mode combine.
    #[serde(default)]
    pub invert: bool,
    /// Authored path closure. `closed` masks offset as rings; open masks offset
    /// as open chains. Fill coverage itself always closes subpaths.
    #[serde(default = "mask_closed_default")]
    pub closed: bool,
}

/// Evaluated mask at one instant; the rasterizable counterpart of [`Mask`].
/// `id` retains authored identity for coverage-cache provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedMask {
    pub id: MaskId,
    pub mode: MaskMode,
    pub path: Path,
    pub feather: f64,
    pub expansion: f64,
    pub opacity: f64,
    pub invert: bool,
    pub closed: bool,
}

#[derive(Debug, Error)]
pub enum MaskError {
    #[error("mask property {0} is missing or has an incompatible descriptor/source")]
    InvalidProperty(PropertyId),
    #[error("mask id {0} repeats inside one clip's mask stack")]
    DuplicateId(MaskId),
    #[error("mask path topology or budget invalid")]
    InvalidPath,
    #[error("evaluated mask scalar {0} is outside its declared range")]
    OutOfRange(PropertyId),
    #[error("clip mask budget exceeded")]
    BudgetExceeded,
}
impl MaskError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidProperty(_) => "MASK_INVALID_PROPERTY",
            Self::DuplicateId(_) => "MASK_DUPLICATE_ID",
            Self::InvalidPath => "MASK_INVALID_PATH",
            Self::OutOfRange(_) => "MASK_OUT_OF_RANGE",
            Self::BudgetExceeded => "MASK_BUDGET_EXCEEDED",
        }
    }
}
impl From<MaskError> for crate::SequenceError {
    fn from(error: MaskError) -> Self {
        crate::SequenceError::Invalid(format!("{}: {error}", error.code()))
    }
}

/// Anchor/control point count of a bezier path: MoveTo/LineTo cost one,
/// quadratic two, cubic three, and Close is free.
pub fn mask_path_points(path: &Path) -> usize {
    path.segments
        .iter()
        .map(|segment| match segment {
            crate::PathSegment::MoveTo(_) | crate::PathSegment::LineTo(_) => 1,
            crate::PathSegment::QuadTo { .. } => 2,
            crate::PathSegment::CubicTo { .. } => 3,
            crate::PathSegment::Close => 0,
        })
        .sum()
}

/// Repeatable clip properties backing one [`Mask`] row. Fixed identities keep
/// every authoring surface aligned with the wire schema.
pub fn mask_descriptors() -> Vec<PropertyDescriptor> {
    let zero = FiniteF64::new(0.0).expect("finite mask default");
    let one = FiniteF64::new(1.0).expect("finite mask default");
    let definition = |id, key, name, unit, default: Value| {
        DescriptorDefinition::new(
            id,
            SchemaKey::new(key).expect("valid mask key"),
            name,
            default.value_type(),
            unit,
            default,
        )
    };
    let path = definition(
        MASK_PATH_ID,
        MASK_PATH_KEY,
        "Mask path",
        Unit::DesignPx,
        Value::Path(Path { segments: vec![] }),
    );
    let mut feather = definition(
        MASK_FEATHER_ID,
        MASK_FEATHER_KEY,
        "Mask feather",
        Unit::DesignPx,
        Value::Scalar(zero),
    );
    feather.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(0.0, 1_000_000.0).expect("valid feather range"),
    ));
    let mut expansion = definition(
        MASK_EXPANSION_ID,
        MASK_EXPANSION_KEY,
        "Mask expansion",
        Unit::DesignPx,
        Value::Scalar(zero),
    );
    expansion.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(-1_000_000.0, 1_000_000.0).expect("valid expansion range"),
    ));
    let mut opacity = definition(
        MASK_OPACITY_ID,
        MASK_OPACITY_KEY,
        "Mask opacity",
        Unit::Dimensionless,
        Value::Scalar(one),
    );
    opacity.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(0.0, 1.0).expect("valid opacity range"),
    ));
    [path, feather, expansion, opacity]
        .into_iter()
        .map(|mut definition| {
            definition.repeatable = true;
            PropertyDescriptor::new(definition).expect("valid mask descriptor")
        })
        .collect()
}

impl Mask {
    /// The four property references with their required descriptor keys.
    fn references(&self) -> [(&'static str, PropertyId); 4] {
        [
            (MASK_PATH_KEY, self.path),
            (MASK_FEATHER_KEY, self.feather),
            (MASK_EXPANSION_KEY, self.expansion),
            (MASK_OPACITY_KEY, self.opacity),
        ]
    }
    /// Property existence, descriptor identity, and intrinsic value/range
    /// checks. Constant paths additionally carry topology and point budgets so
    /// authoring failures surface at edit time, not render time.
    pub fn validate(
        &self,
        properties: &[Property],
        registry: &SchemaRegistry,
    ) -> Result<(), MaskError> {
        for (key, id) in self.references() {
            let property = properties
                .iter()
                .find(|p| p.id() == id)
                .ok_or(MaskError::InvalidProperty(id))?;
            if property.descriptor().key.as_str() != key {
                return Err(MaskError::InvalidProperty(id));
            }
            property
                .validate(registry)
                .map_err(|_| MaskError::InvalidProperty(id))?;
            if key == MASK_PATH_KEY
                && let crate::PropertySource::Constant(Value::Path(path)) = property.source()
            {
                crate::validate_path(path).map_err(|_| MaskError::InvalidPath)?;
                if mask_path_points(path) > MASK_PATH_POINTS_MAX {
                    return Err(MaskError::BudgetExceeded);
                }
            }
        }
        Ok(())
    }
    /// Evaluate at one instant from the owning node's resolved property values.
    pub fn resolve(&self, values: &BTreeMap<PropertyId, Value>) -> Result<ResolvedMask, MaskError> {
        let scalar = |id: PropertyId| match values.get(&id) {
            Some(Value::Scalar(v)) => Ok(v.get()),
            _ => Err(MaskError::InvalidProperty(id)),
        };
        let path = match values.get(&self.path) {
            Some(Value::Path(path)) => path.clone(),
            _ => return Err(MaskError::InvalidProperty(self.path)),
        };
        crate::validate_path(&path).map_err(|_| MaskError::InvalidPath)?;
        if mask_path_points(&path) > MASK_PATH_POINTS_MAX {
            return Err(MaskError::BudgetExceeded);
        }
        let feather = scalar(self.feather)?;
        let expansion = scalar(self.expansion)?;
        let opacity = scalar(self.opacity)?;
        // Curve/expression outputs reach evaluation unchecked, so the ADR
        // invariants (nonnegative feather, opacity in [0,1]) are enforced here.
        if feather < 0.0 {
            return Err(MaskError::OutOfRange(self.feather));
        }
        if !(0.0..=1.0).contains(&opacity) {
            return Err(MaskError::OutOfRange(self.opacity));
        }
        Ok(ResolvedMask {
            id: self.id,
            mode: self.mode,
            path,
            feather,
            expansion,
            opacity,
            invert: self.invert,
            closed: self.closed,
        })
    }
}

/// The complete authored mask stack of one clip: unique ids, per-mask
/// validation, and stack/count budgets. Mask parameters are clip-owned
/// properties, so their identity checks run in the caller's property pass.
pub fn validate_clip_masks(
    masks: &[Mask],
    properties: &[Property],
    registry: &SchemaRegistry,
) -> Result<(), MaskError> {
    if masks.len() > MASKS_PER_CLIP_MAX {
        return Err(MaskError::BudgetExceeded);
    }
    let mut ids = BTreeSet::new();
    let mut points = 0usize;
    for mask in masks {
        if !ids.insert(mask.id) {
            return Err(MaskError::DuplicateId(mask.id));
        }
        mask.validate(properties, registry)?;
        // Constant paths contribute their authored point count to the clip
        // total; animated paths are budgeted again at evaluation.
        if let Some(crate::PropertySource::Constant(Value::Path(path))) = properties
            .iter()
            .find(|p| p.id() == mask.path)
            .map(|p| p.source())
        {
            points += mask_path_points(path);
            if points > CLIP_MASK_POINTS_MAX {
                return Err(MaskError::BudgetExceeded);
            }
        }
    }
    Ok(())
}
