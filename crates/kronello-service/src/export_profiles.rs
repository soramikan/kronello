//! Closed output discovery. Registration never opens a codec/device session.
use crate::{JobOutput, MediaCapabilities, ServiceError};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportExecution {
    Software,
    Hardware,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAvailability {
    Available,
    Unavailable,
    Unverified,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportAudioCodec {
    PcmS24le,
    Alac,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportProfileCapability {
    pub format: String,
    pub profile_versions: Vec<u32>,
    pub audio_modes: Vec<kronello_audio::AudioSourceMode>,
    /// AAC is omitted until its export contract is adopted.
    pub audio_codecs: Vec<ExportAudioCodec>,
    pub container_extension: String,
    pub execution: ExportExecution,
    pub encoder_registered: bool,
    pub device_availability: DeviceAvailability,
    pub reason: Option<ServiceError>,
}
impl JobOutput {
    /// Representative values for discovery only; coverage is checked against
    /// the schema of the actual exporter enum, including image sequences.
    pub(crate) fn discovery_outputs() -> Vec<Self> {
        use kronello_audio::AudioSourceMode::Explicit;
        use kronello_media::DeliveryAudioCodec::Alac;
        vec![
            Self::ImageSequence,
            Self::ProResMov {
                audio: Explicit,
                profile_version: 3,
                clips: vec![],
                background: [0.0; 3],
            },
            Self::ProResSdrFromHdrMov {
                audio: Explicit,
                profile_version: 1,
                clips: vec![],
                background: [0.0; 3],
            },
            Self::ProResHdrMov {
                audio: Explicit,
                profile_version: 1,
                transfer: kronello_render::HdrTransfer::Pq,
                clips: vec![],
                background: [0.0; 3],
            },
            Self::Av1Mp4 {
                audio: Explicit,
                audio_codec: Alac,
                profile_version: 1,
                clips: vec![],
                background: [0.0; 3],
            },
            Self::H264Mov {
                audio: Explicit,
                audio_codec: Alac,
                profile_version: 1,
                clips: vec![],
                background: [0.0; 3],
            },
            Self::HevcMov {
                audio: Explicit,
                audio_codec: Alac,
                profile_version: 1,
                clips: vec![],
                background: [0.0; 3],
            },
        ]
    }
    fn discovery(&self, media: Option<&MediaCapabilities>) -> ExportProfileCapability {
        use kronello_audio::AudioSourceMode::{Document, Explicit, Silence};
        let (modes, codecs, extension, encoder, hardware) = match self {
            Self::ImageSequence => (vec![], vec![], "", "", false),
            Self::ProResSdrFromHdrMov { .. }
            | Self::ProResMov { .. }
            | Self::ProResHdrMov { .. } => (
                vec![Explicit, Document, Silence],
                vec!["pcm_s24le"],
                "mov",
                "prores_ks",
                false,
            ),
            Self::Av1Mp4 { .. } => (
                vec![Explicit, Document, Silence],
                vec!["alac"],
                "mp4",
                "libsvtav1",
                false,
            ),
            Self::H264Mov { .. } => (
                vec![Explicit, Document, Silence],
                vec!["alac"],
                "mov",
                "h264_videotoolbox",
                true,
            ),
            Self::HevcMov { .. } => (
                vec![Explicit, Document, Silence],
                vec!["alac"],
                "mov",
                "hevc_videotoolbox",
                true,
            ),
        };
        let registered = encoder.is_empty()
            || media.is_some_and(|m| {
                m.codecs
                    .iter()
                    .any(|c| c.name == encoder && c.encoder && (!hardware || c.hardware))
                    && codecs
                        .iter()
                        .all(|name| m.encoders.iter().any(|c| c == name))
            });
        let unavailable = media.is_some() && !registered;
        ExportProfileCapability {
            format: serde_json::to_value(self).expect("output serialization")["format"]
                .as_str()
                .expect("output tag")
                .into(),
            profile_versions: self.supported_profile_versions().to_vec(),
            audio_modes: modes,
            audio_codecs: codecs
                .into_iter()
                .map(|codec| match codec {
                    "pcm_s24le" => ExportAudioCodec::PcmS24le,
                    "alac" => ExportAudioCodec::Alac,
                    _ => unreachable!("closed audio codec"),
                })
                .collect(),
            container_extension: extension.into(),
            execution: if hardware {
                ExportExecution::Hardware
            } else {
                ExportExecution::Software
            },
            encoder_registered: registered,
            device_availability: if unavailable {
                DeviceAvailability::Unavailable
            } else if hardware || (media.is_none() && !encoder.is_empty()) {
                DeviceAvailability::Unverified
            } else {
                DeviceAvailability::Available
            },
            reason: unavailable.then(|| {
                ServiceError::new(
                    "ENCODER_UNAVAILABLE",
                    "required closed-profile encoder is not registered",
                )
            }),
        }
    }
}
pub(crate) fn export_profiles(media: Option<&MediaCapabilities>) -> Vec<ExportProfileCapability> {
    JobOutput::discovery_outputs()
        .iter()
        .map(|o| o.discovery(media))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn discovery_covers_exactly_exporter_variants_and_versions() {
        let wire = serde_json::to_value(schemars::schema_for!(JobOutput)).unwrap();
        let expected: BTreeSet<_> = wire["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                v["properties"]["format"]["const"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        let profiles = export_profiles(None);
        assert_eq!(
            profiles
                .iter()
                .map(|p| p.format.clone())
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert_eq!(profiles.len(), expected.len());
        for output in JobOutput::discovery_outputs() {
            let profile = output.discovery(None);
            if !matches!(output, JobOutput::ImageSequence) {
                for version in &profile.profile_versions {
                    let mut wire = serde_json::to_value(&output).unwrap();
                    wire["profile_version"] = (*version).into();
                    assert!(
                        serde_json::from_value::<JobOutput>(wire)
                            .unwrap()
                            .movie_settings()
                            .is_ok()
                    );
                }
            }
            assert!(
                profile
                    .audio_codecs
                    .iter()
                    .all(|c| matches!(c, ExportAudioCodec::Alac | ExportAudioCodec::PcmS24le))
            );
            if matches!(profile.execution, ExportExecution::Hardware) {
                assert!(matches!(
                    profile.device_availability,
                    DeviceAvailability::Unverified
                ));
            }
        }
    }
    #[test]
    fn absent_hardware_registration_is_typed_unavailable() {
        let mut media: MediaCapabilities = serde_json::from_value(serde_json::json!({"runtime_version":"test","decoders":[],"encoders":[],"hwaccels":[],"schema_version":1,"ffmpeg_version":"test","library_directory":"/test","substituted":false,"libraries":[],"distribution_eligible":false,"development_only":true,"codecs":[]})).unwrap();
        for hardware in [false, true] {
            media.encoders = vec!["h264_videotoolbox".into(), "alac".into()];
            media.codecs = vec![kronello_media::CodecCapability {
                name: "h264_videotoolbox".into(),
                encoder: true,
                decoder: false,
                hardware,
            }];
            let profiles = export_profiles(Some(&media));
            let p = profiles.iter().find(|p| p.format == "h264_mov").unwrap();
            assert_eq!(p.encoder_registered, hardware);
            if hardware {
                assert!(matches!(
                    p.device_availability,
                    DeviceAvailability::Unverified
                ));
            } else {
                assert_eq!(p.reason.as_ref().unwrap().code, "ENCODER_UNAVAILABLE");
            }
        }
    }
}
