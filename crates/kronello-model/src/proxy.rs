//! Preview-proxy linkage (ADR-0119). A proxy is an ordinary `AssetKind::Video`
//! asset; the link records provenance and the locked identities of both sides.
//! Authored media nodes keep the original `AssetId`; render-time substitution
//! is transient and never rewrites the document.
use crate::{AssetId, FiniteF64, ProjectError};
use kronello_time::Rational;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyLink {
    /// Authored media identity; unique across `Project::proxies`.
    pub original: AssetId,
    /// The proxy asset; unique across `Project::proxies` and `assets` entries
    /// are still independent (a proxy asset may serve exactly one original).
    pub proxy: AssetId,
    /// Source stream that was transcoded.
    pub original_stream_index: u32,
    /// Video stream index inside the proxy asset.
    pub proxy_stream_index: u32,
    /// Linear scale applied to source dimensions.
    pub scale: FiniteF64,
    /// Proxy pixel dimensions (rounded to the encoder's even constraint).
    pub width: u32,
    pub height: u32,
    /// Locked `Asset::content_hash` of the original at registration time.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub source_content_hash: String,
    /// Locked duration of the original stream, used for duration checks.
    pub source_duration: Option<Rational>,
    /// Originating `proxy.generate` job id, when produced by a job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
}

fn invalid(reason: &str) -> ProjectError {
    ProjectError::InvalidDocument(reason.into())
}

/// Encoder-feasible proxy dimensions for `scale` in `(0, 1]`: each axis is
/// `round(source * scale)` forced even, clamped to `[2, even_floor(source)]`.
/// `None` when the source cannot produce a conforming proxy.
pub fn proxy_dimensions(source_width: u32, source_height: u32, scale: f64) -> Option<(u32, u32)> {
    if !scale.is_finite() || scale <= 0.0 || scale > 1.0 || source_width < 2 || source_height < 2 {
        return None;
    }
    let axis = |source: u32| -> u32 {
        let scaled = (f64::from(source) * scale)
            .round()
            .clamp(0.0, f64::from(u32::MAX)) as u32;
        scaled.clamp(2, source & !1) & !1
    };
    Some((axis(source_width), axis(source_height)))
}

impl ProxyLink {
    /// Metadata-only structural validation; no filesystem access.
    pub fn validate(&self) -> Result<(), ProjectError> {
        let valid_hash = self.source_content_hash.len() == 64
            && self
                .source_content_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if self.original == self.proxy
            || self.scale.get() <= 0.0
            || self.scale.get() > 1.0
            || self.width == 0
            || self.height == 0
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
            || self.source_duration.is_some_and(|d| d <= Rational::ZERO)
            || self
                .job
                .as_ref()
                .is_some_and(|j| uuid::Uuid::parse_str(j).map_or(true, |id| id.to_string() != *j))
            || !valid_hash
        {
            return Err(invalid("invalid proxy link"));
        }
        Ok(())
    }
}

impl crate::Project {
    /// Link registered for an authored asset, if any.
    pub fn proxy_link(&self, original: AssetId) -> Option<&ProxyLink> {
        self.proxies.iter().find(|link| link.original == original)
    }
    /// True when any authored structure still references `id`: composition
    /// media nodes, sequence clips, template-instance and repeater media
    /// bindings. `proxy.clear` uses this to decide whether the proxy asset
    /// object may leave the document together with its link.
    pub fn asset_in_use(&self, id: AssetId) -> bool {
        let constant_ref = |source: &crate::PropertySource<crate::Value>| matches!(source, crate::PropertySource::Constant(crate::Value::AssetRef(a)) if *a == id);
        let value_ref =
            |value: &crate::Value| matches!(value, crate::Value::AssetRef(a) if *a == id);
        for object in &self.compositions {
            if let crate::DocumentObject::Known(composition) = object {
                for node in &composition.nodes {
                    match &node.kind {
                        crate::NodeKind::Media(media) if media.asset == id => return true,
                        crate::NodeKind::CompositionInstance(instance)
                            if instance.input_bindings.values().any(constant_ref) =>
                        {
                            return true;
                        }
                        _ => (),
                    }
                }
            }
        }
        for object in &self.sequences {
            if let crate::DocumentObject::Known(sequence) = object {
                for clip in sequence.tracks.iter().flat_map(|t| t.clips.iter()) {
                    if matches!(&clip.source_ref, crate::SourceRef::Asset { asset, .. } if *asset == id)
                    {
                        return true;
                    }
                }
            }
        }
        // NLE-007: multicam angles reference document assets directly.
        if self
            .multicams
            .iter()
            .flat_map(|group| &group.angles)
            .any(|angle| angle.asset == id)
        {
            return true;
        }
        for object in &self.template_instances {
            if let crate::DocumentObject::Known(instance) = object
                && instance.inputs.values().any(value_ref)
            {
                return true;
            }
        }
        for object in &self.repeaters {
            if let crate::DocumentObject::Known(repeater) = object {
                for instance in &repeater.instances {
                    if instance.input_bindings.values().any(constant_ref) {
                        return true;
                    }
                }
            }
        }
        false
    }
    /// True when the link currently refers to the live content of both sides.
    /// Metadata only; file resolution happens in `kronello-media`.
    pub fn proxy_link_state(&self, link: &ProxyLink) -> Result<(), ProjectError> {
        let original = self
            .assets
            .iter()
            .find_map(|a| match a {
                crate::DocumentObject::Known(a) if a.id == link.original => Some(a),
                _ => None,
            })
            .ok_or_else(|| invalid("proxy link original missing"))?;
        let proxy = self
            .assets
            .iter()
            .find_map(|a| match a {
                crate::DocumentObject::Known(a) if a.id == link.proxy => Some(a),
                _ => None,
            })
            .ok_or_else(|| invalid("proxy link target missing"))?;
        if original.kind != crate::AssetKind::Video
            || proxy.kind != crate::AssetKind::Video
            || original.content_hash != link.source_content_hash
            || !original
                .streams
                .iter()
                .any(|s| s.index == link.original_stream_index)
            || !proxy.streams.iter().any(|s| {
                s.index == link.proxy_stream_index
                    && s.width == Some(link.width)
                    && s.height == Some(link.height)
            })
        {
            return Err(invalid("proxy link is stale"));
        }
        Ok(())
    }
}
