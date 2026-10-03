use kronello_model::{Color, ColorSpace, ModelError};

// Version-1 D65 primary matrices share the GPU reference's coefficients, but
// pure animation uses f64 and never depends on GPU types or premultiplies RGB.
const REC709_TO_REC2020: [[f64; 3]; 3] = [
    [0.627404, 0.329282, 0.0433136],
    [0.0690973, 0.9195404, 0.0113623],
    [0.0163914, 0.0880133, 0.8955953],
];
const REC2020_TO_REC709: [[f64; 3]; 3] = [
    [1.660491, -0.5876411, -0.0728499],
    [-0.1245505, 1.1328999, -0.0083494],
    [-0.0181508, -0.1005789, 1.1187297],
];

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn to_working(color: Color, target: ColorSpace) -> Result<Color, ModelError> {
    let c = color.components();
    let mut rgb = [c.r.get(), c.g.get(), c.b.get()];
    let source = if color.space() == ColorSpace::Srgb {
        rgb = rgb.map(srgb_decode);
        ColorSpace::LinearRec709
    } else {
        color.space()
    };
    if source != target {
        let matrix = if source == ColorSpace::LinearRec709 {
            REC709_TO_REC2020
        } else {
            REC2020_TO_REC709
        };
        rgb = matrix.map(|row| row.iter().zip(rgb).map(|(a, b)| a * b).sum());
    }
    Color::new(target, rgb, c.alpha.get())
}
