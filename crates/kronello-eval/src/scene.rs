use std::collections::BTreeMap;

use kronello_model::{
    CompositionId, InstancePath, NodeId, NodeKind, PropertyKey, SchemaKey, Value,
};
use kronello_time::Time;

use crate::{DependencyGraph, EvaluationError, RuntimePropertyKey};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKey {
    pub instance_path: InstancePath,
    pub node: NodeId,
}

/// Row-major 2x3 affine matrix, applied to column vectors [x,y,1].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine2(pub [[f64; 3]; 2]);
impl Affine2 {
    pub const IDENTITY: Self = Self([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    pub fn transform_point(self, point: [f64; 2]) -> [f64; 2] {
        self.0
            .map(|row| row[0] * point[0] + row[1] * point[1] + row[2])
    }
    /// self * rhs: apply rhs first, then self.
    pub fn compose(self, rhs: Self) -> Self {
        let [a, b] = self.0;
        let [c, d] = rhs.0;
        Self([
            [
                a[0] * c[0] + a[1] * d[0],
                a[0] * c[1] + a[1] * d[1],
                a[0] * c[2] + a[1] * d[2] + a[2],
            ],
            [
                b[0] * c[0] + b[1] * d[0],
                b[0] * c[1] + b[1] * d[1],
                b[0] * c[2] + b[1] * d[2] + b[2],
            ],
        ])
    }
    fn finite(self, node: &NodeKey) -> Result<Self, EvaluationError> {
        if self.0.iter().flatten().all(|x| x.is_finite()) {
            Ok(self)
        } else {
            Err(EvaluationError::NonFiniteTransform { node: node.clone() })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformValues {
    pub position: [f64; 2],
    pub anchor: [f64; 2],
    pub rotation: f64,
    pub scale: [f64; 2],
    pub skew: f64,
    pub opacity: f64,
}
impl TransformValues {
    /// T(position)*R(rotation)*K(skew)*S(scale)*T(-anchor).
    /// K is an X shear: x' = x + tan(skew in degrees)*y; y' = y.
    pub fn affine(self) -> Affine2 {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let shear = self.skew.to_radians().tan();
        let a = cos * self.scale[0];
        let b = sin * self.scale[0];
        let c = (cos * shear - sin) * self.scale[1];
        let d = (sin * shear + cos) * self.scale[1];
        Affine2([
            [
                a,
                c,
                self.position[0] - a * self.anchor[0] - c * self.anchor[1],
            ],
            [
                b,
                d,
                self.position[1] - b * self.anchor[0] - d * self.anchor[1],
            ],
        ])
    }
}

/// Retains containment and kind (including Group/Null/placement frames) so later
/// scene compilation can preserve group isolation and local opacity semantics.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedNode {
    pub key: NodeKey,
    pub composition: CompositionId,
    pub containment_parent: Option<NodeKey>,
    pub kind: NodeKind,
    pub local_time: Time,
    pub transform: TransformValues,
    pub local_transform: Affine2,
    pub world_transform: Affine2,
    pub properties: BTreeMap<RuntimePropertyKey, Value>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedScene {
    pub composition: CompositionId,
    pub time: Time,
    /// Containment pre-order. An instance's roots appear immediately after its
    /// placement frame, before its authored children, each in child_order.
    pub nodes: Vec<EvaluatedNode>,
    pub inputs: BTreeMap<RuntimePropertyKey, Value>,
}

type PropertyResolver<'a> = dyn FnMut(
        &[RuntimePropertyKey],
        Time,
    ) -> Result<BTreeMap<RuntimePropertyKey, Value>, EvaluationError>
    + 'a;

impl DependencyGraph<'_> {
    fn transform_values(
        &self,
        key: &NodeKey,
        time: Time,
        all_properties: bool,
        resolve: &mut PropertyResolver<'_>,
    ) -> Result<(TransformValues, BTreeMap<RuntimePropertyKey, Value>), EvaluationError> {
        let node = self.scopes[&key.instance_path]
            .composition
            .nodes
            .iter()
            .find(|node| node.id == key.node)
            .unwrap();
        let keys: Vec<_> = node
            .properties
            .iter()
            .filter(|property| {
                all_properties
                    || matches!(
                        property.descriptor().key.as_str(),
                        "kronello.transform.position"
                            | "kronello.transform.anchor"
                            | "kronello.transform.rotation"
                            | "kronello.transform.scale"
                            | "kronello.transform.skew"
                    )
            })
            .map(|property| {
                PropertyKey {
                    instance_path: key.instance_path.clone(),
                    node: key.node,
                    property: property.id(),
                }
                .into()
            })
            .collect();
        let properties = resolve(&keys, time)?;
        let builtin = kronello_model::SchemaRegistry::with_builtin();
        let value = |name: &str| -> Result<Value, EvaluationError> {
            let schema = SchemaKey::new(name).expect("valid builtin key");
            if let Some(property) = node.properties.iter().find(|p| {
                p.descriptor().key == schema
                    && (all_properties || schema.as_str() != "kronello.opacity")
            }) {
                let runtime = RuntimePropertyKey::Node(PropertyKey {
                    instance_path: key.instance_path.clone(),
                    node: key.node,
                    property: property.id(),
                });
                let value = properties[&runtime].clone();
                builtin
                    .lookup(&schema)
                    .expect("builtin key")
                    .validate_value(&value)
                    .map_err(|source| EvaluationError::InvalidValue {
                        key: runtime,
                        source,
                    })?;
                Ok(value)
            } else {
                // Defaults are the existing builtin schema's values, never a
                // fallback for a property whose source failed evaluation.
                let descriptor = builtin
                    .lookup(&schema)
                    .expect("builtin transform descriptor must be present");
                Ok(descriptor.definition().default.clone())
            }
        };
        let vec2 = |name| -> Result<[f64; 2], EvaluationError> {
            match value(name)? {
                Value::Vec2(v) => Ok(v.map(|x| x.get())),
                _ => unreachable!("validated builtin type"),
            }
        };
        let angle = |name| -> Result<f64, EvaluationError> {
            match value(name)? {
                Value::Angle(v) => Ok(v.get()),
                _ => unreachable!("validated builtin type"),
            }
        };
        let opacity = match value("kronello.opacity")? {
            Value::Scalar(v) => v.get(),
            _ => unreachable!("validated builtin type"),
        };
        Ok((
            TransformValues {
                position: vec2("kronello.transform.position")?,
                anchor: vec2("kronello.transform.anchor")?,
                scale: vec2("kronello.transform.scale")?,
                rotation: angle("kronello.transform.rotation")?,
                skew: angle("kronello.transform.skew")?,
                opacity,
            },
            properties,
        ))
    }

    fn world_transform(
        &self,
        key: &NodeKey,
        time: Time,
        cache: &mut BTreeMap<NodeKey, Affine2>,
        resolve: &mut PropertyResolver<'_>,
    ) -> Result<Affine2, EvaluationError> {
        // Iterative parent chain includes the enclosing placement transform but
        // never the containment parent unless explicitly transform-parented.
        let mut chain = Vec::new();
        let mut current = Some(key.clone());
        let mut world = Affine2::IDENTITY;
        while let Some(key) = current {
            if let Some(cached) = cache.get(&key) {
                world = *cached;
                break;
            }
            let scope = &self.scopes[&key.instance_path];
            let node = scope
                .composition
                .nodes
                .iter()
                .find(|n| n.id == key.node)
                .unwrap();
            current = node
                .transform_parent
                .map(|node| NodeKey {
                    instance_path: key.instance_path.clone(),
                    node,
                })
                .or_else(|| {
                    scope.parent.as_ref().map(|(path, _, node)| NodeKey {
                        instance_path: path.clone(),
                        node: *node,
                    })
                });
            chain.push(key);
        }
        for key in chain.into_iter().rev() {
            let (transform, _) = self.transform_values(&key, time, false, resolve)?;
            let local = transform.affine().finite(&key)?;
            world = world.compose(local).finite(&key)?;
            cache.insert(key, world);
        }
        Ok(world)
    }

    pub fn evaluate_scene(&self, time: Time) -> Result<EvaluatedScene, EvaluationError> {
        self.evaluate_scene_with_properties(time, &mut |keys, time| {
            self.evaluate_properties(keys, time)
        })
    }

    /// Injects derived-value memoization without exposing a cache or backend type.
    /// The resolver must preserve evaluate_properties semantics for this graph.
    pub fn evaluate_scene_with_properties(
        &self,
        time: Time,
        resolve: &mut PropertyResolver<'_>,
    ) -> Result<EvaluatedScene, EvaluationError> {
        let path = InstancePath::root();
        let scope = &self.scopes[&path];
        let mut pending: Vec<_> = scope
            .composition
            .root_nodes
            .iter()
            .rev()
            .map(|node| {
                (
                    NodeKey {
                        instance_path: path.clone(),
                        node: *node,
                    },
                    None,
                )
            })
            .collect();
        let input_keys: Vec<_> = scope
            .composition
            .properties
            .iter()
            .map(|p| RuntimePropertyKey::Composition {
                instance_path: path.clone(),
                composition: scope.composition.id,
                property: p.id(),
            })
            .collect();
        let mut inputs = resolve(&input_keys, time)?;
        let mut nodes = Vec::new();
        let mut cache = BTreeMap::new();
        while let Some((key, parent)) = pending.pop() {
            let scope = &self.scopes[&key.instance_path];
            let local_time = self.local_time(&key.instance_path, time)?;
            let node = scope
                .composition
                .nodes
                .iter()
                .find(|n| n.id == key.node)
                .unwrap();
            if !node.active_range.contains(local_time) {
                continue;
            }
            let (transform, properties) = self.transform_values(&key, time, true, resolve)?;
            let local_transform = transform.affine().finite(&key)?;
            let world_transform = self.world_transform(&key, time, &mut cache, resolve)?;
            nodes.push(EvaluatedNode {
                key: key.clone(),
                composition: scope.composition.id,
                containment_parent: parent,
                kind: node.kind.clone(),
                local_time,
                transform,
                local_transform,
                world_transform,
                properties,
            });
            for child in node.child_order.iter().rev() {
                pending.push((
                    NodeKey {
                        instance_path: key.instance_path.clone(),
                        node: *child,
                    },
                    Some(key.clone()),
                ));
            }
            if let NodeKind::CompositionInstance(instance) = &node.kind {
                let path = key.instance_path.child(instance.id);
                let nested = &self.scopes[&path];
                // Composition properties are public input values; only evaluate
                // them when the placement actually enters the scene.
                let keys: Vec<_> = nested
                    .composition
                    .properties
                    .iter()
                    .map(|p| RuntimePropertyKey::Composition {
                        instance_path: path.clone(),
                        composition: nested.composition.id,
                        property: p.id(),
                    })
                    .collect();
                inputs.extend(resolve(&keys, time)?);
                for child in nested.composition.root_nodes.iter().rev() {
                    pending.push((
                        NodeKey {
                            instance_path: path.clone(),
                            node: *child,
                        },
                        Some(key.clone()),
                    ));
                }
            }
        }
        Ok(EvaluatedScene {
            composition: self.root,
            time,
            nodes,
            inputs,
        })
    }
}
