use kronello_model::*;
use std::collections::BTreeMap;

fn f(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn gradient() -> (Gradient, BTreeMap<PropertyId, Value>) {
    let ids = [
        PropertyId::new(),
        PropertyId::new(),
        PropertyId::new(),
        PropertyId::new(),
    ];
    (
        Gradient::Linear {
            start: [f(0.0); 2],
            end: [f(1.0), f(0.0)],
            options: Default::default(),
            stops: vec![
                GradientStop {
                    color: ids[0],
                    offset: ids[1],
                },
                GradientStop {
                    color: ids[2],
                    offset: ids[3],
                },
            ],
        },
        BTreeMap::from([
            (ids[0], Value::Color(Color::from_srgb8([0; 3], None))),
            (ids[1], Value::Scalar(f(0.0))),
            (ids[2], Value::Color(Color::from_srgb8([255; 3], None))),
            (ids[3], Value::Scalar(f(1.0))),
        ]),
    )
}
#[test]
fn vec004_legacy_defaults_explicit_options_and_unknown_fields() {
    let (g, values) = gradient();
    let mut json = serde_json::to_value(&g).unwrap();
    json.as_object_mut().unwrap().remove("options");
    let old: Gradient = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(old, g);
    let options = GradientOptions {
        spread: GradientSpread::Reflect,
        interpolation: GradientInterpolation::SrgbStraight,
        units: GradientUnits::ObjectBoundingBox,
        ..Default::default()
    };
    json["options"] = serde_json::to_value(&options).unwrap();
    let explicit: Gradient = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(explicit.resolve(&values).unwrap().options, options);
    json["options"]["interpolation_version"] = 2.into();
    assert_eq!(
        serde_json::from_value::<Gradient>(json.clone())
            .unwrap()
            .resolve(&values)
            .unwrap_err(),
        ShapeError::UnsupportedGradientVersion
    );
    json["options"]["future"] = true.into();
    assert!(serde_json::from_value::<Gradient>(json).is_err());
}
#[test]
fn vec004_decimal_json_roundtrips_all_variants_and_rejects_duplicate_fields() {
    let (g, _) = gradient();
    let options = GradientOptions {
        spread: GradientSpread::Repeat,
        interpolation: GradientInterpolation::SrgbPremultiplied,
        transform: [[f(0.8), f(0.15), f(0.05)], [f(-0.1), f(0.9), f(0.05)]],
        ..Default::default()
    };
    let variants = [
        Gradient::Linear {
            start: [f(0.25), f(-0.5)],
            end: [f(1.5), f(0.75)],
            stops: g.stops().to_vec(),
            options: options.clone(),
        },
        Gradient::Radial {
            center: [f(0.5); 2],
            radius: f(0.4),
            stops: g.stops().to_vec(),
            options: options.clone(),
        },
        Gradient::FocalRadial {
            center: [f(0.5); 2],
            radius: f(0.4),
            focal: [f(0.4), f(0.5)],
            focal_radius: f(0.05),
            stops: g.stops().to_vec(),
            options: options.clone(),
        },
        Gradient::Conic {
            center: [f(0.5); 2],
            start_angle: f(20.25),
            sweep_angle: f(240.5),
            stops: g.stops().to_vec(),
            options,
        },
    ];
    for g in variants {
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Gradient>(&json).unwrap(), g);
        let duplicated = format!("{{\"kind\":\"linear\",{}", &json[1..]);
        assert!(serde_json::from_str::<Gradient>(&duplicated).is_err());
        let mut extra = serde_json::to_value(&g).unwrap();
        extra["unknown"] = true.into();
        assert!(serde_json::from_str::<Gradient>(&extra.to_string()).is_err());
    }
}

#[test]
fn vec004_invalid_geometry_and_stop_values_are_typed() {
    let (g, mut values) = gradient();
    let stops = g.stops().to_vec();
    let focal = Gradient::FocalRadial {
        center: [f(0.0); 2],
        radius: f(2.0),
        focal: [f(1.0), f(0.0)],
        focal_radius: f(1.0),
        stops: stops.clone(),
        options: Default::default(),
    };
    assert_eq!(
        focal.resolve(&values).unwrap_err(),
        ShapeError::InvalidGradient
    );
    let conic = Gradient::Conic {
        center: [f(0.0); 2],
        start_angle: f(90.0),
        sweep_angle: f(361.0),
        stops,
        options: Default::default(),
    };
    assert_eq!(
        conic.resolve(&values).unwrap_err(),
        ShapeError::InvalidGradient
    );
    values.insert(g.stops()[0].offset, Value::Scalar(f(-0.1)));
    assert!(matches!(
        g.resolve(&values),
        Err(ShapeError::InvalidParameter { .. })
    ));
    values.remove(&g.stops()[0].offset);
    assert!(matches!(
        g.resolve(&values),
        Err(ShapeError::MissingProperty { .. })
    ));
}
