use crate::{
    CurveId, ExpressionId, JsonError, ModelError, ModifierId, PropertyDescriptor, PropertyId,
    SchemaKey, SchemaRegistry, Value, ValueType,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One source at a time, in memory and JSON. Adjacent tagging represents scalar
/// constants and UUID references uniformly, while deny_unknown_fields rejects
/// competing payloads rather than applying an implicit priority.
///
/// ```compile_fail
/// use kronello_model::PropertySource;
/// let source: PropertySource<bool> = PropertySource::Constant("wrong type");
/// ```
///
/// ```compile_fail
/// use kronello_model::{CurveId, PropertySource};
/// let source = PropertySource::<bool> {
///     constant: true,
///     curve: CurveId::new(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PropertySource<T> {
    Constant(T),
    Curve(CurveId),
    Expression(ExpressionId),
}

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for PropertySource<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = crate::wire::Adjacent::deserialize(d)?;
        match wire.kind.as_str() {
            "constant" => wire.value().map(Self::Constant),
            "curve" => wire.value().map(Self::Curve),
            "expression" => wire.value().map(Self::Expression),
            _ => Err(wire.unknown(&["constant", "curve", "expression"])),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescriptorRef {
    pub key: SchemaKey,
    pub version: u32,
}
impl DescriptorRef {
    pub fn new(descriptor: &PropertyDescriptor) -> Self {
        Self {
            key: descriptor.key().clone(),
            version: descriptor.version(),
        }
    }
    pub fn resolve<'a>(
        &self,
        registry: &'a SchemaRegistry,
    ) -> Result<&'a PropertyDescriptor, ModelError> {
        let descriptor = registry.lookup(&self.key)?;
        if descriptor.version() != self.version {
            return Err(ModelError::DescriptorVersionMismatch {
                key: self.key.clone(),
                requested: self.version,
                registered: descriptor.version(),
            });
        }
        Ok(descriptor)
    }
}

/// Ordered modifier frame only, with semantic parameters and a stable identity.
/// No modifier algorithm or execution capability is implemented here. The
/// animation compiler must resolve kind/version and reject unsupported kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Modifier {
    pub id: ModifierId,
    pub key: SchemaKey,
    pub version: u32,
    pub enabled: bool,
    pub parameters: BTreeMap<String, Value>,
}

/// Source catalogs supply metadata, without evaluation or a backend dependency.
pub trait SourceResolver {
    fn curve_value_type(&self, id: CurveId) -> Option<ValueType>;
    fn expression_value_type(&self, id: ExpressionId) -> Option<ValueType>;
}

/// Deserialization checks intrinsic invariants only; use from_json(input,
/// registry) or validate(registry) to resolve the descriptor before accepting
/// an imported property. Source catalogs are checked by validate_sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "PropertyWire", into = "PropertyWire")]
pub struct Property {
    id: PropertyId,
    descriptor: DescriptorRef,
    source: PropertySource<Value>,
    modifiers: Vec<Modifier>,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct PropertyWire {
    id: PropertyId,
    descriptor: DescriptorRef,
    source: PropertySource<Value>,
    modifiers: Vec<Modifier>,
}
impl Property {
    pub fn new(
        id: PropertyId,
        descriptor: DescriptorRef,
        source: PropertySource<Value>,
        modifiers: Vec<Modifier>,
        registry: &SchemaRegistry,
    ) -> Result<Self, ModelError> {
        let property = Self {
            id,
            descriptor,
            source,
            modifiers,
        };
        property.validate(registry)?;
        Ok(property)
    }
    pub fn from_json(input: &str, registry: &SchemaRegistry) -> Result<Self, JsonError> {
        let wire: PropertyWire = crate::from_json(input)?;
        let property = Self::try_from(wire)?;
        property.validate(registry)?;
        Ok(property)
    }
    pub fn id(&self) -> PropertyId {
        self.id
    }
    pub fn descriptor(&self) -> &DescriptorRef {
        &self.descriptor
    }
    pub fn source(&self) -> &PropertySource<Value> {
        &self.source
    }
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    fn validate_intrinsic(&self) -> Result<(), ModelError> {
        let mut ids = BTreeSet::new();
        for modifier in &self.modifiers {
            if modifier.version == 0 {
                return Err(ModelError::InvalidModifierVersion);
            }
            if !ids.insert(modifier.id) {
                return Err(ModelError::DuplicateModifierId);
            }
        }
        Ok(())
    }
    fn validate_source(
        &self,
        source: &PropertySource<Value>,
        descriptor: &PropertyDescriptor,
    ) -> Result<(), ModelError> {
        let definition = descriptor.definition();
        match source {
            PropertySource::Constant(value) => {
                descriptor.validate_value_type(value)?;
                if descriptor.key().as_str() == crate::BLEND_KEY {
                    crate::BlendMode::from_value(value)?;
                }
                // ADR-0043 validates range after modifiers. A constant is the
                // final value only when there are no enabled modifiers.
                if !self.modifiers.iter().any(|modifier| modifier.enabled) {
                    descriptor.validate_value(value)?;
                }
            }
            PropertySource::Curve(_)
                if !definition.animatable || !definition.capabilities.curves =>
            {
                return Err(ModelError::SourceNotAllowed);
            }
            PropertySource::Expression(_)
                if !definition.animatable || !definition.capabilities.expressions =>
            {
                return Err(ModelError::SourceNotAllowed);
            }
            _ => (),
        }
        Ok(())
    }
    pub fn validate(&self, registry: &SchemaRegistry) -> Result<(), ModelError> {
        self.validate_intrinsic()?;
        let descriptor = self.descriptor.resolve(registry)?;
        if !self.modifiers.is_empty() && !descriptor.definition().capabilities.modifiers {
            return Err(ModelError::ModifiersNotAllowed);
        }
        self.validate_source(&self.source, descriptor)
    }
    pub fn validate_sources(
        &self,
        registry: &SchemaRegistry,
        resolver: &impl SourceResolver,
    ) -> Result<(), ModelError> {
        self.validate(registry)?;
        let expected = self.descriptor.resolve(registry)?.definition().value_type;
        let actual = match self.source {
            PropertySource::Constant(_) => return Ok(()),
            PropertySource::Curve(id) => resolver
                .curve_value_type(id)
                .ok_or(ModelError::CurveNotFound { id })?,
            PropertySource::Expression(id) => resolver
                .expression_value_type(id)
                .ok_or(ModelError::ExpressionNotFound { id })?,
        };
        if actual != expected {
            return Err(ModelError::ValueTypeMismatch { expected, actual });
        }
        Ok(())
    }
    /// Atomic replacement: rejected sources preserve the previous source.
    pub fn set_source(
        &mut self,
        source: PropertySource<Value>,
        registry: &SchemaRegistry,
    ) -> Result<(), ModelError> {
        let candidate = Self {
            source,
            ..self.clone()
        };
        candidate.validate(registry)?;
        self.source = candidate.source;
        Ok(())
    }
    pub fn set_modifiers(
        &mut self,
        modifiers: Vec<Modifier>,
        registry: &SchemaRegistry,
    ) -> Result<(), ModelError> {
        let candidate = Self {
            modifiers,
            ..self.clone()
        };
        candidate.validate(registry)?;
        self.modifiers = candidate.modifiers;
        Ok(())
    }
    /// Evaluation layers must call this on the result after the entire ordered
    /// modifier chain; validating the source alone is insufficient.
    pub fn validate_final_value(
        &self,
        value: &Value,
        registry: &SchemaRegistry,
    ) -> Result<(), ModelError> {
        self.validate(registry)?;
        self.descriptor.resolve(registry)?.validate_value(value)
    }
}
impl TryFrom<PropertyWire> for Property {
    type Error = ModelError;
    fn try_from(wire: PropertyWire) -> Result<Self, Self::Error> {
        let property = Self {
            id: wire.id,
            descriptor: wire.descriptor,
            source: wire.source,
            modifiers: wire.modifiers,
        };
        property.validate_intrinsic()?;
        Ok(property)
    }
}
impl From<Property> for PropertyWire {
    fn from(p: Property) -> Self {
        Self {
            id: p.id,
            descriptor: p.descriptor,
            source: p.source,
            modifiers: p.modifiers,
        }
    }
}
