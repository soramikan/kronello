use crate::{DependencyGraph, EvaluationError, RuntimePropertyKey};
use kronello_model::{
    Expression, ExpressionBudget, ExpressionError, ExpressionNode as N, FiniteF64, InstancePath,
    PropertyKey, Value, expression_value_bytes,
};
use kronello_time::Time;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Usage {
    instructions: usize,
    memory: usize,
    samples: usize,
}
impl Usage {
    pub(crate) fn charge(
        &mut self,
        instructions: usize,
        memory: usize,
        samples: usize,
        limit: ExpressionBudget,
    ) -> Result<(), ExpressionError> {
        self.instructions = self.instructions.saturating_add(instructions);
        self.memory = self.memory.saturating_add(memory);
        self.samples = self.samples.saturating_add(samples);
        for (name, used, maximum) in [
            ("instructions", self.instructions, limit.instructions),
            ("memory_bytes", self.memory, limit.memory_bytes),
            ("samples", self.samples, limit.samples),
        ] {
            if used > maximum {
                return Err(ExpressionError::Budget(name));
            }
        }
        Ok(())
    }
}

impl DependencyGraph<'_> {
    pub(crate) fn expression_key(
        &self,
        scope: &InstancePath,
        node: Option<kronello_model::NodeId>,
        property: kronello_model::PropertyId,
    ) -> RuntimePropertyKey {
        match node {
            Some(node) => PropertyKey {
                instance_path: scope.clone(),
                node,
                property,
            }
            .into(),
            None => RuntimePropertyKey::Composition {
                instance_path: scope.clone(),
                composition: self.scopes[scope].composition.id,
                property,
            },
        }
    }
    pub(crate) fn run_expression(
        &self,
        expression: &Expression,
        entry: &crate::graph::Entry<'_>,
        instance: &InstancePath,
        time: Time,
        upstream: &BTreeMap<RuntimePropertyKey, Value>,
        usage: &mut Usage,
    ) -> Result<Value, ExpressionError> {
        let scope = &entry.source_scope;
        let space = entry
            .property
            .descriptor()
            .resolve(self.registry)
            .expect("compiled descriptor")
            .definition()
            .color_interpolation_space
            .unwrap_or(self.working_space);
        let mut local = Usage::default();
        let initial = expression
            .nodes
            .len()
            .saturating_mul(std::mem::size_of::<Value>());
        local.charge(0, initial, 0, expression.budget)?;
        usage.charge(0, initial, 0, ExpressionBudget::default())?;
        let mut values: Vec<Value> = Vec::with_capacity(expression.nodes.len());
        for (i, node) in expression.nodes.iter().enumerate() {
            let scalar = |index: u32| match values.get(index as usize) {
                Some(Value::Scalar(v)) => Ok(v.get()),
                _ => Err(ExpressionError::TypeMismatch(i)),
            };
            let sampled = usize::from(matches!(node, N::Property { .. } | N::CurveSample { .. }));
            // Charge before any owned payload is cloned or a curve is sampled.
            let work = match node {
                N::CurveSample { curve, .. } => 65 + self.curves[curve].keys().len(),
                N::Noise { .. } => 17 + instance.ids().len().saturating_mul(16),
                N::Property { .. } => 1 + scope.ids().len(),
                _ => 1,
            };
            local.charge(work, 0, sampled, expression.budget)?;
            usage.charge(work, 0, sampled, ExpressionBudget::default())?;
            let payload = match node {
                N::Literal(v) => expression_value_bytes(v),
                N::Property { node, property, .. } => {
                    expression_value_bytes(&upstream[&self.expression_key(scope, *node, *property)])
                        + scope.ids().len().saturating_mul(16)
                        + std::mem::size_of::<RuntimePropertyKey>()
                }
                N::CurveSample { curve, .. } => self.curves[curve]
                    .keys()
                    .iter()
                    .map(|k| expression_value_bytes(&k.value))
                    .max()
                    .unwrap_or(0),
                _ => std::mem::size_of::<Value>(),
            };
            local.charge(0, payload, 0, expression.budget)?;
            usage.charge(0, payload, 0, ExpressionBudget::default())?;
            let number = |v: f64| {
                FiniteF64::new(v)
                    .map(Value::Scalar)
                    .map_err(|_| ExpressionError::Arithmetic("non-finite result"))
            };
            let value = match node {
                N::Literal(v) => v.clone(),
                N::Time => number(time.numerator() as f64 / time.denominator() as f64)?,
                N::Property { node, property, .. } => {
                    upstream[&self.expression_key(scope, *node, *property)].clone()
                }
                N::CurveSample { curve, offset, .. } => {
                    let at = time
                        .checked_add(*offset)
                        .map_err(|_| ExpressionError::Arithmetic("sample time overflow"))?;
                    kronello_animation::sample_in_space(self.curves[curve], at, space)
                        .map_err(|_| ExpressionError::Arithmetic("curve sample failed"))?
                }
                N::Add { left, right } => number(scalar(*left)? + scalar(*right)?)?,
                N::Subtract { left, right } => number(scalar(*left)? - scalar(*right)?)?,
                N::Multiply { left, right } => number(scalar(*left)? * scalar(*right)?)?,
                N::Divide { left, right } => {
                    let divisor = scalar(*right)?;
                    if divisor == 0.0 {
                        return Err(ExpressionError::Arithmetic("division by zero"));
                    }
                    number(scalar(*left)? / divisor)?
                }
                N::Clamp { value, min, max } => {
                    let (min, max) = (scalar(*min)?, scalar(*max)?);
                    if min > max {
                        return Err(ExpressionError::Arithmetic("inverted clamp bounds"));
                    }
                    number(scalar(*value)?.clamp(min, max))?
                }
                N::Lerp { from, to, amount } => number(
                    scalar(*from)? * (1.0 - scalar(*amount)?) + scalar(*to)? * scalar(*amount)?,
                )?,
                N::Sin { input } => number(scalar(*input)?.sin())?,
                N::Vec2 { x, y } => Value::Vec2([
                    FiniteF64::new(scalar(*x)?).unwrap(),
                    FiniteF64::new(scalar(*y)?).unwrap(),
                ]),
                N::Vec3 { x, y, z } => Value::Vec3([
                    FiniteF64::new(scalar(*x)?).unwrap(),
                    FiniteF64::new(scalar(*y)?).unwrap(),
                    FiniteF64::new(scalar(*z)?).unwrap(),
                ]),
                N::Angle { degrees } => Value::Angle(FiniteF64::new(scalar(*degrees)?).unwrap()),
                N::Noise {
                    seed,
                    element,
                    input,
                } => {
                    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
                    let coordinate = scalar(*input)?;
                    // Normalize signed zero, then use explicitly ordered bytes.
                    let bits = if coordinate == 0.0 {
                        0
                    } else {
                        coordinate.to_bits()
                    };
                    for byte in seed
                        .to_le_bytes()
                        .into_iter()
                        .chain(element.to_le_bytes())
                        .chain(bits.to_le_bytes())
                        .chain(
                            instance
                                .ids()
                                .iter()
                                .flat_map(|id| id.as_uuid().into_bytes()),
                        )
                    {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                    }
                    hash ^= hash >> 30;
                    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                    hash ^= hash >> 27;
                    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
                    hash ^= hash >> 31;
                    number((hash >> 11) as f64 / ((1_u64 << 53) - 1) as f64 * 2.0 - 1.0)?
                }
            };
            if let N::Property { value_type, .. } | N::CurveSample { value_type, .. } = node
                && value.value_type() != *value_type
            {
                return Err(ExpressionError::TypeMismatch(i));
            }
            values.push(value);
        }
        Ok(values.pop().expect("compiled nonempty AST"))
    }
}

pub(crate) fn error(key: &RuntimePropertyKey, source: ExpressionError) -> EvaluationError {
    EvaluationError::Expression {
        key: key.clone(),
        source,
    }
}
