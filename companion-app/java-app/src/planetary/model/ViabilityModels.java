package planetary.model;

import java.awt.image.BufferedImage;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

public class ViabilityModels {

    public static class SuitabilityCriteria {
        public double maxSlopeDeg = 8.0;
        public double maxRoughnessM = 12.0;
        public double minViabilityScore = 68.0;
        public double weightSlope = 0.40;
        public double weightRoughness = 0.30;
        public double weightElevation = 0.15;
        public double weightSolar = 0.15;

        public SuitabilityCriteria copy() {
            SuitabilityCriteria c = new SuitabilityCriteria();
            c.maxSlopeDeg = this.maxSlopeDeg;
            c.maxRoughnessM = this.maxRoughnessM;
            c.minViabilityScore = this.minViabilityScore;
            c.weightSlope = this.weightSlope;
            c.weightRoughness = this.weightRoughness;
            c.weightElevation = this.weightElevation;
            c.weightSolar = this.weightSolar;
            return c;
        }
    }

    public static class SiteEvaluation {
        public String id = "";
        public String name = "";
        public double normX;
        public double normY;
        public double normRadius;
        public double latDeg;
        public double lonDeg;
        public double radiusKm;
        public String rationale = "";
        public String resources = "";
        public double baselineScore;
        public double uploadScore;
        public double scoreDelta;
        public double baseSlopeDeg;
        public double uploadSlopeDeg;
        public double maxSlopeDeg;
        public double baseRoughnessM;
        public double uploadRoughnessM;
        public double meanElevationM;
        public double solarScore;
        public double localRmseM;
        public double safeAreaFraction;
        public String status = "";
        public String finding = "";
    }

    public static class DetectedCraterInfo {
        public String id = "";
        public double normX;
        public double normY;
        public double normRadius;
        public double diameterPx;
        public double depthVal;
        public double rimHeightVal;
        public double depthToDiameter;
        public double rimToDepth;
        public double circularity;
        public double floorFlatness;
        public double centralPeak;
        public double wallSlopeDeg;
        public double confidence;
        public double icePotentialScore;
        public String icePotentialLabel = "";
        public String iceRationale = "";
        public float[] radialProfile = new float[16];
    }

    public static class EarthCraterMatchInfo {
        public String id = "";
        public String name = "";
        public String location = "";
        public String country = "";
        public double latDeg;
        public double lonDeg;
        public double diameterKm;
        public double depthM;
        public String morphology = "";
        public double depthToDiameter;
        public double rimToDepth;
        public double circularity;
        public double floorFlatness;
        public double centralPeak;
        public double wallSlopeDeg;
        public double similarityScore;
        public double profileCorrelation;
        public double featureRmse;
        public String iceAnalogNotes = "";
        public String geologyNotes = "";
        public float[] radialProfile = new float[16];

        public String craterNameShort() {
            if (name == null) return "";
            int paren = name.indexOf(" (");
            return paren > 0 ? name.substring(0, paren) : name;
        }
    }

    public static class EarthCraterGrids {
        public final int count;
        public final int width;
        public final int height;
        public final float[][] elevations;

        public EarthCraterGrids(int count, int width, int height, float[][] elevations) {
            this.count = count;
            this.width = width;
            this.height = height;
            this.elevations = elevations;
        }

        public static EarthCraterGrids readFromFile(File f) throws IOException {
            byte[] all = new byte[(int) f.length()];
            try (FileInputStream fis = new FileInputStream(f)) {
                int off = 0;
                while (off < all.length) {
                    int r = fis.read(all, off, all.length - off);
                    if (r < 0) break;
                    off += r;
                }
            }
            if (all.length < 12) {
                return new EarthCraterGrids(0, 0, 0, new float[0][0]);
            }
            ByteBuffer bb = ByteBuffer.wrap(all).order(ByteOrder.LITTLE_ENDIAN);
            int count = bb.getInt();
            int w = bb.getInt();
            int h = bb.getInt();
            int n = w * h;
            float[][] elevs = new float[count][n];
            for (int c = 0; c < count; c++) {
                for (int i = 0; i < n; i++) {
                    elevs[c][i] = bb.getFloat();
                }
            }
            return new EarthCraterGrids(count, w, h, elevs);
        }
    }

    public static class AnalysisReport {
        public String body = "mars";
        public String bodyDisplay = "";
        public String baselineFile = "";
        public String uploadedFile = "";
        public String uploadedFormat = "";
        public boolean isImportedMap = false;
        public int baselineWidth;
        public int baselineHeight;
        public int uploadedWidth;
        public int uploadedHeight;
        public double baselineResM;
        public double uploadedResM;
        public boolean hasGeotiffTags;
        public double elevMinM;
        public double elevMaxM;
        public double elevMeanM;
        public double globalMeanSlopeDeg;
        public double globalMeanRoughnessM;
        public double globalViabilityScore;
        public double pearsonR;
        public double ssim;
        public double rmseM;
        public double maeM;
        public double maxAbsDiffM;
        public double resolutionRatio;
        public double highFreqDetailRatio;
        public String alignmentStatus = "";
        public List<SiteEvaluation> sites = new ArrayList<>();
        public List<DetectedCraterInfo> detectedCraters = new ArrayList<>();
        public List<EarthCraterMatchInfo> earthMatches = new ArrayList<>();
        public TelemetryGrid grid;
        public EarthCraterGrids earthCraterGrids;
        public BufferedImage originalUploadedImage;

        public static AnalysisReport fromKvMap(Map<String, String> kv, TelemetryGrid grid) {
            AnalysisReport r = new AnalysisReport();
            r.body = kv.getOrDefault("BODY", "mars");
            r.bodyDisplay = kv.getOrDefault("BODY_DISPLAY", "Mars");
            r.baselineFile = kv.getOrDefault("BASELINE_FILE", "");
            r.uploadedFile = kv.getOrDefault("UPLOADED_FILE", "");
            r.uploadedFormat = kv.getOrDefault("UPLOADED_FORMAT", "");
            r.isImportedMap = Boolean.parseBoolean(kv.getOrDefault("IS_IMPORTED_MAP", "false"));
            r.baselineWidth = parseInt(kv.get("BASELINE_WIDTH"), 128);
            r.baselineHeight = parseInt(kv.get("BASELINE_HEIGHT"), 128);
            r.uploadedWidth = parseInt(kv.get("UPLOADED_WIDTH"), 128);
            r.uploadedHeight = parseInt(kv.get("UPLOADED_HEIGHT"), 128);
            r.baselineResM = parseDouble(kv.get("BASELINE_RES_M"), 368.0);
            r.uploadedResM = parseDouble(kv.get("UPLOADED_RES_M"), 368.0);
            r.hasGeotiffTags = Boolean.parseBoolean(kv.getOrDefault("HAS_GEOTIFF_TAGS", "false"));
            r.elevMinM = parseDouble(kv.get("ELEV_MIN_M"), -2600.0);
            r.elevMaxM = parseDouble(kv.get("ELEV_MAX_M"), -2000.0);
            r.elevMeanM = parseDouble(kv.get("ELEV_MEAN_M"), -2400.0);
            r.globalMeanSlopeDeg = parseDouble(kv.get("GLOBAL_MEAN_SLOPE_DEG"), 1.5);
            r.globalMeanRoughnessM = parseDouble(kv.get("GLOBAL_MEAN_ROUGHNESS_M"), 4.0);
            r.globalViabilityScore = parseDouble(kv.get("GLOBAL_VIABILITY_SCORE"), 85.0);
            r.pearsonR = parseDouble(kv.get("PEARSON_R"), 1.0);
            r.ssim = parseDouble(kv.get("SSIM"), 1.0);
            r.rmseM = parseDouble(kv.get("RMSE_M"), 0.0);
            r.maeM = parseDouble(kv.get("MAE_M"), 0.0);
            r.maxAbsDiffM = parseDouble(kv.get("MAX_ABS_DIFF_M"), 0.0);
            r.resolutionRatio = parseDouble(kv.get("RESOLUTION_RATIO"), 1.0);
            r.highFreqDetailRatio = parseDouble(kv.get("HIGH_FREQ_DETAIL_RATIO"), 1.0);
            r.alignmentStatus = kv.getOrDefault("ALIGNMENT_STATUS", "VALID");

            int count = parseInt(kv.get("SITE_COUNT"), 0);
            for (int i = 0; i < count; i++) {
                String p = "SITE_" + i + "_";
                SiteEvaluation s = new SiteEvaluation();
                s.id = kv.getOrDefault(p + "ID", "S-" + i);
                s.name = kv.getOrDefault(p + "NAME", "Site " + i);
                s.normX = parseDouble(kv.get(p + "NORM_X"), 0.5);
                s.normY = parseDouble(kv.get(p + "NORM_Y"), 0.5);
                s.normRadius = parseDouble(kv.get(p + "NORM_RADIUS"), 0.09);
                s.latDeg = parseDouble(kv.get(p + "LAT"), 0.0);
                s.lonDeg = parseDouble(kv.get(p + "LON"), 0.0);
                s.radiusKm = parseDouble(kv.get(p + "RADIUS_KM"), 2.0);
                s.rationale = kv.getOrDefault(p + "RATIONALE", "");
                s.resources = kv.getOrDefault(p + "RESOURCES", "");
                s.baselineScore = parseDouble(kv.get(p + "BASELINE_SCORE"), 80.0);
                s.uploadScore = parseDouble(kv.get(p + "UPLOAD_SCORE"), 80.0);
                s.scoreDelta = parseDouble(kv.get(p + "SCORE_DELTA"), 0.0);
                s.baseSlopeDeg = parseDouble(kv.get(p + "BASE_SLOPE"), 1.0);
                s.uploadSlopeDeg = parseDouble(kv.get(p + "UPLOAD_SLOPE"), 1.0);
                s.maxSlopeDeg = parseDouble(kv.get(p + "MAX_SLOPE"), 2.0);
                s.baseRoughnessM = parseDouble(kv.get(p + "BASE_ROUGH"), 3.0);
                s.uploadRoughnessM = parseDouble(kv.get(p + "UPLOAD_ROUGH"), 3.0);
                s.meanElevationM = parseDouble(kv.get(p + "MEAN_ELEV"), 0.0);
                s.solarScore = parseDouble(kv.get(p + "SOLAR"), 90.0);
                s.localRmseM = parseDouble(kv.get(p + "LOCAL_RMSE"), 0.0);
                s.safeAreaFraction = parseDouble(kv.get(p + "SAFE_FRAC"), 1.0);
                s.status = kv.getOrDefault(p + "STATUS", "CONFIRMED_VIABLE");
                s.finding = kv.getOrDefault(p + "FINDING", "");
                r.sites.add(s);
            }

            int craterCount = parseInt(kv.get("CRATER_COUNT"), 0);
            for (int i = 0; i < craterCount; i++) {
                String p = "CRATER_" + i + "_";
                DetectedCraterInfo dc = new DetectedCraterInfo();
                dc.id = kv.getOrDefault(p + "ID", "C-" + (i + 1));
                dc.normX = parseDouble(kv.get(p + "NORM_X"), 0.5);
                dc.normY = parseDouble(kv.get(p + "NORM_Y"), 0.5);
                dc.normRadius = parseDouble(kv.get(p + "NORM_RADIUS"), 0.15);
                dc.diameterPx = parseDouble(kv.get(p + "DIAMETER_PX"), 40.0);
                dc.depthVal = parseDouble(kv.get(p + "DEPTH_VAL"), 10.0);
                dc.rimHeightVal = parseDouble(kv.get(p + "RIM_VAL"), 2.0);
                dc.depthToDiameter = parseDouble(kv.get(p + "D_TO_D"), 0.08);
                dc.rimToDepth = parseDouble(kv.get(p + "RIM_TO_D"), 0.22);
                dc.circularity = parseDouble(kv.get(p + "CIRC"), 0.85);
                dc.floorFlatness = parseDouble(kv.get(p + "FLAT"), 0.45);
                dc.centralPeak = parseDouble(kv.get(p + "PEAK"), 0.0);
                dc.wallSlopeDeg = parseDouble(kv.get(p + "SLOPE"), 18.0);
                dc.confidence = parseDouble(kv.get(p + "CONF"), 75.0);
                dc.icePotentialScore = parseDouble(kv.get(p + "ICE_SCORE"), 60.0);
                dc.icePotentialLabel = kv.getOrDefault(p + "ICE_LABEL", "MODERATE");
                dc.iceRationale = kv.getOrDefault(p + "ICE_NOTE", "");
                dc.radialProfile = parseProfileCsv(kv.get(p + "PROFILE"));
                r.detectedCraters.add(dc);
            }

            int matchCount = parseInt(kv.get("EARTH_MATCH_COUNT"), 0);
            for (int i = 0; i < matchCount; i++) {
                String p = "EMATCH_" + i + "_";
                EarthCraterMatchInfo em = new EarthCraterMatchInfo();
                em.id = kv.getOrDefault(p + "ID", "EC-" + i);
                em.name = kv.getOrDefault(p + "NAME", "");
                em.location = kv.getOrDefault(p + "LOC", "");
                em.country = kv.getOrDefault(p + "COUNTRY", "");
                em.latDeg = parseDouble(kv.get(p + "LAT"), 0.0);
                em.lonDeg = parseDouble(kv.get(p + "LON"), 0.0);
                em.diameterKm = parseDouble(kv.get(p + "DIAM_KM"), 1.0);
                em.depthM = parseDouble(kv.get(p + "DEPTH_M"), 100.0);
                em.morphology = kv.getOrDefault(p + "MORPH", "");
                em.depthToDiameter = parseDouble(kv.get(p + "D_TO_D"), 0.08);
                em.rimToDepth = parseDouble(kv.get(p + "RIM_TO_D"), 0.22);
                em.circularity = parseDouble(kv.get(p + "CIRC"), 0.90);
                em.floorFlatness = parseDouble(kv.get(p + "FLAT"), 0.45);
                em.centralPeak = parseDouble(kv.get(p + "PEAK"), 0.0);
                em.wallSlopeDeg = parseDouble(kv.get(p + "SLOPE"), 20.0);
                em.similarityScore = parseDouble(kv.get(p + "SIM"), 75.0);
                em.profileCorrelation = parseDouble(kv.get(p + "CORR"), 0.80);
                em.featureRmse = parseDouble(kv.get(p + "RMSE"), 0.25);
                em.iceAnalogNotes = kv.getOrDefault(p + "ICE_NOTE", "");
                em.geologyNotes = kv.getOrDefault(p + "GEO_NOTE", "");
                em.radialProfile = parseProfileCsv(kv.get(p + "PROFILE"));
                r.earthMatches.add(em);
            }

            r.grid = grid;
            return r;
        }

        private static float[] parseProfileCsv(String csv) {
            float[] prof = new float[16];
            if (csv == null || csv.trim().isEmpty()) return prof;
            String[] parts = csv.split(",");
            for (int i = 0; i < Math.min(16, parts.length); i++) {
                prof[i] = (float) parseDouble(parts[i], 0.0);
            }
            return prof;
        }

        private static int parseInt(String s, int def) {
            if (s == null) return def;
            try {
                return Integer.parseInt(s.trim());
            } catch (NumberFormatException e) {
                return def;
            }
        }

        private static double parseDouble(String s, double def) {
            if (s == null) return def;
            try {
                return Double.parseDouble(s.trim());
            } catch (NumberFormatException e) {
                return def;
            }
        }
    }

    public static class TelemetryGrid {
        public final int width;
        public final int height;
        public final float[] elevation;
        public final float[] slopeDeg;
        public final float[] roughnessM;
        public final float[] viabilityScore;
        public final float[] residualDiffM;

        public TelemetryGrid(int w, int h, float[] elev, float[] slope, float[] rough, float[] viab, float[] diff) {
            this.width = w;
            this.height = h;
            this.elevation = elev;
            this.slopeDeg = slope;
            this.roughnessM = rough;
            this.viabilityScore = viab;
            this.residualDiffM = diff;
        }

        public static TelemetryGrid readFromFile(File f) throws IOException {
            byte[] all = new byte[(int) f.length()];
            try (FileInputStream fis = new FileInputStream(f)) {
                int off = 0;
                while (off < all.length) {
                    int r = fis.read(all, off, all.length - off);
                    if (r < 0) break;
                    off += r;
                }
            }
            ByteBuffer bb = ByteBuffer.wrap(all).order(ByteOrder.LITTLE_ENDIAN);
            int w = bb.getInt();
            int h = bb.getInt();
            int n = w * h;
            float[] elev = new float[n];
            float[] slope = new float[n];
            float[] rough = new float[n];
            float[] viab = new float[n];
            float[] diff = new float[n];
            for (int i = 0; i < n; i++) elev[i] = bb.getFloat();
            for (int i = 0; i < n; i++) slope[i] = bb.getFloat();
            for (int i = 0; i < n; i++) rough[i] = bb.getFloat();
            for (int i = 0; i < n; i++) viab[i] = bb.getFloat();
            for (int i = 0; i < n; i++) diff[i] = bb.getFloat();
            return new TelemetryGrid(w, h, elev, slope, rough, viab, diff);
        }
    }
}
