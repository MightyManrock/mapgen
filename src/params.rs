/// Trimmed planet-generation params: only the fields the elevation, climate,
/// and hydrology pipeline actually consumes. No per-species, atmosphere, or
/// region-detection generality — Earth-like/human is the only target.
pub struct PlanetGenParams {
    /// How many continent-scale noise wavelengths span the equator. Lower
    /// means fewer, larger landmasses.
    ///
    /// Measured across 6 seeds, paired with `warp_strength`:
    ///
    /// ```text
    ///  wavelengths / warp | landmasses | largest % of land | character
    ///           2.5 / 0.20 |        1.2 |             97.5 | Pangaea
    ///           5.5 / 0.65 |        4.2 |             58.0 | Earth-like (default)
    ///          11.0 / 0.65 |        9.5 |             24.6 | archipelago
    ///          18.0 / 0.70 |       14.2 |             21.7 | island world
    /// ```
    ///
    /// Earth for comparison: ~5 major landmasses, largest ~57% of land area.
    ///
    /// This and `warp_strength` are **not independent** — see that field.
    pub continent_wavelengths: f64,
    /// Strength of the domain warp applied to the elevation sampling
    /// coordinates.
    ///
    /// It does **not** shrink continents, it *indents* them: at 5.5
    /// wavelengths, raising warp 0.20 -> 0.65 increases the largest landmass
    /// (51.7% -> 58.0% of land) while reducing how far inland the interior
    /// gets. It buys bays and peninsulas, not fragmentation.
    ///
    /// It also only helps once `continent_wavelengths` is high enough: at 4.5
    /// wavelengths, warp 0.65 still leaves 9.2% of land further from the ocean
    /// than Earth's most continental point. Raise the wavelength count first,
    /// then use warp for coastline character.
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
    /// Fraction of airmass moisture retained per 1000 km of overland travel.
    ///
    /// Per *distance*, not per cell: a cell spans `39.1 * cos(lat)` km, so a
    /// per-cell figure decays moisture ~1.75x faster per km at 55 deg than at
    /// the equator, and changes meaning with render resolution.
    pub land_decay: f64,
    /// Moisture level, as a fraction of local temperature, that land sustains
    /// through evapotranspiration. The airmass relaxes toward
    /// `land_recycle_floor * temperature` instead of toward zero, so
    /// continental interiors stay habitable. 0.0 disables recycling.
    pub land_recycle_floor: f64,
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
    /// Radius, in rows, of the meridional box blur over the current-bias field.
    /// The bias is derived from a per-row coastline search with no north-south
    /// coupling, which streaks; this supplies it. 0 disables smoothing.
    pub current_smooth_rows: usize,
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
            // 5.5 / 0.65 gives ~4 continents with the largest holding ~58% of
            // land, against Earth's ~5 and ~57%. The previous 3.5 / 0.20 put
            // 90.6% of land in a single mass, leaving 18% of it further from
            // the ocean than anywhere on Earth.
            continent_wavelengths: 5.5,
            warp_strength: 0.65,
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
            // Per 1000 km. 0.985^(1000/39.1) — i.e. exactly reproduces the old
            // per-cell 0.985 at the equator, where a cell is 39.1 km wide.
            // Everything poleward of that gets wetter, which was the bug.
            land_decay: 0.68,
            land_recycle_floor: 0.25,
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
            current_smooth_rows: 4,
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
