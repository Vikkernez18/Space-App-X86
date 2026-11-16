use std::f64::consts::PI;
use std::fs;
use std::path::Path;

use crate::model::{escape_json, GeoBounds, PlanetaryBody, TerrainGrid};

#[derive(Debug, Clone)]
pub struct EarthCrater {
    pub id: &'static str,
    pub name: &'static str,
    pub location: &'static str,
    pub country: &'static str,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub diameter_km: f64,
    pub depth_m: f64,
    pub morphology: &'static str,
    pub depth_to_diameter: f64,
    pub rim_to_depth: f64,
    pub circularity: f64,
    pub floor_flatness: f64,
    pub central_peak: f64,
    pub wall_slope_deg: f64,
    /// Normalized radial elevation profile at 16 evenly spaced radii r/R in [0.0 .. 1.5]
    /// Values normalized such that outer background = 0.0, floor = -1.0, rim peak > 0.0
    pub radial_profile: [f32; 16],
    pub ice_analog_notes: &'static str,
    pub geology_notes: &'static str,
}

/// Build a canonical 16-bin radial profile (r/R from 0.0 to 1.5 in steps of 0.1)
/// from morphological crater parameters.
const fn build_profile(
    rim_ratio: f32,
    floor_flatness: f32,
    central_peak: f32,
) -> [f32; 16] {
    // Radii: 0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5
    let mut prof = [0.0f32; 16];
    let mut i = 0;
    while i < 16 {
        let r = (i as f32) * 0.1;
        let val = if r <= 1.0 {
            // Interior bowl shape controlled by floor_flatness
            // Simple parabolic bowl (flatness ~0.2): z = -1 + r^2 * (1 + rim)
            // Flat-floored / complex (flatness ~0.6): steeper wall near r=0.6..1.0
            let exp_p = 2.0 + floor_flatness * 2.5;
            let mut base = -1.0 + pow_approx(r, exp_p) * (1.0 + rim_ratio);
            // Central uplift peak inside r < 0.35
            if central_peak > 0.01 && r < 0.35 {
                let t = 1.0 - (r / 0.35);
                base += central_peak * 0.75 * t * t;
            }
            // Peak-ring secondary ridge around r = 0.45..0.55 if central_peak > 0.4 && floor_flatness > 0.5
            if central_peak > 0.42 && floor_flatness > 0.50 && r >= 0.30 && r <= 0.65 {
                let d_ring = (r - 0.48) / 0.16;
                let ring_bump = 1.0 - d_ring * d_ring;
                if ring_bump > 0.0 {
                    base += central_peak * 0.35 * ring_bump;
                }
            }
            base
        } else {
            // Outer ejecta rim tapering from +rim_ratio at r=1.0 down to 0.0 at r=1.5
            let t = (r - 1.0) / 0.5;
            if t < 1.0 {
                rim_ratio * (1.0 - t) * (1.0 - t)
            } else {
                0.0
            }
        };
        prof[i] = val;
        i += 1;
    }
    prof
}

const fn pow_approx(x: f32, p: f32) -> f32 {
    // Smooth interpolation between x^2 and x^4.5 without libm in const fn
    let x2 = x * x;
    let x3 = x2 * x;
    let x4 = x2 * x2;
    let x5 = x4 * x;
    if p <= 2.0 {
        x2
    } else if p <= 3.0 {
        let t = p - 2.0;
        x2 * (1.0 - t) + x3 * t
    } else if p <= 4.0 {
        let t = p - 3.0;
        x3 * (1.0 - t) + x4 * t
    } else {
        let t = p - 4.0;
        x4 * (1.0 - t) + x5 * t
    }
}

pub fn default_earth_craters() -> Vec<EarthCrater> {
    vec![
        EarthCrater {
            id: "EC-01",
            name: "Barringer (Meteor Crater)",
            location: "Arizona",
            country: "USA",
            lat_deg: 35.0272,
            lon_deg: -111.0225,
            diameter_km: 1.186,
            depth_m: 170.0,
            morphology: "Simple Bowl",
            depth_to_diameter: 0.143,
            rim_to_depth: 0.25,
            circularity: 0.94,
            floor_flatness: 0.28,
            central_peak: 0.0,
            wall_slope_deg: 30.0,
            radial_profile: build_profile(0.25, 0.28, 0.0),
            ice_analog_notes: "High depth-to-diameter (0.143) creates deep steep-walled shadow traps capable of shielding volatile ice at polar latitudes.",
            geology_notes: "Classic hyper-preserved simple impact crater in Coconino sandstone & Kaibab limestone with overturned ejecta rim.",
        },
        EarthCrater {
            id: "EC-02",
            name: "Pingualuit Crater",
            location: "Nunavik, Quebec",
            country: "Canada",
            lat_deg: 61.2767,
            lon_deg: -73.6622,
            diameter_km: 3.44,
            depth_m: 400.0,
            morphology: "Simple Bowl (Polar Cold-Trap)",
            depth_to_diameter: 0.116,
            rim_to_depth: 0.22,
            circularity: 0.97,
            floor_flatness: 0.35,
            central_peak: 0.0,
            wall_slope_deg: 27.0,
            radial_profile: build_profile(0.22, 0.35, 0.0),
            ice_analog_notes: "Premier Earth polar cold-trap analog: near-circular rim (0.97) and deep bowl retain seasonal ice cover and stratified glacial meltwater.",
            geology_notes: "Excavated in Precambrian crystalline granitic gneiss; pristine unbreached rim preserving deep bowl geometry.",
        },
        EarthCrater {
            id: "EC-03",
            name: "Lonar Crater",
            location: "Maharashtra",
            country: "India",
            lat_deg: 19.9758,
            lon_deg: 76.5069,
            diameter_km: 1.83,
            depth_m: 150.0,
            morphology: "Simple Flat-Floor",
            depth_to_diameter: 0.082,
            rim_to_depth: 0.20,
            circularity: 0.95,
            floor_flatness: 0.48,
            central_peak: 0.0,
            wall_slope_deg: 24.0,
            radial_profile: build_profile(0.20, 0.48, 0.0),
            ice_analog_notes: "Basaltic bowl traps subsurface volatiles & closed-basin water; moderate d/D (0.082) sustains floor shadowing at >70° latitude.",
            geology_notes: "Only well-preserved impact structure in flood basalt (Deccan Traps), making it a direct mineralogical analog for lunar maria & Mars.",
        },
        EarthCrater {
            id: "EC-04",
            name: "Haughton Crater",
            location: "Devon Island, Nunavut",
            country: "Canada",
            lat_deg: 75.3750,
            lon_deg: -89.6667,
            diameter_km: 23.0,
            depth_m: 350.0,
            morphology: "Complex Central-Peak (Arctic)",
            depth_to_diameter: 0.015,
            rim_to_depth: 0.16,
            circularity: 0.89,
            floor_flatness: 0.66,
            central_peak: 0.38,
            wall_slope_deg: 14.0,
            radial_profile: build_profile(0.16, 0.66, 0.38),
            ice_analog_notes: "High-Arctic permafrost crater: ground ice & paleolake sediments preserved around terraced inner walls and central uplift.",
            geology_notes: "Extensively studied Mars/Moon polar analog with impact melt breccias and hydrothermal permafrost alteration.",
        },
        EarthCrater {
            id: "EC-05",
            name: "Tswaing (Pretoria Saltpan)",
            location: "Gauteng",
            country: "South Africa",
            lat_deg: -25.4083,
            lon_deg: 28.0833,
            diameter_km: 1.13,
            depth_m: 119.0,
            morphology: "Simple Bowl",
            depth_to_diameter: 0.105,
            rim_to_depth: 0.24,
            circularity: 0.96,
            floor_flatness: 0.42,
            central_peak: 0.0,
            wall_slope_deg: 25.5,
            radial_profile: build_profile(0.24, 0.42, 0.0),
            ice_analog_notes: "Steep rim crest and closed depression (d/D 0.105) act as an efficient volatile & briny-ice accumulation basin.",
            geology_notes: "Young (~220 ka) granite-hosted crater with continuous raised rim and laminated evaporite floor sediments.",
        },
        EarthCrater {
            id: "EC-06",
            name: "Tenoumer Crater",
            location: "Adrar Region",
            country: "Mauritania",
            lat_deg: 22.9167,
            lon_deg: -10.4000,
            diameter_km: 1.90,
            depth_m: 110.0,
            morphology: "Simple Flat-Floor",
            depth_to_diameter: 0.058,
            rim_to_depth: 0.28,
            circularity: 0.95,
            floor_flatness: 0.52,
            central_peak: 0.0,
            wall_slope_deg: 21.0,
            radial_profile: build_profile(0.28, 0.52, 0.0),
            ice_analog_notes: "Prominent raised rim (0.28) provides high solar rim vantage while shielding floor deposits from low-angle insolation.",
            geology_notes: "Circular impact structure in Precambrian gneiss with prominent upturned sedimentary/crystalline collar.",
        },
        EarthCrater {
            id: "EC-07",
            name: "Wolfe Creek Crater",
            location: "Western Australia",
            country: "Australia",
            lat_deg: -19.1717,
            lon_deg: 127.7958,
            diameter_km: 0.875,
            depth_m: 60.0,
            morphology: "Simple Flat-Floor",
            depth_to_diameter: 0.069,
            rim_to_depth: 0.26,
            circularity: 0.92,
            floor_flatness: 0.58,
            central_peak: 0.0,
            wall_slope_deg: 22.0,
            radial_profile: build_profile(0.26, 0.58, 0.0),
            ice_analog_notes: "Flat infilled floor indicates post-impact trapping; subsurface moisture/ice pockets persist beneath gypsum/regolith fill.",
            geology_notes: "Well-preserved quartzite crater with steep inner talus slopes and flat central evaporite pan.",
        },
        EarthCrater {
            id: "EC-08",
            name: "Roter Kamm Crater",
            location: "Namib Desert",
            country: "Namibia",
            lat_deg: -27.7667,
            lon_deg: 16.3000,
            diameter_km: 2.50,
            depth_m: 130.0,
            morphology: "Simple Raised-Rim",
            depth_to_diameter: 0.052,
            rim_to_depth: 0.30,
            circularity: 0.93,
            floor_flatness: 0.50,
            central_peak: 0.0,
            wall_slope_deg: 19.5,
            radial_profile: build_profile(0.30, 0.50, 0.0),
            ice_analog_notes: "High rim-to-depth ratio (0.30) maximizes polar horizon solar capture next to sheltered interior cold traps.",
            geology_notes: "Exposed crystalline gneiss rim rising ~40-90 m above surrounding plains with aeolian sand infill.",
        },
        EarthCrater {
            id: "EC-09",
            name: "Bosumtwi Crater",
            location: "Ashanti Region",
            country: "Ghana",
            lat_deg: 6.5042,
            lon_deg: -1.4092,
            diameter_km: 10.5,
            depth_m: 380.0,
            morphology: "Transitional Complex",
            depth_to_diameter: 0.036,
            rim_to_depth: 0.21,
            circularity: 0.94,
            floor_flatness: 0.54,
            central_peak: 0.22,
            wall_slope_deg: 18.0,
            radial_profile: build_profile(0.21, 0.54, 0.22),
            ice_analog_notes: "Deep transitional crater basin supporting a permanent closed lake; strong analog for ancient crater lakes & ice sheets.",
            geology_notes: "Young (~1.07 Ma) tektite-producing impact structure with subtle buried central uplift beneath lake sediments.",
        },
        EarthCrater {
            id: "EC-10",
            name: "Nördlinger Ries Crater",
            location: "Bavaria",
            country: "Germany",
            lat_deg: 48.8833,
            lon_deg: 10.5667,
            diameter_km: 24.0,
            depth_m: 200.0,
            morphology: "Complex Peak-Ring",
            depth_to_diameter: 0.008,
            rim_to_depth: 0.18,
            circularity: 0.90,
            floor_flatness: 0.72,
            central_peak: 0.45,
            wall_slope_deg: 11.0,
            radial_profile: build_profile(0.18, 0.72, 0.45),
            ice_analog_notes: "Broad flat floor & inner crystalline ring trap stratified clay/ice-bearing deposits in annular moats.",
            geology_notes: "Benchmark complex crater with double-ring crystalline uplift, suevite breccia, and flat lacustrine floor.",
        },
        EarthCrater {
            id: "EC-11",
            name: "Mistastin Crater",
            location: "Labrador",
            country: "Canada",
            lat_deg: 55.8833,
            lon_deg: -63.3000,
            diameter_km: 28.0,
            depth_m: 320.0,
            morphology: "Complex Central-Peak",
            depth_to_diameter: 0.011,
            rim_to_depth: 0.15,
            circularity: 0.87,
            floor_flatness: 0.65,
            central_peak: 0.52,
            wall_slope_deg: 13.0,
            radial_profile: build_profile(0.15, 0.65, 0.52),
            ice_analog_notes: "Sub-arctic crater with central island uplift & annular moat holding seasonal/perennial ice cover; direct lunar anorthosite analog.",
            geology_notes: "Excavated in anorthosite & mangerite bedrock matching lunar highland crust composition.",
        },
        EarthCrater {
            id: "EC-12",
            name: "Manicouagan Crater",
            location: "Quebec",
            country: "Canada",
            lat_deg: 51.3833,
            lon_deg: -68.7000,
            diameter_km: 85.0,
            depth_m: 450.0,
            morphology: "Complex Peak-Ring",
            depth_to_diameter: 0.005,
            rim_to_depth: 0.14,
            circularity: 0.88,
            floor_flatness: 0.70,
            central_peak: 0.62,
            wall_slope_deg: 10.5,
            radial_profile: build_profile(0.14, 0.70, 0.62),
            ice_analog_notes: "Annular peripheral trough traps massive frozen reservoir ring around central massif; multi-ring volatile sink.",
            geology_notes: "One of Earth's largest preserved complex peak-ring craters featuring anorthosite central uplift and annular melt sheet.",
        },
        EarthCrater {
            id: "EC-13",
            name: "Clearwater West Crater",
            location: "Nunavik, Quebec",
            country: "Canada",
            lat_deg: 56.2167,
            lon_deg: -74.5000,
            diameter_km: 36.0,
            depth_m: 340.0,
            morphology: "Complex Peak-Ring",
            depth_to_diameter: 0.009,
            rim_to_depth: 0.16,
            circularity: 0.91,
            floor_flatness: 0.68,
            central_peak: 0.48,
            wall_slope_deg: 12.5,
            radial_profile: build_profile(0.16, 0.68, 0.48),
            ice_analog_notes: "Sub-arctic complex ring basin with deep frozen lake moat; high volatile-retention potential in sheltered inner ring.",
            geology_notes: "Double-impact complex structure with prominent ring of volcanic-looking impact melt islands.",
        },
        EarthCrater {
            id: "EC-14",
            name: "Gosses Bluff (Tnorala)",
            location: "Northern Territory",
            country: "Australia",
            lat_deg: -23.8194,
            lon_deg: 132.3078,
            diameter_km: 22.0,
            depth_m: 280.0,
            morphology: "Complex Central-Peak",
            depth_to_diameter: 0.013,
            rim_to_depth: 0.14,
            circularity: 0.86,
            floor_flatness: 0.62,
            central_peak: 0.72,
            wall_slope_deg: 15.5,
            radial_profile: build_profile(0.14, 0.62, 0.72),
            ice_analog_notes: "Prominent central peak casts long low-angle shadows across surrounding annular floor, forming secondary cold traps.",
            geology_notes: "Deeply eroded complex crater exposing steep upturned central uplift collar of resistant sandstone.",
        },
        EarthCrater {
            id: "EC-15",
            name: "Upheaval Dome",
            location: "Utah",
            country: "USA",
            lat_deg: 38.4369,
            lon_deg: -109.9292,
            diameter_km: 10.0,
            depth_m: 310.0,
            morphology: "Complex Central-Peak",
            depth_to_diameter: 0.031,
            rim_to_depth: 0.17,
            circularity: 0.88,
            floor_flatness: 0.56,
            central_peak: 0.68,
            wall_slope_deg: 17.0,
            radial_profile: build_profile(0.17, 0.56, 0.68),
            ice_analog_notes: "Steep inner walls and central dome create shadowed radial gullies and fractured-rock volatile reservoirs.",
            geology_notes: "Deeply dissected complex impact structure in Colorado Plateau sandstone displaying upturned central strata.",
        },
        EarthCrater {
            id: "EC-16",
            name: "Monturaqui Crater",
            location: "Atacama Desert",
            country: "Chile",
            lat_deg: -23.9278,
            lon_deg: -68.2617,
            diameter_km: 0.46,
            depth_m: 34.0,
            morphology: "Simple Bowl (Hyper-Arid)",
            depth_to_diameter: 0.074,
            rim_to_depth: 0.22,
            circularity: 0.91,
            floor_flatness: 0.36,
            central_peak: 0.0,
            wall_slope_deg: 21.5,
            radial_profile: build_profile(0.22, 0.36, 0.0),
            ice_analog_notes: "Hyper-arid Mars analog crater; moderate d/D (0.074) shields shadowed south/pole-facing wall regolith.",
            geology_notes: "Small simple crater excavated in ignimbrite/granite with well-defined parabolic bowl and ejecta blanket.",
        },
    ]
}

/// Synthesize a realistic 2D DEM TerrainGrid for an Earth crater (size x size)
/// for visual side-by-side comparison and radial verification.
pub fn synthesize_earth_crater_grid(crater: &EarthCrater, size: usize) -> TerrainGrid {
    let w = size.max(64);
    let h = size.max(64);
    let mut elev = vec![0.0f32; w * h];
    let depth = crater.depth_m.max(10.0) as f32;

    // Crater occupies radius = 0.34 of grid width so full rim (1.0R) and ejecta (1.5R) fit nicely
    let r_norm_crater = 0.34f32;

    for y in 0..h {
        let v = (y as f32) / ((h - 1).max(1) as f32);
        let dy = v - 0.5;
        for x in 0..w {
            let u = (x as f32) / ((w - 1).max(1) as f32);
            let dx = u - 0.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let r_ratio = dist / r_norm_crater;
            let theta = (dy as f64).atan2(dx as f64);

            // Slight natural azimuthal irregularity matching crater circularity
            let irreg = (1.0 - crater.circularity as f32)
                * 0.55
                * ((theta * 3.0).sin() as f32 * 0.6 + (theta * 5.0 + 1.1).cos() as f32 * 0.4);
            let r_eff = (r_ratio * (1.0 + irreg)).max(0.0);

            let prof_val = sample_profile(&crater.radial_profile, r_eff);

            // Subtle natural geological terracing & ejecta ray texture
            let ray_tex = if r_eff > 0.85 {
                let fade = (-(r_eff - 1.0).abs() * 2.2).exp();
                0.045 * fade * ((theta * 14.0).sin() as f32)
            } else if r_eff > 0.55 && crater.central_peak > 0.15 {
                // Wall slump terraces on complex craters
                0.04 * ((r_eff * 28.0) as f64 * PI / 2.0).sin() as f32
            } else {
                0.0
            };
            let micro = 0.015 * ((u * 43.0).sin() * (v * 39.0).cos());

            elev[y * w + x] = (prof_val + ray_tex + micro) * depth;
        }
    }

    let span_m = crater.diameter_km * 1000.0 / (2.0 * r_norm_crater as f64);
    let m_per_px = span_m / (w as f64);
    let mut bounds = GeoBounds::default_for(PlanetaryBody::Mars, w, h);
    bounds.min_lon = crater.lon_deg - 0.15;
    bounds.max_lon = crater.lon_deg + 0.15;
    bounds.min_lat = crater.lat_deg - 0.15;
    bounds.max_lat = crater.lat_deg + 0.15;
    bounds.meters_per_pixel_x = m_per_px;
    bounds.meters_per_pixel_y = m_per_px;

    TerrainGrid {
        width: w,
        height: h,
        elevation: elev,
        bounds,
        source_format: format!("Earth Crater Reference ({})", crater.name),
        has_geotiff_tags: false,
        nodata_value: None,
    }
}

fn sample_profile(prof: &[f32; 16], r_eff: f32) -> f32 {
    if r_eff >= 1.5 {
        return 0.0;
    }
    let fidx = (r_eff / 0.1).clamp(0.0, 14.999);
    let i0 = fidx.floor() as usize;
    let i1 = (i0 + 1).min(15);
    let t = fidx - (i0 as f32);
    prof[i0] * (1.0 - t) + prof[i1] * t
}

pub fn export_earth_craters_json(path: &Path) -> Result<(), String> {
    let craters = default_earth_craters();
    let mut s = String::from("[\n");
    for (i, c) in craters.iter().enumerate() {
        s.push_str("  {\n");
        s.push_str(&format!("    \"id\": \"{}\",\n", escape_json(c.id)));
        s.push_str(&format!("    \"name\": \"{}\",\n", escape_json(c.name)));
        s.push_str(&format!("    \"location\": \"{}\",\n", escape_json(c.location)));
        s.push_str(&format!("    \"country\": \"{}\",\n", escape_json(c.country)));
        s.push_str(&format!("    \"lat_deg\": {:.4},\n", c.lat_deg));
        s.push_str(&format!("    \"lon_deg\": {:.4},\n", c.lon_deg));
        s.push_str(&format!("    \"diameter_km\": {:.3},\n", c.diameter_km));
        s.push_str(&format!("    \"depth_m\": {:.1},\n", c.depth_m));
        s.push_str(&format!("    \"morphology\": \"{}\",\n", escape_json(c.morphology)));
        s.push_str(&format!("    \"depth_to_diameter\": {:.4},\n", c.depth_to_diameter));
        s.push_str(&format!("    \"rim_to_depth\": {:.3},\n", c.rim_to_depth));
        s.push_str(&format!("    \"circularity\": {:.3},\n", c.circularity));
        s.push_str(&format!("    \"floor_flatness\": {:.3},\n", c.floor_flatness));
        s.push_str(&format!("    \"central_peak\": {:.3},\n", c.central_peak));
        s.push_str(&format!("    \"wall_slope_deg\": {:.1},\n", c.wall_slope_deg));
        s.push_str(&format!("    \"ice_analog_notes\": \"{}\",\n", escape_json(c.ice_analog_notes)));
        s.push_str(&format!("    \"geology_notes\": \"{}\"\n", escape_json(c.geology_notes)));
        if i + 1 < craters.len() {
            s.push_str("  },\n");
        } else {
            s.push_str("  }\n");
        }
    }
    s.push_str("]\n");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, s).map_err(|e| format!("Failed to write earth craters JSON {:?}: {}", path, e))
}
