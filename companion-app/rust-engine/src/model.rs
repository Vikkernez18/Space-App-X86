use crate::crater::{DetectedCrater, EarthCraterMatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanetaryBody {
    Mars,
    Moon,
}

impl PlanetaryBody {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanetaryBody::Mars => "mars",
            PlanetaryBody::Moon => "moon",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            PlanetaryBody::Mars => "Mars (Jezero–Arcadia Sector)",
            PlanetaryBody::Moon => "Moon (Shackleton–South Pole Rim)",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mars" => Some(PlanetaryBody::Mars),
            "moon" | "luna" => Some(PlanetaryBody::Moon),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GeoBounds {
    pub min_lon: f64,
    pub max_lon: f64,
    pub min_lat: f64,
    pub max_lat: f64,
    pub meters_per_pixel_x: f64,
    pub meters_per_pixel_y: f64,
}

impl GeoBounds {
    pub fn default_for(body: PlanetaryBody, width: usize, height: usize) -> Self {
        match body {
            PlanetaryBody::Mars => {
                // Jezero / Arcadia regional sector on Mars
                let min_lon = 77.20;
                let max_lon = 78.00;
                let min_lat = 18.10;
                let max_lat = 18.90;
                let span_m_x = 47_200.0;
                let span_m_y = 47_200.0;
                GeoBounds {
                    min_lon,
                    max_lon,
                    min_lat,
                    max_lat,
                    meters_per_pixel_x: span_m_x / (width.max(1) as f64),
                    meters_per_pixel_y: span_m_y / (height.max(1) as f64),
                }
            }
            PlanetaryBody::Moon => {
                // Shackleton / South Pole regional sector on the Moon
                let min_lon = -10.00;
                let max_lon = 10.00;
                let min_lat = -89.80;
                let max_lat = -88.20;
                let span_m_x = 32_000.0;
                let span_m_y = 32_000.0;
                GeoBounds {
                    min_lon,
                    max_lon,
                    min_lat,
                    max_lat,
                    meters_per_pixel_x: span_m_x / (width.max(1) as f64),
                    meters_per_pixel_y: span_m_y / (height.max(1) as f64),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerrainGrid {
    pub width: usize,
    pub height: usize,
    /// Elevation in meters (row-major, top-to-bottom: y=0 is max_lat / north)
    pub elevation: Vec<f32>,
    pub bounds: GeoBounds,
    pub source_format: String,
    pub has_geotiff_tags: bool,
    pub nodata_value: Option<f32>,
}

impl TerrainGrid {
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.elevation[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, val: f32) {
        self.elevation[y * self.width + x] = val;
    }

    pub fn min_max_mean(&self) -> (f32, f32, f32) {
        if self.elevation.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        let mut min_v = f32::INFINITY;
        let mut max_v = f32::NEG_INFINITY;
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for &v in &self.elevation {
            if v.is_finite() {
                if v < min_v {
                    min_v = v;
                }
                if v > max_v {
                    max_v = v;
                }
                sum += v as f64;
                count += 1;
            }
        }
        if count == 0 {
            (0.0, 0.0, 0.0)
        } else {
            (min_v, max_v, (sum / count as f64) as f32)
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecommendedSite {
    pub id: String,
    pub name: String,
    pub body: PlanetaryBody,
    /// Normalized coordinates [0.0, 1.0] across the sector grid (u = easting, v = southing from top)
    pub norm_x: f64,
    pub norm_y: f64,
    /// Normalized radius on the sector map
    pub norm_radius: f64,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub radius_km: f64,
    pub rationale: String,
    pub primary_resources: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SuitabilityCriteria {
    /// Maximum safe landing/construction slope angle in degrees (hard penalty above this)
    pub max_slope_deg: f64,
    /// Maximum acceptable Terrain Ruggedness Index (TRI) in meters
    pub max_roughness_m: f64,
    /// Minimum overall viability score [0..100] required to classify a site as viable
    pub min_viability_score: f64,
    /// Minimum Pearson correlation r required to consider an uploaded map spatially aligned with baseline
    pub min_correlation_r: f64,
    /// Weight for slope flatness (0..1)
    pub weight_slope: f64,
    /// Weight for low surface roughness / boulder-free terrain (0..1)
    pub weight_roughness: f64,
    /// Weight for elevation suitability (atmospheric braking on Mars / sunlit altitude on Moon) (0..1)
    pub weight_elevation: f64,
    /// Weight for solar illumination / aspect favorability (0..1)
    pub weight_solar: f64,
}

impl Default for SuitabilityCriteria {
    fn default() -> Self {
        SuitabilityCriteria {
            max_slope_deg: 8.0,
            max_roughness_m: 12.0,
            min_viability_score: 68.0,
            min_correlation_r: 0.45,
            weight_slope: 0.40,
            weight_roughness: 0.30,
            weight_elevation: 0.15,
            weight_solar: 0.15,
        }
    }
}

impl SuitabilityCriteria {
    pub fn normalized_weights(&self) -> (f64, f64, f64, f64) {
        let sum = (self.weight_slope + self.weight_roughness + self.weight_elevation + self.weight_solar)
            .max(1e-6);
        (
            self.weight_slope / sum,
            self.weight_roughness / sum,
            self.weight_elevation / sum,
            self.weight_solar / sum,
        )
    }
}

#[derive(Debug, Clone)]
pub struct ComparisonMetrics {
    pub pearson_r: f64,
    pub ssim: f64,
    pub rmse_m: f64,
    pub mae_m: f64,
    pub max_abs_diff_m: f64,
    pub resolution_ratio: f64,
    pub high_freq_detail_ratio: f64,
    pub alignment_status: String,
}

#[derive(Debug, Clone)]
pub struct SiteEvaluation {
    pub site: RecommendedSite,
    pub baseline_score: f64,
    pub upload_score: f64,
    pub score_delta: f64,
    pub baseline_mean_slope_deg: f64,
    pub upload_mean_slope_deg: f64,
    pub upload_max_slope_deg: f64,
    pub baseline_roughness_m: f64,
    pub upload_roughness_m: f64,
    pub mean_elevation_m: f64,
    pub solar_score: f64,
    pub local_rmse_m: f64,
    pub safe_area_fraction: f64,
    /// CONFIRMED_VIABLE, DOWNGRADED_HAZARD, UPGRADED_VIABLE, MISMATCH_OR_INVALID
    pub status: String,
    pub finding_summary: String,
}

#[derive(Debug, Clone)]
pub struct AnalysisReport {
    pub body: PlanetaryBody,
    pub baseline_file: String,
    pub uploaded_file: String,
    pub uploaded_format: String,
    pub is_imported_map: bool,
    pub baseline_dims: (usize, usize),
    pub uploaded_dims: (usize, usize),
    pub baseline_res_m: f64,
    pub uploaded_res_m: f64,
    pub has_geotiff_tags: bool,
    pub elevation_min_m: f32,
    pub elevation_max_m: f32,
    pub elevation_mean_m: f32,
    pub global_mean_slope_deg: f64,
    pub global_mean_roughness_m: f64,
    pub global_viability_score: f64,
    pub comparison: ComparisonMetrics,
    pub criteria: SuitabilityCriteria,
    pub site_evaluations: Vec<SiteEvaluation>,
    pub detected_craters: Vec<DetectedCrater>,
    pub earth_matches: Vec<EarthCraterMatch>,
}

pub fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

impl AnalysisReport {
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"body\": \"{}\",\n", self.body.as_str()));
        s.push_str(&format!("  \"body_display\": \"{}\",\n", escape_json(self.body.display_name())));
        s.push_str(&format!("  \"baseline_file\": \"{}\",\n", escape_json(&self.baseline_file)));
        s.push_str(&format!("  \"uploaded_file\": \"{}\",\n", escape_json(&self.uploaded_file)));
        s.push_str(&format!("  \"uploaded_format\": \"{}\",\n", escape_json(&self.uploaded_format)));
        s.push_str(&format!("  \"is_imported_map\": {},\n", self.is_imported_map));
        s.push_str(&format!("  \"baseline_width\": {},\n", self.baseline_dims.0));
        s.push_str(&format!("  \"baseline_height\": {},\n", self.baseline_dims.1));
        s.push_str(&format!("  \"uploaded_width\": {},\n", self.uploaded_dims.0));
        s.push_str(&format!("  \"uploaded_height\": {},\n", self.uploaded_dims.1));
        s.push_str(&format!("  \"baseline_res_m\": {:.3},\n", self.baseline_res_m));
        s.push_str(&format!("  \"uploaded_res_m\": {:.3},\n", self.uploaded_res_m));
        s.push_str(&format!("  \"has_geotiff_tags\": {},\n", self.has_geotiff_tags));
        s.push_str(&format!("  \"elevation_min_m\": {:.2},\n", self.elevation_min_m));
        s.push_str(&format!("  \"elevation_max_m\": {:.2},\n", self.elevation_max_m));
        s.push_str(&format!("  \"elevation_mean_m\": {:.2},\n", self.elevation_mean_m));
        s.push_str(&format!("  \"global_mean_slope_deg\": {:.3},\n", self.global_mean_slope_deg));
        s.push_str(&format!("  \"global_mean_roughness_m\": {:.3},\n", self.global_mean_roughness_m));
        s.push_str(&format!("  \"global_viability_score\": {:.2},\n", self.global_viability_score));

        s.push_str("  \"comparison\": {\n");
        s.push_str(&format!("    \"pearson_r\": {:.4},\n", self.comparison.pearson_r));
        s.push_str(&format!("    \"ssim\": {:.4},\n", self.comparison.ssim));
        s.push_str(&format!("    \"rmse_m\": {:.3},\n", self.comparison.rmse_m));
        s.push_str(&format!("    \"mae_m\": {:.3},\n", self.comparison.mae_m));
        s.push_str(&format!("    \"max_abs_diff_m\": {:.3},\n", self.comparison.max_abs_diff_m));
        s.push_str(&format!("    \"resolution_ratio\": {:.3},\n", self.comparison.resolution_ratio));
        s.push_str(&format!("    \"high_freq_detail_ratio\": {:.3},\n", self.comparison.high_freq_detail_ratio));
        s.push_str(&format!("    \"alignment_status\": \"{}\"\n", escape_json(&self.comparison.alignment_status)));
        s.push_str("  },\n");

        s.push_str("  \"criteria\": {\n");
        s.push_str(&format!("    \"max_slope_deg\": {:.2},\n", self.criteria.max_slope_deg));
        s.push_str(&format!("    \"max_roughness_m\": {:.2},\n", self.criteria.max_roughness_m));
        s.push_str(&format!("    \"min_viability_score\": {:.2},\n", self.criteria.min_viability_score));
        s.push_str(&format!("    \"min_correlation_r\": {:.2},\n", self.criteria.min_correlation_r));
        s.push_str(&format!("    \"weight_slope\": {:.3},\n", self.criteria.weight_slope));
        s.push_str(&format!("    \"weight_roughness\": {:.3},\n", self.criteria.weight_roughness));
        s.push_str(&format!("    \"weight_elevation\": {:.3},\n", self.criteria.weight_elevation));
        s.push_str(&format!("    \"weight_solar\": {:.3}\n", self.criteria.weight_solar));
        s.push_str("  },\n");

        s.push_str("  \"site_evaluations\": [\n");
        for (i, ev) in self.site_evaluations.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"id\": \"{}\",\n", escape_json(&ev.site.id)));
            s.push_str(&format!("      \"name\": \"{}\",\n", escape_json(&ev.site.name)));
            s.push_str(&format!("      \"norm_x\": {:.4},\n", ev.site.norm_x));
            s.push_str(&format!("      \"norm_y\": {:.4},\n", ev.site.norm_y));
            s.push_str(&format!("      \"norm_radius\": {:.4},\n", ev.site.norm_radius));
            s.push_str(&format!("      \"lat_deg\": {:.4},\n", ev.site.lat_deg));
            s.push_str(&format!("      \"lon_deg\": {:.4},\n", ev.site.lon_deg));
            s.push_str(&format!("      \"radius_km\": {:.2},\n", ev.site.radius_km));
            s.push_str(&format!("      \"rationale\": \"{}\",\n", escape_json(&ev.site.rationale)));
            s.push_str(&format!("      \"primary_resources\": \"{}\",\n", escape_json(&ev.site.primary_resources)));
            s.push_str(&format!("      \"baseline_score\": {:.2},\n", ev.baseline_score));
            s.push_str(&format!("      \"upload_score\": {:.2},\n", ev.upload_score));
            s.push_str(&format!("      \"score_delta\": {:.2},\n", ev.score_delta));
            s.push_str(&format!("      \"baseline_mean_slope_deg\": {:.2},\n", ev.baseline_mean_slope_deg));
            s.push_str(&format!("      \"upload_mean_slope_deg\": {:.2},\n", ev.upload_mean_slope_deg));
            s.push_str(&format!("      \"upload_max_slope_deg\": {:.2},\n", ev.upload_max_slope_deg));
            s.push_str(&format!("      \"baseline_roughness_m\": {:.2},\n", ev.baseline_roughness_m));
            s.push_str(&format!("      \"upload_roughness_m\": {:.2},\n", ev.upload_roughness_m));
            s.push_str(&format!("      \"mean_elevation_m\": {:.2},\n", ev.mean_elevation_m));
            s.push_str(&format!("      \"solar_score\": {:.2},\n", ev.solar_score));
            s.push_str(&format!("      \"local_rmse_m\": {:.2},\n", ev.local_rmse_m));
            s.push_str(&format!("      \"safe_area_fraction\": {:.4},\n", ev.safe_area_fraction));
            s.push_str(&format!("      \"status\": \"{}\",\n", escape_json(&ev.status)));
            s.push_str(&format!("      \"finding_summary\": \"{}\"\n", escape_json(&ev.finding_summary)));
            if i + 1 < self.site_evaluations.len() {
                s.push_str("    },\n");
            } else {
                s.push_str("    }\n");
            }
        }
        s.push_str("  ],\n");

        s.push_str("  \"detected_craters\": [\n");
        for (i, dc) in self.detected_craters.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"id\": \"{}\",\n", escape_json(&dc.id)));
            s.push_str(&format!("      \"norm_x\": {:.4},\n", dc.norm_x));
            s.push_str(&format!("      \"norm_y\": {:.4},\n", dc.norm_y));
            s.push_str(&format!("      \"norm_radius\": {:.4},\n", dc.norm_radius));
            s.push_str(&format!("      \"diameter_px\": {:.1},\n", dc.diameter_px));
            s.push_str(&format!("      \"depth_val\": {:.2},\n", dc.depth_val));
            s.push_str(&format!("      \"rim_height_val\": {:.2},\n", dc.rim_height_val));
            s.push_str(&format!("      \"depth_to_diameter\": {:.4},\n", dc.depth_to_diameter));
            s.push_str(&format!("      \"rim_to_depth\": {:.3},\n", dc.rim_to_depth));
            s.push_str(&format!("      \"circularity\": {:.3},\n", dc.circularity));
            s.push_str(&format!("      \"floor_flatness\": {:.3},\n", dc.floor_flatness));
            s.push_str(&format!("      \"central_peak\": {:.3},\n", dc.central_peak));
            s.push_str(&format!("      \"wall_slope_deg\": {:.2},\n", dc.wall_slope_deg));
            s.push_str(&format!("      \"confidence\": {:.1},\n", dc.confidence));
            s.push_str(&format!("      \"ice_potential_score\": {:.1},\n", dc.ice_potential_score));
            s.push_str(&format!("      \"ice_potential_label\": \"{}\",\n", escape_json(&dc.ice_potential_label)));
            s.push_str(&format!("      \"ice_rationale\": \"{}\"\n", escape_json(&dc.ice_rationale)));
            if i + 1 < self.detected_craters.len() {
                s.push_str("    },\n");
            } else {
                s.push_str("    }\n");
            }
        }
        s.push_str("  ],\n");

        s.push_str("  \"earth_crater_matches\": [\n");
        for (i, em) in self.earth_matches.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"id\": \"{}\",\n", escape_json(em.crater.id)));
            s.push_str(&format!("      \"name\": \"{}\",\n", escape_json(em.crater.name)));
            s.push_str(&format!("      \"location\": \"{}\",\n", escape_json(em.crater.location)));
            s.push_str(&format!("      \"country\": \"{}\",\n", escape_json(em.crater.country)));
            s.push_str(&format!("      \"morphology\": \"{}\",\n", escape_json(em.crater.morphology)));
            s.push_str(&format!("      \"diameter_km\": {:.3},\n", em.crater.diameter_km));
            s.push_str(&format!("      \"depth_m\": {:.1},\n", em.crater.depth_m));
            s.push_str(&format!("      \"similarity_score\": {:.2},\n", em.similarity_score));
            s.push_str(&format!("      \"profile_correlation\": {:.4},\n", em.profile_correlation));
            s.push_str(&format!("      \"ice_analog_notes\": \"{}\",\n", escape_json(em.crater.ice_analog_notes)));
            s.push_str(&format!("      \"geology_notes\": \"{}\"\n", escape_json(em.crater.geology_notes)));
            if i + 1 < self.earth_matches.len() {
                s.push_str("    },\n");
            } else {
                s.push_str("    }\n");
            }
        }
        s.push_str("  ]\n");
        s.push_str("}\n");
        s
    }

    /// Line-oriented KV format for zero-dependency Java parsing alongside JSON
    pub fn to_kv_lines(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("BODY={}\n", self.body.as_str()));
        out.push_str(&format!("BODY_DISPLAY={}\n", self.body.display_name()));
        out.push_str(&format!("BASELINE_FILE={}\n", self.baseline_file));
        out.push_str(&format!("UPLOADED_FILE={}\n", self.uploaded_file));
        out.push_str(&format!("UPLOADED_FORMAT={}\n", self.uploaded_format));
        out.push_str(&format!("IS_IMPORTED_MAP={}\n", self.is_imported_map));
        out.push_str(&format!("BASELINE_WIDTH={}\n", self.baseline_dims.0));
        out.push_str(&format!("BASELINE_HEIGHT={}\n", self.baseline_dims.1));
        out.push_str(&format!("UPLOADED_WIDTH={}\n", self.uploaded_dims.0));
        out.push_str(&format!("UPLOADED_HEIGHT={}\n", self.uploaded_dims.1));
        out.push_str(&format!("BASELINE_RES_M={:.3}\n", self.baseline_res_m));
        out.push_str(&format!("UPLOADED_RES_M={:.3}\n", self.uploaded_res_m));
        out.push_str(&format!("HAS_GEOTIFF_TAGS={}\n", self.has_geotiff_tags));
        out.push_str(&format!("ELEV_MIN_M={:.2}\n", self.elevation_min_m));
        out.push_str(&format!("ELEV_MAX_M={:.2}\n", self.elevation_max_m));
        out.push_str(&format!("ELEV_MEAN_M={:.2}\n", self.elevation_mean_m));
        out.push_str(&format!("GLOBAL_MEAN_SLOPE_DEG={:.3}\n", self.global_mean_slope_deg));
        out.push_str(&format!("GLOBAL_MEAN_ROUGHNESS_M={:.3}\n", self.global_mean_roughness_m));
        out.push_str(&format!("GLOBAL_VIABILITY_SCORE={:.2}\n", self.global_viability_score));
        out.push_str(&format!("PEARSON_R={:.4}\n", self.comparison.pearson_r));
        out.push_str(&format!("SSIM={:.4}\n", self.comparison.ssim));
        out.push_str(&format!("RMSE_M={:.3}\n", self.comparison.rmse_m));
        out.push_str(&format!("MAE_M={:.3}\n", self.comparison.mae_m));
        out.push_str(&format!("MAX_ABS_DIFF_M={:.3}\n", self.comparison.max_abs_diff_m));
        out.push_str(&format!("RESOLUTION_RATIO={:.3}\n", self.comparison.resolution_ratio));
        out.push_str(&format!("HIGH_FREQ_DETAIL_RATIO={:.3}\n", self.comparison.high_freq_detail_ratio));
        out.push_str(&format!("ALIGNMENT_STATUS={}\n", self.comparison.alignment_status));
        out.push_str(&format!("SITE_COUNT={}\n", self.site_evaluations.len()));
        for (i, ev) in self.site_evaluations.iter().enumerate() {
            out.push_str(&format!("SITE_{}_ID={}\n", i, ev.site.id));
            out.push_str(&format!("SITE_{}_NAME={}\n", i, ev.site.name));
            out.push_str(&format!("SITE_{}_NORM_X={:.4}\n", i, ev.site.norm_x));
            out.push_str(&format!("SITE_{}_NORM_Y={:.4}\n", i, ev.site.norm_y));
            out.push_str(&format!("SITE_{}_NORM_RADIUS={:.4}\n", i, ev.site.norm_radius));
            out.push_str(&format!("SITE_{}_LAT={:.4}\n", i, ev.site.lat_deg));
            out.push_str(&format!("SITE_{}_LON={:.4}\n", i, ev.site.lon_deg));
            out.push_str(&format!("SITE_{}_RADIUS_KM={:.2}\n", i, ev.site.radius_km));
            out.push_str(&format!("SITE_{}_RATIONALE={}\n", i, ev.site.rationale));
            out.push_str(&format!("SITE_{}_RESOURCES={}\n", i, ev.site.primary_resources));
            out.push_str(&format!("SITE_{}_BASELINE_SCORE={:.2}\n", i, ev.baseline_score));
            out.push_str(&format!("SITE_{}_UPLOAD_SCORE={:.2}\n", i, ev.upload_score));
            out.push_str(&format!("SITE_{}_SCORE_DELTA={:.2}\n", i, ev.score_delta));
            out.push_str(&format!("SITE_{}_BASE_SLOPE={:.2}\n", i, ev.baseline_mean_slope_deg));
            out.push_str(&format!("SITE_{}_UPLOAD_SLOPE={:.2}\n", i, ev.upload_mean_slope_deg));
            out.push_str(&format!("SITE_{}_MAX_SLOPE={:.2}\n", i, ev.upload_max_slope_deg));
            out.push_str(&format!("SITE_{}_BASE_ROUGH={:.2}\n", i, ev.baseline_roughness_m));
            out.push_str(&format!("SITE_{}_UPLOAD_ROUGH={:.2}\n", i, ev.upload_roughness_m));
            out.push_str(&format!("SITE_{}_MEAN_ELEV={:.2}\n", i, ev.mean_elevation_m));
            out.push_str(&format!("SITE_{}_SOLAR={:.2}\n", i, ev.solar_score));
            out.push_str(&format!("SITE_{}_LOCAL_RMSE={:.2}\n", i, ev.local_rmse_m));
            out.push_str(&format!("SITE_{}_SAFE_FRAC={:.4}\n", i, ev.safe_area_fraction));
            out.push_str(&format!("SITE_{}_STATUS={}\n", i, ev.status));
            out.push_str(&format!("SITE_{}_FINDING={}\n", i, ev.finding_summary));
        }
        out.push_str(&format!("CRATER_COUNT={}\n", self.detected_craters.len()));
        for (i, dc) in self.detected_craters.iter().enumerate() {
            out.push_str(&format!("CRATER_{}_ID={}\n", i, dc.id));
            out.push_str(&format!("CRATER_{}_NORM_X={:.4}\n", i, dc.norm_x));
            out.push_str(&format!("CRATER_{}_NORM_Y={:.4}\n", i, dc.norm_y));
            out.push_str(&format!("CRATER_{}_NORM_RADIUS={:.4}\n", i, dc.norm_radius));
            out.push_str(&format!("CRATER_{}_DIAMETER_PX={:.1}\n", i, dc.diameter_px));
            out.push_str(&format!("CRATER_{}_DEPTH_VAL={:.2}\n", i, dc.depth_val));
            out.push_str(&format!("CRATER_{}_RIM_VAL={:.2}\n", i, dc.rim_height_val));
            out.push_str(&format!("CRATER_{}_D_TO_D={:.4}\n", i, dc.depth_to_diameter));
            out.push_str(&format!("CRATER_{}_RIM_TO_D={:.3}\n", i, dc.rim_to_depth));
            out.push_str(&format!("CRATER_{}_CIRC={:.3}\n", i, dc.circularity));
            out.push_str(&format!("CRATER_{}_FLAT={:.3}\n", i, dc.floor_flatness));
            out.push_str(&format!("CRATER_{}_PEAK={:.3}\n", i, dc.central_peak));
            out.push_str(&format!("CRATER_{}_SLOPE={:.2}\n", i, dc.wall_slope_deg));
            out.push_str(&format!("CRATER_{}_CONF={:.1}\n", i, dc.confidence));
            out.push_str(&format!("CRATER_{}_ICE_SCORE={:.1}\n", i, dc.ice_potential_score));
            out.push_str(&format!("CRATER_{}_ICE_LABEL={}\n", i, dc.ice_potential_label));
            out.push_str(&format!("CRATER_{}_ICE_NOTE={}\n", i, dc.ice_rationale));
            let prof_csv: Vec<String> = dc.radial_profile.iter().map(|v| format!("{:.4}", v)).collect();
            out.push_str(&format!("CRATER_{}_PROFILE={}\n", i, prof_csv.join(",")));
        }
        out.push_str(&format!("EARTH_MATCH_COUNT={}\n", self.earth_matches.len()));
        for (i, em) in self.earth_matches.iter().enumerate() {
            out.push_str(&format!("EMATCH_{}_ID={}\n", i, em.crater.id));
            out.push_str(&format!("EMATCH_{}_NAME={}\n", i, em.crater.name));
            out.push_str(&format!("EMATCH_{}_LOC={}\n", i, em.crater.location));
            out.push_str(&format!("EMATCH_{}_COUNTRY={}\n", i, em.crater.country));
            out.push_str(&format!("EMATCH_{}_LAT={:.4}\n", i, em.crater.lat_deg));
            out.push_str(&format!("EMATCH_{}_LON={:.4}\n", i, em.crater.lon_deg));
            out.push_str(&format!("EMATCH_{}_DIAM_KM={:.3}\n", i, em.crater.diameter_km));
            out.push_str(&format!("EMATCH_{}_DEPTH_M={:.1}\n", i, em.crater.depth_m));
            out.push_str(&format!("EMATCH_{}_MORPH={}\n", i, em.crater.morphology));
            out.push_str(&format!("EMATCH_{}_D_TO_D={:.4}\n", i, em.crater.depth_to_diameter));
            out.push_str(&format!("EMATCH_{}_RIM_TO_D={:.3}\n", i, em.crater.rim_to_depth));
            out.push_str(&format!("EMATCH_{}_CIRC={:.3}\n", i, em.crater.circularity));
            out.push_str(&format!("EMATCH_{}_FLAT={:.3}\n", i, em.crater.floor_flatness));
            out.push_str(&format!("EMATCH_{}_PEAK={:.3}\n", i, em.crater.central_peak));
            out.push_str(&format!("EMATCH_{}_SLOPE={:.1}\n", i, em.crater.wall_slope_deg));
            out.push_str(&format!("EMATCH_{}_SIM={:.2}\n", i, em.similarity_score));
            out.push_str(&format!("EMATCH_{}_CORR={:.4}\n", i, em.profile_correlation));
            out.push_str(&format!("EMATCH_{}_RMSE={:.4}\n", i, em.feature_rmse));
            out.push_str(&format!("EMATCH_{}_ICE_NOTE={}\n", i, em.crater.ice_analog_notes));
            out.push_str(&format!("EMATCH_{}_GEO_NOTE={}\n", i, em.crater.geology_notes));
            let prof_csv: Vec<String> = em.crater.radial_profile.iter().map(|v| format!("{:.4}", v)).collect();
            out.push_str(&format!("EMATCH_{}_PROFILE={}\n", i, prof_csv.join(",")));
        }
        out
    }
}
