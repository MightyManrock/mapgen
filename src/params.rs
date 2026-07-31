/// Trimmed planet-generation params: only the fields the elevation, climate,
/// and hydrology pipeline actually consumes. No per-species, atmosphere, or
/// region-detection generality — Earth-like/human is the only target.
pub struct PlanetGenParams {
    pub warp_strength: f64,
    /// Fraction of the planet's surface *area* (cos-lat weighted, not cell
    /// count) to place above sea level. `None` disables the solve and uses
    /// `sea_level` literally, preserving pre-normalization behavior.
    ///
    /// A target, not a guarantee: `roughen_coastline` and `flood_fill_ocean`
    /// run afterward and push the realized figure up by 0.1–1.9 points.
    pub target_land_fraction: Option<f64>,
    /// DERIVED, not an input, whenever `target_land_fraction` is `Some` —
    /// `generate_elevation` normalizes the field about the solved sea level,
    /// so this must then be 0.5. Only meaningful as an input when the target
    /// is `None`.
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
    pub radius_km: f64,
    pub land_threshold: f64,
    pub ocean_threshold: f64,
    pub region_min_size: usize,
    pub island_coast_dist: usize,
    pub island_arch_dist: usize,
    pub lon_weight: f64,
    pub current_temp_bias: f64,
    pub current_search_dist: usize,
    pub current_bleed_dist: usize,
    // Freshwater greening (render-only vegetation shift, not a climate change)
    pub greening_radius: usize,
    pub greening_aquifer_strength: f64,
    pub greening_ocean_damp_dist: usize,
    pub greening_strength: f64,
    pub greening_temp_floor: f64,
    pub greening_temp_full: f64,
    // Lake-effect climate (one-way, applied after hydrology)
    pub lake_effect_min_size: usize,
    pub lake_halo_dist: usize,
    pub lake_precip_boost: f64,
    pub lake_swing_damp: f64,
    // Coastline character (render-only)
    pub shore_mouth_radius: usize,
    pub shore_cliff_slope: f64,
    // Region polygon export
    pub polygon_simplify_epsilon: f64,
}

impl PlanetGenParams {
    /// Earth-analog defaults, adapted from demiurge-rust's `PlanetParams::earth_like()`.
    pub fn earth_like() -> Self {
        Self {
            warp_strength: 0.2,
            target_land_fraction: Some(0.30),
            // Derived: overwritten to 0.5 by the normalization above.
            sea_level: 0.5,
            axial_tilt: 23.5,
            temp_baseline: 1.0,
            temp_gradient: 1.0,
            // Calibrated against the *above-sea-level* lapse reference, which
            // averages ~0.24 over land where the old raw-elevation reference
            // averaged ~0.7. 0.3 under the new reference leaves the world far
            // too warm (mean land T 0.80 vs 0.68 baseline, glaciers all but
            // gone); 0.70 holds temperature within noise of the old behavior.
            lapse_factor: 0.7,
            sea_ice_evap_factor: 0.25,
            precip_moisture: 1.0,
            land_decay: 0.985,
            // Scaled by the 1.539x land-relief factor from normalization, to
            // preserve prior behavior under the new fixed scale. NOTE this
            // keeps the rain shadow inert: post-normalization p95 per-cell
            // land gradient is ~0.011, still below this. Deliberate — this
            // change must not alter orographic behavior. Activating the rain
            // shadow wants ~0.004-0.006 plus a windward gain term.
            slope_threshold: 0.023,
            slope_loss: 0.5,
            base_arid: 0.05,
            et_factor: 0.35,
            rotation_period: 1.0,
            glacier_temp_threshold: 0.20,
            sea_ice_temp_threshold: 0.14,
            // Also scaled by 1.539x — it is an absolute fill depth in
            // elevation units, used both as the lake-vs-endorheic cutoff and
            // to normalize lake depth for the hydrology encoding.
            max_lake_fill: 0.062,
            river_threshold: 400.0,
            aquifer_probability: 0.35,
            glacier_melt_factor: 2.5,
            radius_km: 6371.0,
            land_threshold: 0.15,
            ocean_threshold: 0.59,
            region_min_size: 150,
            island_coast_dist: 3,
            island_arch_dist: 25,
            lon_weight: 0.82,
            current_temp_bias: 0.10,
            current_search_dist: 40,
            current_bleed_dist: 8,
            greening_radius: 6,
            greening_aquifer_strength: 0.4,
            greening_ocean_damp_dist: 5,
            greening_strength: 0.35,
            greening_temp_floor: 0.30,
            greening_temp_full: 0.45,
            lake_effect_min_size: 12,
            lake_halo_dist: 6,
            lake_precip_boost: 0.08,
            lake_swing_damp: 0.4,
            shore_mouth_radius: 3,
            shore_cliff_slope: 0.012,
            polygon_simplify_epsilon: 1.2,
        }
    }
}
