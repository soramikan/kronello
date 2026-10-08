//! IRIDAS `.cube` 3D LUT parsing and deterministic tetrahedral sampling
//! (ADR-0113). The parser is pure: it consumes bytes, never a filesystem
//! locator. Rendering treats the normalized lattice as opaque data.
use serde::{Deserialize, Serialize};

/// Smallest `.cube` lattice the parser accepts.
pub const LUT_3D_MIN_SIZE: u32 = 2;
/// Largest `.cube` lattice the parser accepts.
pub const LUT_3D_MAX_SIZE: u32 = 65;
/// Document-used lattice ceiling (ADR-0113): 33^3 RGB entries per stored LUT.
/// Import and render-input validation enforce this; the parser only enforces
/// `LUT_3D_MAX_SIZE` so validation errors remain distinguishable.
pub const LUT_3D_DOCUMENT_MAX_SIZE: u32 = 33;
/// Total budget of characters in one `.cube` document, matching the largest
/// accepted lattice with generous whitespace (65^3 rows).
pub const LUT_MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LutError {
    /// `INVALID_LUT`: the bytes do not form a supported `.cube` document.
    #[error("INVALID_LUT: {0}")]
    Invalid(String),
    /// `UNSUPPORTED_FEATURE`: a well-formed construct outside the supported
    /// subset (1D-only files, lattice size outside 2..=65 or document >33).
    #[error("UNSUPPORTED_FEATURE: {0}")]
    UnsupportedFeature(String),
}
impl LutError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "INVALID_LUT",
            Self::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
        }
    }
}

/// A normalized 3D LUT lattice. Rows keep the `.cube` red-fastest ordering:
/// `data[((b * size + g) * size + r) * 3 + channel]` on `r,g,b` lattice
/// coordinates in `0..size`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CubeLut {
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub data: Vec<f32>,
}
impl CubeLut {
    /// Parse an IRIDAS `.cube` document. Comments (`#` and `#comment` style
    /// lines) and `TITLE` are allowed. `LUT_1D_SIZE` tables are skipped when a
    /// `LUT_3D_SIZE` block is also present; a file containing only 1D data is
    /// `UNSUPPORTED_FEATURE`. `DOMAIN_MIN`/`DOMAIN_MAX` default to [0,0,0] and
    /// [1,1,1]. Exactly `size^3` RGB rows must follow the 3D declaration.
    pub fn parse(bytes: &[u8]) -> Result<Self, LutError> {
        if bytes.len() > LUT_MAX_BYTES {
            return Err(LutError::UnsupportedFeature(
                "lut document exceeds the byte budget".into(),
            ));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| LutError::Invalid("lut is not UTF-8 text".into()))?;
        let mut size_1d: Option<u32> = None;
        let mut size_3d: Option<u32> = None;
        let mut domain_min = [0.0f32; 3];
        let mut domain_max = [1.0f32; 3];
        let mut data = Vec::new();
        // Rows belonging to a declared 1D table are skipped, so combined
        // 1D+3D files parse deterministically.
        let mut skip_rows = 0usize;
        for (line_number, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (head, _) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            match head {
                "TITLE" | "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {}
                "COMMENT" => {}
                "LUT_1D_SIZE" => {
                    let n = parse_size(line, "LUT_1D_SIZE")?;
                    if !(LUT_3D_MIN_SIZE..=LUT_3D_MAX_SIZE).contains(&n) {
                        return Err(LutError::UnsupportedFeature(format!(
                            "LUT_1D_SIZE {n} outside {}..={LUT_3D_MAX_SIZE}",
                            LUT_3D_MIN_SIZE
                        )));
                    }
                    if size_1d.is_some() {
                        return Err(LutError::Invalid(
                            "duplicate LUT_1D_SIZE declaration".into(),
                        ));
                    }
                    size_1d = Some(n);
                    skip_rows = n as usize;
                }
                "LUT_3D_SIZE" => {
                    if size_3d.is_some() {
                        return Err(LutError::Invalid(
                            "duplicate LUT_3D_SIZE declaration".into(),
                        ));
                    }
                    let n = parse_size(line, "LUT_3D_SIZE")?;
                    if !(LUT_3D_MIN_SIZE..=LUT_3D_MAX_SIZE).contains(&n) {
                        return Err(LutError::UnsupportedFeature(format!(
                            "LUT_3D_SIZE {n} outside {}..={LUT_3D_MAX_SIZE}",
                            LUT_3D_MIN_SIZE
                        )));
                    }
                    size_3d = Some(n);
                }
                "DOMAIN_MIN" => {
                    domain_min = parse_domain(line, "DOMAIN_MIN")?;
                }
                "DOMAIN_MAX" => {
                    domain_max = parse_domain(line, "DOMAIN_MAX")?;
                }
                _ => {
                    // A data row inside a declared 1D table is skipped
                    // regardless of its column count, so combined 1D+3D
                    // documents remain parseable.
                    if skip_rows > 0 {
                        skip_rows -= 1;
                        continue;
                    }
                    let row = parse_row(line).map_err(|message| {
                        LutError::Invalid(format!("line {}: {message}", line_number + 1))
                    })?;
                    if size_3d.is_none() {
                        return Err(LutError::Invalid(format!(
                            "line {}: data row before LUT_3D_SIZE",
                            line_number + 1
                        )));
                    }
                    data.extend(row);
                }
            }
        }
        let size = size_3d.ok_or(LutError::UnsupportedFeature(
            "lut contains no LUT_3D_SIZE block".into(),
        ))?;
        if skip_rows > 0 {
            return Err(LutError::Invalid("truncated LUT_1D_SIZE table".into()));
        }
        let expected = size as usize * size as usize * size as usize * 3;
        if data.len() != expected {
            return Err(LutError::Invalid(format!(
                "lut row count mismatch: {} values for {size}^3 rows",
                data.len() / 3
            )));
        }
        validate_domain(domain_min, domain_max)?;
        Ok(Self {
            size,
            domain_min,
            domain_max,
            data,
        })
    }
    /// Structural validation shared by import, render inputs and the pixel
    /// contract. The document ceiling is caller-selected so the pure parser
    /// stays usable for the full 2..=65 range.
    pub fn validate(&self) -> Result<(), LutError> {
        if !(LUT_3D_MIN_SIZE..=LUT_3D_MAX_SIZE).contains(&self.size) {
            return Err(LutError::UnsupportedFeature(format!(
                "lut size {} outside {LUT_3D_MIN_SIZE}..={LUT_3D_MAX_SIZE}",
                self.size
            )));
        }
        let expected = self.size as usize * self.size as usize * self.size as usize * 3;
        if self.data.len() != expected || self.data.iter().any(|v| !v.is_finite()) {
            return Err(LutError::Invalid(
                "lut data length or finiteness mismatch".into(),
            ));
        }
        validate_domain(self.domain_min, self.domain_max)?;
        Ok(())
    }
    /// Reject lattices a document may not reference (ADR-0113: N <= 33).
    pub fn validate_document_size(&self) -> Result<(), LutError> {
        if self.size > LUT_3D_DOCUMENT_MAX_SIZE {
            return Err(LutError::UnsupportedFeature(format!(
                "lut size {} exceeds the document limit {LUT_3D_DOCUMENT_MAX_SIZE}",
                self.size
            )));
        }
        Ok(())
    }
    /// Sample one straight (non-premultiplied) working-space RGB triplet. The
    /// input is normalized against the declared domain; out-of-domain values
    /// (HDR or negative) clamp to the lattice endpoints, so the LUT returns
    /// its endpoint color instead of extrapolating. Interpolation is
    /// tetrahedral and evaluated in f32, mirrored by the WGSL pass.
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let span = [0, 1, 2].map(|i| self.domain_max[i] - self.domain_min[i]);
        let pos = [0, 1, 2]
            .map(|i| ((rgb[i] - self.domain_min[i]) / span[i]).clamp(0.0, 1.0) * (n - 1) as f32);
        let base = pos.map(|v| (v.floor() as u32).min(n - 2));
        let f = [0, 1, 2].map(|i| pos[i] - base[i] as f32);
        let corner = |r: u32, g: u32, b: u32| -> [f32; 3] {
            let index = (((b + base[2]) * n + (g + base[1])) * n + (r + base[0])) * 3;
            [
                self.data[index as usize],
                self.data[index as usize + 1],
                self.data[index as usize + 2],
            ]
        };
        let c000 = corner(0, 0, 0);
        let c001 = corner(0, 0, 1);
        let c010 = corner(0, 1, 0);
        let c011 = corner(0, 1, 1);
        let c100 = corner(1, 0, 0);
        let c101 = corner(1, 0, 1);
        let c110 = corner(1, 1, 0);
        let c111 = corner(1, 1, 1);
        // Canonical tetrahedral branch order; identical comparisons appear in
        // effect.wgsl so CPU and GPU take the same sub-tetrahedron.
        let (a, b, c, d) = if f[0] >= f[1] {
            if f[1] >= f[2] {
                (c000, c100, c110, c111)
            } else if f[0] >= f[2] {
                (c000, c100, c101, c111)
            } else {
                (c000, c001, c101, c111)
            }
        } else if f[2] >= f[1] {
            (c000, c001, c011, c111)
        } else if f[0] >= f[2] {
            (c000, c010, c110, c111)
        } else {
            (c000, c010, c011, c111)
        };
        let (fr, fg, fb) = (f[0], f[1], f[2]);
        std::array::from_fn(|i| {
            let (w0, w1, w2, w3) = if f[0] >= f[1] {
                if f[1] >= f[2] {
                    (1.0 - fr, fr - fg, fg - fb, fb)
                } else if f[0] >= f[2] {
                    (1.0 - fr, fr - fb, fb - fg, fg)
                } else {
                    (1.0 - fb, fb - fr, fr - fg, fg)
                }
            } else if f[2] >= f[1] {
                (1.0 - fb, fb - fg, fg - fr, fr)
            } else if f[0] >= f[2] {
                (1.0 - fg, fg - fr, fr - fb, fb)
            } else {
                (1.0 - fg, fg - fb, fb - fr, fr)
            };
            a[i] * w0 + b[i] * w1 + c[i] * w2 + d[i] * w3
        })
    }
}
fn parse_size(line: &str, key: &str) -> Result<u32, LutError> {
    let mut parts = line.split_whitespace();
    let _ = parts.next();
    let value = parts
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or_else(|| LutError::Invalid(format!("{key} requires one positive integer")))?;
    if parts.next().is_some() {
        return Err(LutError::Invalid(format!("{key} has trailing fields")));
    }
    Ok(value)
}
fn parse_domain(line: &str, key: &str) -> Result<[f32; 3], LutError> {
    let values: Vec<f32> = line
        .split_whitespace()
        .skip(1)
        .map(|v| v.parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| LutError::Invalid(format!("{key} requires three finite numbers")))?;
    let domain: [f32; 3] = values
        .try_into()
        .map_err(|_| LutError::Invalid(format!("{key} requires exactly three numbers")))?;
    if domain.iter().any(|v| !v.is_finite()) {
        return Err(LutError::Invalid(format!("{key} requires finite numbers")));
    }
    Ok(domain)
}
fn parse_row(line: &str) -> Result<[f32; 3], String> {
    let values: Vec<f32> = line
        .split_whitespace()
        .map(|v| v.parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| "lut row requires three numbers".to_string())?;
    let row: [f32; 3] = values
        .try_into()
        .map_err(|_| "lut row requires exactly three numbers".to_string())?;
    if row.iter().any(|v| !v.is_finite()) {
        return Err("lut row requires finite numbers".into());
    }
    Ok(row)
}
fn validate_domain(min: [f32; 3], max: [f32; 3]) -> Result<(), LutError> {
    if min.iter().chain(&max).any(|v| !v.is_finite()) || (0..3).any(|i| max[i] <= min[i]) {
        return Err(LutError::Invalid(
            "domain bounds must be finite and increasing".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc(size: u32) -> String {
        let mut text = format!("TITLE \"fixture\"\n# comment\nLUT_3D_SIZE {size}\n");
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    text.push_str(&format!(
                        "{} {} {}\n",
                        r as f32 / (size - 1) as f32,
                        g as f32 / (size - 1) as f32,
                        b as f32 / (size - 1) as f32
                    ));
                }
            }
        }
        text
    }
    #[test]
    fn parses_identity_lattice_and_tolerates_title_comments() {
        let lut = CubeLut::parse(doc(2).as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.data.len(), 24);
        for (r, g, b) in [
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
            (0.25, 0.5, 0.75),
            (1.0, 1.0, 1.0),
        ] {
            let out = lut.sample([r, g, b]);
            assert!(
                (out[0] - r).abs() < 1e-6 && (out[1] - g).abs() < 1e-6 && (out[2] - b).abs() < 1e-6
            );
        }
    }
    #[test]
    fn domain_normalization_and_endpoint_clamp() {
        let mut text =
            "LUT_3D_SIZE 2\nDOMAIN_MIN 0.1 0.1 0.1\nDOMAIN_MAX 0.9 0.9 0.9\n".to_string();
        text.push_str(doc(2).split("LUT_3D_SIZE 2\n").nth(1).unwrap());
        let lut = CubeLut::parse(text.as_bytes()).unwrap();
        // Below domain_min: normalized negative clamps to the first endpoint.
        assert_eq!(lut.sample([-1.0, -1.0, -1.0]), [0.0, 0.0, 0.0]);
        // Above domain_max: HDR >1 values return the last endpoint.
        assert_eq!(lut.sample([4.0, 4.0, 4.0]), [1.0, 1.0, 1.0]);
        let mid = lut.sample([0.5, 0.5, 0.5]);
        assert!((mid[0] - 0.5).abs() < 1e-6);
    }
    #[test]
    fn malformed_documents_are_typed_invalid_lut() {
        for bad in [
            "LUT_3D_SIZE two\n0 0 0\n",
            "LUT_3D_SIZE 2\n0 0\n",
            "LUT_3D_SIZE 2\n0 0 0\n",
            "LUT_3D_SIZE 2\n0 0 0 extra\n",
            "LUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n1 1 1\n",
            "LUT_3D_SIZE 2\nDOMAIN_MIN 1 1 1\n0 0 0\n",
            "0 0 0\nLUT_3D_SIZE 2\n",
        ] {
            assert!(
                matches!(CubeLut::parse(bad.as_bytes()), Err(LutError::Invalid(_))),
                "{bad}"
            );
        }
    }
    #[test]
    fn one_dimensional_only_and_oversized_lattices_are_unsupported() {
        let one_d = "LUT_1D_SIZE 4\n0\n0.3\n0.7\n1\n";
        assert!(matches!(
            CubeLut::parse(one_d.as_bytes()),
            Err(LutError::UnsupportedFeature(_))
        ));
        assert!(matches!(
            CubeLut::parse("LUT_3D_SIZE 66\n".as_bytes()),
            Err(LutError::UnsupportedFeature(_))
        ));
        assert!(matches!(
            CubeLut::parse("LUT_3D_SIZE 1\n".as_bytes()),
            Err(LutError::UnsupportedFeature(_))
        ));
    }
    #[test]
    fn combined_one_and_three_dimensional_documents_parse() {
        let mut text = "LUT_1D_SIZE 2\n0 0 0\n1 1 1\nLUT_3D_SIZE 2\n".to_string();
        text.push_str(doc(2).split("LUT_3D_SIZE 2\n").nth(1).unwrap());
        let lut = CubeLut::parse(text.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
    }
    #[test]
    fn document_size_limit_is_separate_from_parser_acceptance() {
        let lut = CubeLut::parse(doc(34).as_bytes()).unwrap();
        assert!(matches!(
            lut.validate_document_size(),
            Err(LutError::UnsupportedFeature(_))
        ));
        CubeLut::parse(doc(33).as_bytes())
            .unwrap()
            .validate_document_size()
            .unwrap();
    }
}
