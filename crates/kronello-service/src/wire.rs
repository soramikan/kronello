//! Decode tagged JSON without serde Content buffering: arbitrary_precision
//! numbers must reach the concrete f64 fields through the JSON deserializer.
//! Raw payloads also preserve duplicate keys for strict request deserialization.
use std::collections::BTreeMap;

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Error, MapAccess, Visitor},
};
use serde_json::value::RawValue;

use crate::{Request, Response, ResultData};

// Keep f32 audio/background values on the concrete JSON decoder as well.
impl<'de> Deserialize<'de> for crate::JobOutput {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let format: String = take(&mut fields, "format")?;
        match format.as_str() {
            "image_sequence" => {
                exhausted::<D::Error>(&fields)?;
                Ok(Self::ImageSequence)
            }
            "pro_res_mov" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Mov {
                    #[serde(default)]
                    audio: kronello_audio::AudioSourceMode,
                    #[serde(default = "crate::jobs::movie_profile_v1")]
                    profile_version: u32,
                    #[serde(default)]
                    audio_layout: Option<kronello_model::ChannelMask>,
                    clips: Vec<crate::JobAudioClip>,
                    background: [f32; 3],
                }
                let mov: Mov = payload(&fields)?;
                Ok(Self::ProResMov {
                    audio: mov.audio,
                    profile_version: mov.profile_version,
                    audio_layout: mov.audio_layout,
                    clips: mov.clips,
                    background: mov.background,
                })
            }
            "pro_res_sdr_from_hdr_mov" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct SdrMov {
                    profile_version: u32,
                    #[serde(default)]
                    audio: kronello_audio::AudioSourceMode,
                    #[serde(default)]
                    audio_layout: Option<kronello_model::ChannelMask>,
                    clips: Vec<crate::JobAudioClip>,
                    background: [f32; 3],
                }
                let mov: SdrMov = payload(&fields)?;
                Ok(Self::ProResSdrFromHdrMov {
                    profile_version: mov.profile_version,
                    audio: mov.audio,
                    audio_layout: mov.audio_layout,
                    clips: mov.clips,
                    background: mov.background,
                })
            }
            "pro_res_hdr_mov" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct HdrMov {
                    profile_version: u32,
                    transfer: kronello_render::HdrTransfer,
                    #[serde(default)]
                    audio: kronello_audio::AudioSourceMode,
                    #[serde(default)]
                    audio_layout: Option<kronello_model::ChannelMask>,
                    clips: Vec<crate::JobAudioClip>,
                    background: [f32; 3],
                }
                let mov: HdrMov = payload(&fields)?;
                Ok(Self::ProResHdrMov {
                    profile_version: mov.profile_version,
                    transfer: mov.transfer,
                    audio: mov.audio,
                    audio_layout: mov.audio_layout,
                    clips: mov.clips,
                    background: mov.background,
                })
            }
            "av1_mp4" | "h264_mov" | "hevc_mov" | "av1_webm" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Delivery {
                    profile_version: u32,
                    #[serde(default)]
                    audio: kronello_audio::AudioSourceMode,
                    #[serde(default)]
                    audio_codec: kronello_media::DeliveryAudioCodec,
                    #[serde(default)]
                    audio_layout: Option<kronello_model::ChannelMask>,
                    clips: Vec<crate::JobAudioClip>,
                    background: [f32; 3],
                }
                let mov: Delivery = payload(&fields)?;
                Ok(match format.as_str() {
                    "av1_mp4" => Self::Av1Mp4 {
                        profile_version: mov.profile_version,
                        audio: mov.audio,
                        audio_codec: mov.audio_codec,
                        audio_layout: mov.audio_layout,
                        clips: mov.clips,
                        background: mov.background,
                    },
                    "h264_mov" => Self::H264Mov {
                        profile_version: mov.profile_version,
                        audio: mov.audio,
                        audio_codec: mov.audio_codec,
                        audio_layout: mov.audio_layout,
                        clips: mov.clips,
                        background: mov.background,
                    },
                    "hevc_mov" => Self::HevcMov {
                        profile_version: mov.profile_version,
                        audio: mov.audio,
                        audio_codec: mov.audio_codec,
                        audio_layout: mov.audio_layout,
                        clips: mov.clips,
                        background: mov.background,
                    },
                    _ => Self::Av1Webm {
                        profile_version: mov.profile_version,
                        audio: mov.audio,
                        audio_codec: mov.audio_codec,
                        audio_layout: mov.audio_layout,
                        clips: mov.clips,
                        background: mov.background,
                    },
                })
            }
            "caption_sidecar" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Sidecar {
                    sequence: kronello_model::SequenceId,
                    caption_format: kronello_model::CaptionFormat,
                }
                let sidecar: Sidecar = payload(&fields)?;
                Ok(Self::CaptionSidecar {
                    sequence: sidecar.sequence,
                    caption_format: sidecar.caption_format,
                })
            }
            _ => Err(D::Error::custom("unknown job output format")),
        }
    }
}

type Fields = BTreeMap<String, Box<RawValue>>;
fn fields<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Fields, D::Error> {
    struct Unique;
    impl<'de> Visitor<'de> for Unique {
        type Value = Fields;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a JSON object with unique field names")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Fields, M::Error> {
            let mut fields = Fields::new();
            while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                if fields.insert(key.clone(), value).is_some() {
                    return Err(M::Error::custom(format!("duplicate field: {key}")));
                }
            }
            Ok(fields)
        }
    }
    deserializer.deserialize_map(Unique)
}
fn take<T: DeserializeOwned, E: Error>(fields: &mut Fields, key: &'static str) -> Result<T, E> {
    let value = fields.remove(key).ok_or_else(|| E::missing_field(key))?;
    serde_json::from_str(value.get()).map_err(E::custom)
}
fn payload<T: DeserializeOwned, E: Error>(fields: &Fields) -> Result<T, E> {
    let json = serde_json::to_string(fields).map_err(E::custom)?;
    serde_json::from_str(&json).map_err(E::custom)
}
fn exhausted<E: Error>(fields: &Fields) -> Result<(), E> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(E::custom("unknown envelope field"))
    }
}
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let tag: String = take(&mut fields, "operation")?;
        match tag.as_str() {
            "font.pin" => payload(&fields).map(Self::FontPin),
            "svg.inspect" => payload(&fields).map(Self::SvgInspect),
            "svg.export" => payload(&fields).map(Self::SvgExport),
            "svg.import_plan" => payload(&fields).map(Self::SvgImportPlan),
            "audio.analyze" => payload(&fields).map(Self::AudioAnalyze),
            "track.analyze" => payload(&fields).map(Self::TrackAnalyze),
            "scene.detect" => payload(&fields).map(Self::SceneDetect),
            "scene.apply" => payload(&fields).map(Self::SceneApply),
            "proxy.generate" => payload(&fields).map(Self::ProxyGenerate),
            "proxy.status" => payload(&fields).map(Self::ProxyStatus),
            "proxy.clear" => payload(&fields).map(Self::ProxyClear),
            "audio.loudness" => payload(&fields).map(Self::AudioLoudness),
            "audio.normalize" => payload(&fields).map(Self::AudioNormalize),
            "sequence.query" => payload(&fields).map(Self::SequenceQuery),
            "sequence.create" => payload(&fields).map(Self::SequenceCreate),
            "clip.place" => payload(&fields).map(Self::ClipPlace),
            "clip.trim" => payload(&fields).map(Self::ClipTrim),
            "clip.stretch" => payload(&fields).map(Self::ClipStretch),
            "instance.retime" => payload(&fields).map(Self::InstanceRetime),
            "template_instance.retime" => payload(&fields).map(Self::TemplateInstanceRetime),
            "captions.import_plan" => payload(&fields).map(Self::CaptionsImportPlan),
            "captions.import" => payload(&fields).map(Self::CaptionsImport),
            "captions.export" => payload(&fields).map(Self::CaptionsExport),

            "render.export" => payload(&fields).map(Self::RenderExport),
            "render.submit" => payload(&fields).map(Self::RenderSubmit),
            "job.get" => payload(&fields).map(Self::JobGet),
            "job.list" => payload(&fields).map(Self::JobList),
            "job.cancel" => payload(&fields).map(Self::JobCancel),
            "job.resume" => payload(&fields).map(Self::JobResume),
            "job.prune" => payload(&fields).map(Self::JobPrune),
            "template.set_duration" => payload(&fields).map(Self::TemplateSetDuration),
            "template.preview" => payload(&fields).map(Self::TemplatePreview),
            "template.migration_plan" => payload(&fields).map(Self::TemplateMigrationPlan),
            "template.define" => payload(&fields).map(Self::TemplateDefine),
            "template.instantiate" => payload(&fields).map(Self::TemplateInstantiate),
            "template.set_input" => payload(&fields).map(Self::TemplateSetInput),
            "asset.relink" => payload(&fields).map(Self::AssetRelink),
            "project.collect" => payload(&fields).map(Self::ProjectCollect),
            "project.create_plan" => payload(&fields).map(Self::ProjectCreatePlan),
            "project.import_plan" => payload(&fields).map(Self::ProjectImportPlan),
            "project.create" => payload(&fields).map(Self::ProjectCreate),
            "project.import" => payload(&fields).map(Self::ProjectImport),
            "project.export" => payload(&fields).map(Self::ProjectExport),
            "project.info" => payload(&fields).map(Self::ProjectInfo),
            "render.frame" => payload(&fields).map(Self::RenderFrame),
            "render.sequence" => payload(&fields).map(Self::RenderSequence),
            "edit.plan" => payload(&fields).map(Self::EditPlan),
            "edit.apply" => payload(&fields).map(Self::EditApply),
            "edit.undo" => payload(&fields).map(Self::EditUndo),
            "expression.format" => payload(&fields).map(Self::ExpressionFormat),
            "history.list" => payload(&fields).map(Self::HistoryList),
            "scene.query" => payload(&fields).map(Self::SceneQuery),
            "node.explain" => payload(&fields).map(Self::NodeExplain),
            "render.explain" => payload(&fields).map(Self::RenderExplain),
            "property.sample" => payload(&fields).map(Self::PropertySample),
            "capabilities.get" => payload(&fields).map(Self::CapabilitiesGet),
            "lut.import" => payload(&fields).map(Self::LutImport),
            "inspect.scopes" => payload(&fields).map(Self::InspectScopes),
            _ => Err(D::Error::custom("unknown operation")),
        }
    }
}
impl<'de> Deserialize<'de> for ResultData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let tag: String = take(&mut fields, "kind")?;
        let result = match tag.as_str() {
            "font" => Self::Font(take(&mut fields, "value")?),
            "svg_report" => Self::SvgReport(take(&mut fields, "value")?),
            "svg_export" => Self::SvgExport(take(&mut fields, "value")?),
            "timeline" => Self::Timeline(take(&mut fields, "value")?),
            "movie" => Self::Movie(take(&mut fields, "value")?),
            "job" => Self::Job(take(&mut fields, "value")?),
            "jobs" => Self::Jobs(take(&mut fields, "value")?),
            "pruned" => Self::Pruned(take(&mut fields, "value")?),
            "collected" => Self::Collected(take(&mut fields, "value")?),
            "project" => Self::Project(take(&mut fields, "value")?),
            "project_plan" => Self::ProjectPlan(take(&mut fields, "value")?),
            "export" => Self::Export(take(&mut fields, "value")?),
            "frame" => Self::Frame(take(&mut fields, "value")?),
            "sequence" => Self::Sequence(take(&mut fields, "value")?),
            "plan" => Self::Plan(take(&mut fields, "value")?),
            "expression_text" => Self::ExpressionText(take(&mut fields, "value")?),
            "template_preview" => Self::TemplatePreview(take(&mut fields, "value")?),
            "template_migration_plan" => Self::TemplateMigrationPlan(take(&mut fields, "value")?),
            "edit" => Self::Edit(take(&mut fields, "value")?),
            "history" => Self::History(take(&mut fields, "value")?),
            "scene" => Self::Scene(take(&mut fields, "value")?),
            "node_explanation" => Self::NodeExplanation(take(&mut fields, "value")?),
            "render_explanation" => Self::RenderExplanation(take(&mut fields, "value")?),
            "samples" => Self::Samples(take(&mut fields, "value")?),
            "proxies" => Self::Proxies(take(&mut fields, "value")?),
            "capabilities" => Self::Capabilities(take(&mut fields, "value")?),
            "captions" => Self::Captions(take(&mut fields, "value")?),
            "loudness" => Self::Loudness(take(&mut fields, "value")?),
            "normalize" => Self::Normalize(take(&mut fields, "value")?),
            "scopes" => Self::Scopes(take(&mut fields, "value")?),
            _ => return Err(D::Error::custom("unknown result kind")),
        };
        exhausted::<D::Error>(&fields)?;
        Ok(result)
    }
}
impl<'de> Deserialize<'de> for Response {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let tag: String = take(&mut fields, "status")?;
        let result = match tag.as_str() {
            "success" => Self::Success {
                result: take(&mut fields, "result")?,
            },
            "error" => Self::Error {
                error: take(&mut fields, "error")?,
            },
            _ => return Err(D::Error::custom("unknown response status")),
        };
        exhausted::<D::Error>(&fields)?;
        Ok(result)
    }
}

impl<'de> Deserialize<'de> for crate::RenderInput {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut fields = fields(d)?;
        let project = take(&mut fields, "project")?;
        let composition = if fields.contains_key("composition") {
            Some(take(&mut fields, "composition")?)
        } else {
            None
        };
        let target = if fields.contains_key("target") {
            Some(take(&mut fields, "target")?)
        } else {
            None
        };
        if composition.is_some() == target.is_some() {
            return Err(D::Error::custom(
                "specify exactly one of composition or target",
            ));
        }
        let region = take(&mut fields, "region")?;
        let profile = if fields.contains_key("profile") {
            take(&mut fields, "profile")?
        } else {
            Default::default()
        };
        let fonts = if fields.contains_key("fonts") {
            take(&mut fields, "fonts")?
        } else {
            vec![]
        };
        let media_proxies = if fields.contains_key("media_proxies") {
            take(&mut fields, "media_proxies")?
        } else {
            kronello_render::MediaProxyMode::Off
        };
        let luts = if fields.contains_key("luts") {
            take(&mut fields, "luts")?
        } else {
            vec![]
        };
        exhausted::<D::Error>(&fields)?;
        Ok(Self {
            project,
            composition,
            target,
            region,
            profile,
            fonts,
            media_proxies,
            luts,
        })
    }
}
