use crate::params::PlanetGenParams;
use serde::Serialize;
use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::fs;

#[derive(Serialize)]
struct Size {
    w: usize,
    h: usize,
}

#[derive(Serialize)]
struct Layer {
    id: &'static str,
    name: &'static str,
    visible: bool,
    locked: bool,
}

#[derive(Serialize)]
struct Measurement {
    scales: BTreeMap<String, f64>,
    #[serde(rename = "customUnitPxPerUnit")]
    custom_unit_px_per_unit: BTreeMap<String, f64>,
    #[serde(rename = "travelTimePresetIds")]
    travel_time_preset_ids: Vec<String>,
    #[serde(rename = "travelDaysEnabled")]
    travel_days_enabled: bool,
}

#[derive(Serialize)]
struct SecondScreen {}

#[derive(Serialize)]
struct Marker {
    id: String,
    x: f64,
    y: f64,
    layer: &'static str,
    link: &'static str,
    #[serde(rename = "iconKey")]
    icon_key: &'static str,
    tooltip: String,
}

#[derive(Serialize)]
struct MarkersFile {
    size: Size,
    layers: Vec<Layer>,
    markers: Vec<Marker>,
    bases: Vec<String>,
    overlays: Vec<()>,
    #[serde(rename = "activeBase")]
    active_base: String,
    measurement: Measurement,
    #[serde(rename = "pinSizeOverrides")]
    pin_size_overrides: BTreeMap<String, f64>,
    grids: Vec<()>,
    #[serde(rename = "panClamp")]
    pan_clamp: bool,
    #[serde(rename = "drawLayers")]
    draw_layers: Vec<()>,
    drawings: Vec<()>,
    #[serde(rename = "secondScreen")]
    second_screen: SecondScreen,
    #[serde(rename = "textLayers")]
    text_layers: Vec<()>,
}

/// Pole-to-pole distance divided by render height: the vertical scale is
/// constant across an equirectangular map, unlike the horizontal scale
/// (which shrinks by cos(latitude) toward the poles). See
/// docs/superpowers/specs/2026-07-28-obsidian-markers-export-design.md.
fn meters_per_pixel(params: &PlanetGenParams, render_height: usize) -> f64 {
    (PI * params.radius_km * 1000.0) / render_height as f64
}

pub fn save_markers_json(
    render_width: usize,
    render_height: usize,
    params: &PlanetGenParams,
    image_filename: &str,
    regions: &[crate::regions::Region],
    grid_width: usize,
    grid_height: usize,
    path: &str,
) -> std::io::Result<()> {
    let mut scales = BTreeMap::new();
    scales.insert(
        image_filename.to_string(),
        meters_per_pixel(params, render_height),
    );

    let markers: Vec<Marker> = regions
        .iter()
        .filter(|r| r.kind == crate::regions::CellKind::Land)
        .map(|r| Marker {
            id: format!("region_{}", r.id),
            x: (r.label_pos.0 as f64 + 0.5) / grid_width as f64,
            y: (r.label_pos.1 as f64 + 0.5) / grid_height as f64,
            layer: "default",
            link: "",
            icon_key: "pinRed",
            tooltip: r.character(),
        })
        .collect();

    let data = MarkersFile {
        size: Size {
            w: render_width,
            h: render_height,
        },
        layers: vec![Layer {
            id: "default",
            name: "Default",
            visible: true,
            locked: false,
        }],
        markers,
        bases: vec![image_filename.to_string()],
        overlays: vec![],
        active_base: image_filename.to_string(),
        measurement: Measurement {
            scales,
            custom_unit_px_per_unit: BTreeMap::new(),
            travel_time_preset_ids: vec![],
            travel_days_enabled: false,
        },
        pin_size_overrides: BTreeMap::new(),
        grids: vec![],
        pan_clamp: true,
        draw_layers: vec![],
        drawings: vec![],
        second_screen: SecondScreen {},
        text_layers: vec![],
    };

    let json = serde_json::to_string_pretty(&data).expect("MarkersFile serialization is infallible");
    fs::write(path, json)
}
