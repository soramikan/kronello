//! Deterministic synthetic camera-RAW media generators (MEDIA-005 / ADR-0136).
//!
//! The bytes are generated in-process so tests stay hermetic and no fixture
//! license is needed. The DNG writer emits a complete single-IFD TIFF with the
//! tags LibRaw requires: Bayer CFA layout, DNGVersion, ColorMatrix1,
//! AsShotNeutral, black/white levels, and one uncompressed strip.
//!
//! A TIFF IFD entry is (tag, type, count, value); values longer than four
//! bytes live in the data area after the IFD.
use std::collections::BTreeMap;

/// TIFF field types used by the generator.
const BYTE: u16 = 1;
const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;
const SRATIONAL: u16 = 10;

fn rat(n: u32, d: u32) -> Vec<u8> {
    [n.to_le_bytes(), d.to_le_bytes()].concat()
}
fn srat(n: i32, d: u32) -> Vec<u8> {
    [n.to_le_bytes(), d.to_le_bytes()].concat()
}

/// Build a single-IFD little-endian TIFF whose first IFD is at offset 8 and
/// whose pixel strip is appended after all tag data. Returns the file bytes.
fn tiff_single_ifd(entries: &[(u16, u16, u32, Vec<u8>)], pixels: &[u8]) -> Vec<u8> {
    let sorted: BTreeMap<u16, (u16, u32, &Vec<u8>)> = entries
        .iter()
        .map(|(tag, typ, count, value)| (*tag, (*typ, *count, value)))
        .collect();
    let count = sorted.len() as u16;
    let ifd_size = 2 + usize::from(count) * 12 + 4;
    let data_base = 8 + ifd_size;
    let mut extra = Vec::new();
    let mut records = Vec::new();
    let mut strip_entry = None;
    for (tag, (typ, cnt, value)) in sorted {
        if tag == 273 {
            // StripOffsets patched after the data area size is known.
            strip_entry = Some((typ, cnt));
            continue;
        }
        if value.len() <= 4 {
            let mut inline = value.clone();
            inline.resize(4, 0);
            records.push((tag, typ, cnt, inline));
        } else {
            let offset = (data_base + extra.len()) as u32;
            records.push((tag, typ, cnt, offset.to_le_bytes().to_vec()));
            extra.extend_from_slice(value);
            if extra.len() % 2 != 0 {
                extra.push(0);
            }
        }
    }
    if let Some((typ, cnt)) = strip_entry {
        let offset = (data_base + extra.len()) as u32;
        records.push((273, typ, cnt, offset.to_le_bytes().to_vec()));
        records.sort_by_key(|(tag, ..)| *tag);
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for (tag, typ, cnt, value) in &records {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&typ.to_le_bytes());
        out.extend_from_slice(&cnt.to_le_bytes());
        out.extend_from_slice(value);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&extra);
    out.extend_from_slice(pixels);
    out
}

/// Deterministic uncompressed 16-bit RGGB Bayer DNG, `width`×`height`.
/// LibRaw requires RAW dimensions ≥ 22 in each axis.
pub fn dng_bayer16(width: u32, height: u32) -> Vec<u8> {
    dng_bayer16_seeded(width, height, 1, 0)
}

/// Same fixture with an arbitrary TIFF Compression tag. The strip bytes are
/// stored uncompressed regardless; detection and the compression gate only
/// read the tag, while a decoder that honours it sees truncated data.
pub fn dng_bayer16_with_compression(width: u32, height: u32, compression: u16) -> Vec<u8> {
    dng_bayer16_seeded(width, height, compression, 0)
}

/// Deterministic Bayer DNG with a pixel seed, so frame-sequence members carry
/// distinct content while remaining reproducible.
pub fn dng_bayer16_seeded(width: u32, height: u32, compression: u16, seed: u32) -> Vec<u8> {
    let samples: Vec<u16> = (0..(width * height))
        .map(|i| ((i * 97 + seed * 977 + 512) & 0x3fff) as u16)
        .collect();
    let pixels: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
    let cm = [
        srat(1, 1),
        srat(0, 1),
        srat(0, 1),
        srat(0, 1),
        srat(1, 1),
        srat(0, 1),
        srat(0, 1),
        srat(0, 1),
        srat(1, 1),
    ]
    .concat();
    let entries: Vec<(u16, u16, u32, Vec<u8>)> = vec![
        (254, LONG, 1, 0u32.to_le_bytes().to_vec()),
        (256, LONG, 1, width.to_le_bytes().to_vec()),
        (257, LONG, 1, height.to_le_bytes().to_vec()),
        (258, SHORT, 1, 16u16.to_le_bytes().to_vec()),
        (259, SHORT, 1, compression.to_le_bytes().to_vec()),
        (262, SHORT, 1, 32803u16.to_le_bytes().to_vec()), // CFA
        (271, ASCII, 9, b"Kronello\0".to_vec()),
        (272, ASCII, 14, b"Synthetic DNG\0".to_vec()),
        (273, LONG, 1, vec![]), // StripOffsets patched by tiff_single_ifd
        (274, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (277, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (278, LONG, 1, height.to_le_bytes().to_vec()),
        (279, LONG, 1, (width * height * 2).to_le_bytes().to_vec()),
        (284, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (
            33421,
            SHORT,
            2,
            [2u16, 2].iter().flat_map(|v| v.to_le_bytes()).collect(),
        ),
        (33422, BYTE, 4, vec![0, 1, 1, 2]), // RGGB
        (50706, BYTE, 4, vec![1, 4, 0, 0]), // DNGVersion
        (50707, BYTE, 4, vec![1, 1, 0, 0]),
        (50708, ASCII, 18, b"Kronello Synthetic\0".to_vec()),
        (50710, BYTE, 3, vec![0, 1, 2]), // CFAPlaneColor
        (50711, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (50714, LONG, 1, 0u32.to_le_bytes().to_vec()), // BlackLevel
        (50717, SHORT, 1, 16383u16.to_le_bytes().to_vec()), // WhiteLevel
        (50721, SRATIONAL, 9, cm),                     // ColorMatrix1
        (
            50728,
            RATIONAL,
            3,
            [rat(1, 1), rat(1, 1), rat(1, 1)].concat(),
        ), // AsShotNeutral
        (50778, SHORT, 1, 21u16.to_le_bytes().to_vec()), // CalibrationIlluminant1 D65
        (50779, SHORT, 1, 21u16.to_le_bytes().to_vec()),
    ];
    tiff_single_ifd(&entries, &pixels)
}

/// Minimal Blackmagic RAW marker: BRAW is a ZIP archive, so the fixture is a
/// PK zip whose filename carries the Blackmagic marker.
pub fn braw_fixture() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PK\x03\x04");
    out.extend_from_slice(&[0u8; 22]); // version..sizes fields of the local header
    out.extend_from_slice(&8u16.to_le_bytes()); // filename length
    out.extend_from_slice(&0u16.to_le_bytes()); // extra length
    out.extend_from_slice(b"cam.braw");
    out
}

/// Minimal RED R3D marker: the RED2 magic followed by a padded header.
pub fn r3d_fixture() -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    out.extend_from_slice(b"RED2");
    out.extend_from_slice(&[0u8; 508]);
    out
}

fn bx(four: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(four);
    out.extend_from_slice(payload);
    out
}

/// Minimal QuickTime carrying one video track whose sample description is
/// `codec` (`aprn` = ProRes RAW, `aprh` = ProRes RAW HQ). Detection reads the
/// stsd entry, `mdhd` timing, `stts` frame count and the `colr` nclx tag; no
/// compressed samples are synthesized, so the file is detection-only.
pub fn prores_raw_mov(
    codec: &[u8; 4],
    width: u16,
    height: u16,
    timescale: u32,
    frame_duration: u32,
    frame_count: u32,
) -> Vec<u8> {
    // VideoSampleEntry, 86 bytes, then a colr nclx sub-atom.
    let mut entry = (86u32 + 19).to_be_bytes().to_vec();
    entry.extend_from_slice(codec);
    entry.extend_from_slice(&[0u8; 6]); // reserved
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    entry.extend_from_slice(&[0u8; 16]); // pre_defined + reserved + pre_defined[3]
    entry.extend_from_slice(&width.to_be_bytes()); // +32
    entry.extend_from_slice(&height.to_be_bytes()); // +34
    entry.extend_from_slice(&[0u8; 50]); // resolutions, frame_count, name, depth
    let mut colr = b"nclx".to_vec();
    colr.extend_from_slice(&9u16.to_be_bytes()); // primaries bt2020
    colr.extend_from_slice(&16u16.to_be_bytes()); // transfer linear (arnonstd)
    colr.extend_from_slice(&0u16.to_be_bytes()); // matrix RGB
    colr.push(0x80); // full range
    entry.extend_from_slice(&bx(b"colr", &colr));
    let mut stsd = vec![0u8; 4];
    stsd.extend_from_slice(&1u32.to_be_bytes());
    stsd.extend_from_slice(&entry);
    let mut stts = vec![0u8; 4];
    stts.extend_from_slice(&1u32.to_be_bytes());
    stts.extend_from_slice(&frame_count.to_be_bytes());
    stts.extend_from_slice(&frame_duration.to_be_bytes());
    let stbl = bx(b"stbl", &[bx(b"stsd", &stsd), bx(b"stts", &stts)].concat());
    let minf = bx(b"minf", &stbl);
    let mut mdhd = vec![0u8; 4];
    mdhd.extend_from_slice(&[0u8; 8]); // creation + modification
    mdhd.extend_from_slice(&timescale.to_be_bytes());
    mdhd.extend_from_slice(&(frame_duration * frame_count).to_be_bytes());
    mdhd.extend_from_slice(&[0u8; 4]); // language + pre_defined
    let mut hdlr = vec![0u8; 8];
    hdlr.extend_from_slice(b"vide");
    let mdia = bx(
        b"mdia",
        &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), minf].concat(),
    );
    let trak = bx(b"trak", &mdia);
    let moov = bx(b"moov", &trak);
    let mut ftyp = b"qt  ".to_vec();
    ftyp.extend_from_slice(&[0u8; 4]);
    ftyp.extend_from_slice(b"qt  ");
    [bx(b"ftyp", &ftyp), moov].concat()
}
