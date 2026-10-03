use crate::{ColorSpace, DescriptorId, FiniteF64, ModelError, SchemaKey, Value, ValueType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Dimensionless,
    DesignPx,
    Degrees,
}
impl Unit {
    /// Times and durations use kronello-time's rational types, not Scalar/f64.
    pub fn validate(self, value_type: ValueType) -> Result<(), ModelError> {
        let compatible = match value_type {
            ValueType::Angle => self == Self::Degrees,
            ValueType::Path => self == Self::DesignPx,
            ValueType::Scalar | ValueType::Vec2 | ValueType::Vec3 => {
                matches!(self, Self::Dimensionless | Self::DesignPx)
            }
            _ => self == Self::Dimensionless,
        };
        if compatible {
            Ok(())
        } else {
            Err(ModelError::IncompatibleUnit {
                value_type,
                unit: self,
            })
        }
    }
}

/// Design coordinates in node content, its transform parent, or the Composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSpace {
    /// Node content coordinates before anchor and transform application.
    LocalDesign,
    /// Transform parent coordinates; the Composition space for an unparented node.
    ParentDesign,
    /// Absolute coordinates in the Composition's design rectangle.
    CompositionDesign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMode {
    Hold,
    Linear,
    Cubic,
}

impl ValueType {
    /// Type capability only; no curve evaluation takes place in this crate.
    /// Path morph requires topology validation in a later layer and is not
    /// advertised as an unconditional numeric interpolation capability.
    pub const fn interpolation_modes(self) -> &'static [InterpolationMode] {
        use InterpolationMode::{Cubic, Hold, Linear};
        match self {
            Self::Scalar | Self::Vec2 | Self::Vec3 | Self::Angle | Self::Color => {
                &[Hold, Linear, Cubic]
            }
            _ => &[Hold],
        }
    }
    pub fn validate_interpolation(self, mode: InterpolationMode) -> Result<(), ModelError> {
        if self.interpolation_modes().contains(&mode) {
            Ok(())
        } else {
            Err(ModelError::IncompatibleInterpolation {
                value_type: self,
                mode,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericBound {
    pub value: FiniteF64,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericRange {
    pub min: Option<NumericBound>,
    pub max: Option<NumericBound>,
}
impl NumericRange {
    pub fn inclusive(min: f64, max: f64) -> Result<Self, ModelError> {
        let range = Self {
            min: Some(NumericBound {
                value: FiniteF64::new(min)?,
                inclusive: true,
            }),
            max: Some(NumericBound {
                value: FiniteF64::new(max)?,
                inclusive: true,
            }),
        };
        range.validate()?;
        Ok(range)
    }
    pub fn validate(self) -> Result<(), ModelError> {
        if let (Some(min), Some(max)) = (self.min, self.max)
            && (min.value > max.value
                || (min.value == max.value && (!min.inclusive || !max.inclusive)))
        {
            return Err(ModelError::InvalidRange);
        }
        Ok(())
    }
    pub fn validate_component(self, value: FiniteF64, component: usize) -> Result<(), ModelError> {
        self.validate()?;
        if self
            .min
            .is_some_and(|min| value < min.value || (value == min.value && !min.inclusive))
            || self
                .max
                .is_some_and(|max| value > max.value || (value == max.value && !max.inclusive))
        {
            return Err(ModelError::OutOfRange {
                component,
                value: value.get(),
            });
        }
        Ok(())
    }
}

/// Vector bounds are declared per component, without scalar broadcasting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ValueRange {
    Scalar(NumericRange),
    Vec2([NumericRange; 2]),
    Vec3([NumericRange; 3]),
    Angle(NumericRange),
}
impl ValueRange {
    pub fn validate_type(&self, value_type: ValueType) -> Result<(), ModelError> {
        let ranges: &[NumericRange] = match (self, value_type) {
            (Self::Scalar(range), ValueType::Scalar) | (Self::Angle(range), ValueType::Angle) => {
                std::slice::from_ref(range)
            }
            (Self::Vec2(ranges), ValueType::Vec2) => ranges,
            (Self::Vec3(ranges), ValueType::Vec3) => ranges,
            _ => return Err(ModelError::IncompatibleRange { value_type }),
        };
        for range in ranges {
            range.validate()?;
        }
        Ok(())
    }
    pub fn validate_value(&self, value: &Value) -> Result<(), ModelError> {
        self.validate_type(value.value_type())?;
        let (ranges, values): (&[NumericRange], &[FiniteF64]) = match (self, value) {
            (Self::Scalar(range), Value::Scalar(value))
            | (Self::Angle(range), Value::Angle(value)) => {
                (std::slice::from_ref(range), std::slice::from_ref(value))
            }
            (Self::Vec2(ranges), Value::Vec2(values)) => (ranges, values),
            (Self::Vec3(ranges), Value::Vec3(values)) => (ranges, values),
            _ => {
                return Err(ModelError::IncompatibleRange {
                    value_type: value.value_type(),
                });
            }
        };
        for (component, (range, value)) in ranges.iter().zip(values).enumerate() {
            range.validate_component(*value, component)?;
        }
        Ok(())
    }
}

/// Allowed document features, not a claim that an evaluator supports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub curves: bool,
    pub expressions: bool,
    pub modifiers: bool,
}

/// Editable schema input. Convert to PropertyDescriptor before registration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescriptorDefinition {
    pub id: DescriptorId,
    pub key: SchemaKey,
    pub name: String,
    pub value_type: ValueType,
    pub unit: Unit,
    pub coordinate_space: Option<CoordinateSpace>,
    pub interpolation_modes: BTreeSet<InterpolationMode>,
    /// For Color, None selects the target Sequence's working linear space;
    /// Some selects an explicit linear space. Straight RGB and alpha interpolate
    /// independently. Other types require None. Rec.2020 is not HDR support.
    pub color_interpolation_space: Option<ColorSpace>,
    pub range: Option<ValueRange>,
    pub default: Value,
    pub animatable: bool,
    pub capabilities: Capabilities,
    /// This descriptor structure/contract supports version 1. Global snapshot
    /// schema_version and semantic_versions belong to STORE-001.
    pub version: u32,
}
impl DescriptorDefinition {
    pub fn new(
        id: DescriptorId,
        key: SchemaKey,
        name: impl Into<String>,
        value_type: ValueType,
        unit: Unit,
        default: Value,
    ) -> Self {
        Self {
            id,
            key,
            name: name.into(),
            value_type,
            unit,
            coordinate_space: if unit == Unit::DesignPx
                && matches!(
                    value_type,
                    ValueType::Vec2 | ValueType::Vec3 | ValueType::Path
                ) {
                Some(CoordinateSpace::LocalDesign)
            } else {
                None
            },
            interpolation_modes: value_type.interpolation_modes().iter().copied().collect(),
            color_interpolation_space: None,
            range: None,
            default,
            animatable: true,
            capabilities: Capabilities {
                curves: true,
                expressions: true,
                modifiers: true,
            },
            version: 1,
        }
    }
}

/// Validated immutable descriptor. Display labels can change; ID, key, version,
/// and all semantic metadata stay fixed after registration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DescriptorDefinition", into = "DescriptorDefinition")]
pub struct PropertyDescriptor(DescriptorDefinition);
impl PropertyDescriptor {
    /// Import boundary preserving domain validation errors separately from
    /// incompatible JSON structure errors.
    pub fn from_json(input: &str) -> Result<Self, crate::JsonError> {
        let definition: DescriptorDefinition = crate::from_json(input)?;
        Ok(Self::new(definition)?)
    }
    pub fn new(definition: DescriptorDefinition) -> Result<Self, ModelError> {
        Self::try_from(definition)
    }
    pub fn definition(&self) -> &DescriptorDefinition {
        &self.0
    }
    pub fn id(&self) -> DescriptorId {
        self.0.id
    }
    pub fn key(&self) -> &SchemaKey {
        &self.0.key
    }
    pub fn name(&self) -> &str {
        &self.0.name
    }
    pub fn version(&self) -> u32 {
        self.0.version
    }
    pub fn rename(&mut self, name: impl Into<String>) {
        self.0.name = name.into();
    }
    pub fn validate_value_type(&self, value: &Value) -> Result<(), ModelError> {
        let actual = value.value_type();
        if actual != self.0.value_type {
            return Err(ModelError::ValueTypeMismatch {
                expected: self.0.value_type,
                actual,
            });
        }
        Ok(())
    }
    /// Validate the final value after all explicit modifiers, with no clamping.
    pub fn validate_value(&self, value: &Value) -> Result<(), ModelError> {
        self.validate_value_type(value)?;
        if let Some(range) = &self.0.range {
            range.validate_value(value)?;
        }
        Ok(())
    }
}
impl TryFrom<DescriptorDefinition> for PropertyDescriptor {
    type Error = ModelError;
    fn try_from(d: DescriptorDefinition) -> Result<Self, Self::Error> {
        if d.version != 1 {
            return Err(ModelError::UnsupportedDescriptorVersion { version: d.version });
        }
        d.unit.validate(d.value_type)?;
        if d.coordinate_space.is_some()
            && (d.unit != Unit::DesignPx
                || !matches!(
                    d.value_type,
                    ValueType::Vec2 | ValueType::Vec3 | ValueType::Path
                ))
            || (d.coordinate_space.is_none()
                && d.unit == Unit::DesignPx
                && matches!(
                    d.value_type,
                    ValueType::Vec2 | ValueType::Vec3 | ValueType::Path
                ))
        {
            return Err(ModelError::IncompatibleCoordinateSpace);
        }
        if !d.animatable && (d.capabilities.curves || d.capabilities.expressions) {
            return Err(ModelError::IncompatibleCapabilities);
        }
        if d.animatable && d.interpolation_modes.is_empty() {
            return Err(ModelError::MissingInterpolation);
        }
        for mode in &d.interpolation_modes {
            d.value_type.validate_interpolation(*mode)?;
        }
        match (d.value_type, d.color_interpolation_space) {
            (ValueType::Color, None) => (),
            (ValueType::Color, Some(ColorSpace::LinearRec709 | ColorSpace::LinearRec2020)) => (),
            (ValueType::Color, _) => return Err(ModelError::InvalidColorInterpolationSpace),
            (_, None) => (),
            _ => return Err(ModelError::InvalidColorInterpolationSpace),
        }
        if let Some(range) = &d.range {
            range.validate_type(d.value_type)?;
        }
        let descriptor = Self(d);
        descriptor.validate_value(&descriptor.0.default)?;
        Ok(descriptor)
    }
}
impl From<PropertyDescriptor> for DescriptorDefinition {
    fn from(d: PropertyDescriptor) -> Self {
        d.0
    }
}

/// Keys enumerate in lexical order regardless of registration order. Entries
/// cannot be replaced or silently deduplicated. Only display labels can change.
#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    descriptors: BTreeMap<SchemaKey, PropertyDescriptor>,
}
impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    /// Register the standard transform, opacity, fill color, and stroke width
    /// descriptors with fixed UUID v4 identities and lexical enumeration order.
    pub fn with_builtin() -> Self {
        crate::builtin::registry()
    }
    pub fn register(&mut self, descriptor: PropertyDescriptor) -> Result<(), ModelError> {
        if self.descriptors.contains_key(descriptor.key()) {
            return Err(ModelError::DuplicateSchemaKey {
                key: descriptor.key().clone(),
            });
        }
        if self.descriptors.values().any(|d| d.id() == descriptor.id()) {
            return Err(ModelError::DuplicateDescriptorId);
        }
        self.descriptors
            .insert(descriptor.key().clone(), descriptor);
        Ok(())
    }
    pub fn lookup(&self, key: &SchemaKey) -> Result<&PropertyDescriptor, ModelError> {
        self.descriptors
            .get(key)
            .ok_or_else(|| ModelError::DescriptorNotFound { key: key.clone() })
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&SchemaKey, &PropertyDescriptor)> {
        self.descriptors.iter()
    }
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
    pub fn rename(&mut self, key: &SchemaKey, name: impl Into<String>) -> Result<(), ModelError> {
        let descriptor = self
            .descriptors
            .get_mut(key)
            .ok_or_else(|| ModelError::DescriptorNotFound { key: key.clone() })?;
        descriptor.rename(name);
        Ok(())
    }
}
