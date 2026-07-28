/// Trimmed planet-generation params: only the fields the elevation, climate,
/// and hydrology pipeline actually consumes. No per-species, atmosphere, or
/// region-detection generality — Earth-like/human is the only target.
pub struct PlanetGenParams {
    pub warp_strength: f64,
    pub sea_level: f64,
    pub axial_tilt: f64,
    pub temp_baseline: f64,
    pub temp_gradient: f64,
    pub lapse_factor: f64,
    pub sea_ice_evap_factor: f64,
    pub precip_moisture: f64,
    pub land_decay: f64,
    pub slope_threshold: f64,
    pub slope_loss: f64,
    pub base_arid: f64,
    pub et_factor: f64,
    pub rotation_period: f64,
    pub glacier_temp_threshold: f64,
    pub sea_ice_temp_threshold: f64,
    pub max_lake_fill: f64,
    pub river_threshold: f64,
    pub aquifer_probability: f64,
    pub glacier_melt_factor: f64,
}

impl PlanetGenParams {
    /// Earth-analog defaults, adapted from demiurge-rust's `PlanetParams::earth_like()`.
    pub fn earth_like() -> Self {
        Self {
            warp_strength: 0.2,
            sea_level: 0.5,
            axial_tilt: 23.5,
            temp_baseline: 1.0,
            temp_gradient: 1.0,
            lapse_factor: 0.3,
            sea_ice_evap_factor: 0.25,
            precip_moisture: 1.0,
            land_decay: 0.985,
            slope_threshold: 0.015,
            slope_loss: 0.5,
            base_arid: 0.05,
            et_factor: 0.35,
            rotation_period: 1.0,
            glacier_temp_threshold: 0.20,
            sea_ice_temp_threshold: 0.14,
            max_lake_fill: 0.04,
            river_threshold: 400.0,
            aquifer_probability: 0.35,
            glacier_melt_factor: 2.5,
        }
    }
}
