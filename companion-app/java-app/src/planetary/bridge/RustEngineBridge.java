package planetary.bridge;

import java.awt.Graphics2D;
import java.awt.image.BufferedImage;
import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import javax.imageio.ImageIO;

import planetary.model.ViabilityModels.AnalysisReport;
import planetary.model.ViabilityModels.EarthCraterGrids;
import planetary.model.ViabilityModels.SuitabilityCriteria;
import planetary.model.ViabilityModels.TelemetryGrid;

public class RustEngineBridge {

    private final File engineBinary;
    private final File dataDir;

    public RustEngineBridge(File projectRoot) {
        this.engineBinary = new File(projectRoot, "rust-engine/target/release/planetary-engine");
        this.dataDir = new File(projectRoot, "data");
    }

    public File getDataDir() {
        return dataDir;
    }

    public File getSampleUploadsDir() {
        return new File(dataDir, "sample_uploads");
    }

    public File getReportsDir() {
        return new File(dataDir, "reports");
    }

    public void ensurePlaceholdersSeeded() throws IOException, InterruptedException {
        File marsBase = new File(dataDir, "baseline/mars_jezero_arcadia_baseline.tif");
        File earthCraters = new File(dataDir, "earth_craters/earth_craters.json");
        if (marsBase.exists() && earthCraters.exists()) {
            return;
        }
        List<String> cmd = new ArrayList<>();
        cmd.add(engineBinary.getAbsolutePath());
        cmd.add("seed");
        cmd.add(dataDir.getAbsolutePath());
        ProcessBuilder pb = new ProcessBuilder(cmd);
        pb.redirectErrorStream(true);
        Process p = pb.start();
        int code = p.waitFor();
        if (code != 0) {
            throw new IOException("Rust planetary-engine seed exited with code " + code);
        }
    }

    public AnalysisReport analyze(String body, File uploadedFileOrNull, SuitabilityCriteria criteria)
            throws IOException, InterruptedException {
        File reportsDir = getReportsDir();
        reportsDir.mkdirs();
        File telemetryBin = new File(reportsDir, "grid_telemetry.bin");
        File craterGridBin = new File(reportsDir, "earth_crater_grid.bin");
        File jsonReport = new File(reportsDir, "latest_report.json");

        File effectiveUpload = uploadedFileOrNull;
        String displayNameOverride = null;
        String sourceFormatOverride = null;
        BufferedImage originalImage = null;

        if (uploadedFileOrNull != null && uploadedFileOrNull.exists()) {
            String lower = uploadedFileOrNull.getName().toLowerCase(Locale.US);
            if (lower.endsWith(".png") || lower.endsWith(".jpg") || lower.endsWith(".jpeg")) {
                BufferedImage decoded = ImageIO.read(uploadedFileOrNull);
                if (decoded == null) {
                    throw new IOException("Could not decode image file: " + uploadedFileOrNull.getName());
                }
                BufferedImage rgbImg = new BufferedImage(
                        decoded.getWidth(), decoded.getHeight(), BufferedImage.TYPE_INT_RGB);
                Graphics2D g2 = rgbImg.createGraphics();
                g2.drawImage(decoded, 0, 0, null);
                g2.dispose();
                originalImage = rgbImg;

                File convertedBmp = new File(reportsDir, ".converted_upload.bmp");
                if (!ImageIO.write(rgbImg, "bmp", convertedBmp)) {
                    throw new IOException("Failed to write intermediate BMP for image upload.");
                }
                effectiveUpload = convertedBmp;
                displayNameOverride = uploadedFileOrNull.getName();
                sourceFormatOverride = lower.endsWith(".png")
                        ? String.format(Locale.US, "PNG Image (%dx%d)", rgbImg.getWidth(), rgbImg.getHeight())
                        : String.format(Locale.US, "JPEG Image (%dx%d)", rgbImg.getWidth(), rgbImg.getHeight());
            }
        }

        List<String> cmd = new ArrayList<>();
        cmd.add(engineBinary.getAbsolutePath());
        cmd.add("analyze");
        cmd.add("--body");
        cmd.add(body);
        cmd.add("--data-dir");
        cmd.add(dataDir.getAbsolutePath());
        if (effectiveUpload != null && effectiveUpload.exists()) {
            cmd.add("--upload");
            cmd.add(effectiveUpload.getAbsolutePath());
        } else {
            cmd.add("--upload");
            cmd.add("baseline");
        }
        if (displayNameOverride != null) {
            cmd.add("--display-name");
            cmd.add(displayNameOverride);
        }
        if (sourceFormatOverride != null) {
            cmd.add("--source-format");
            cmd.add(sourceFormatOverride);
        }
        cmd.add("--telemetry-out");
        cmd.add(telemetryBin.getAbsolutePath());
        cmd.add("--crater-grid-out");
        cmd.add(craterGridBin.getAbsolutePath());
        cmd.add("--json-out");
        cmd.add(jsonReport.getAbsolutePath());
        cmd.add("--format");
        cmd.add("kv");
        cmd.add("--max-slope");
        cmd.add(String.format(Locale.US, "%.2f", criteria.maxSlopeDeg));
        cmd.add("--max-rough");
        cmd.add(String.format(Locale.US, "%.2f", criteria.maxRoughnessM));
        cmd.add("--min-score");
        cmd.add(String.format(Locale.US, "%.2f", criteria.minViabilityScore));
        cmd.add("--w-slope");
        cmd.add(String.format(Locale.US, "%.3f", criteria.weightSlope));
        cmd.add("--w-rough");
        cmd.add(String.format(Locale.US, "%.3f", criteria.weightRoughness));
        cmd.add("--w-elev");
        cmd.add(String.format(Locale.US, "%.3f", criteria.weightElevation));
        cmd.add("--w-solar");
        cmd.add(String.format(Locale.US, "%.3f", criteria.weightSolar));

        ProcessBuilder pb = new ProcessBuilder(cmd);
        Process proc = pb.start();

        Map<String, String> kv = new LinkedHashMap<>();
        StringBuilder errBuf = new StringBuilder();

        try (BufferedReader br = new BufferedReader(
                new InputStreamReader(proc.getInputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = br.readLine()) != null) {
                int eq = line.indexOf('=');
                if (eq > 0) {
                    String k = line.substring(0, eq).trim();
                    String v = line.substring(eq + 1).trim();
                    kv.put(k, v);
                }
            }
        }

        try (BufferedReader errReader = new BufferedReader(
                new InputStreamReader(proc.getErrorStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = errReader.readLine()) != null) {
                errBuf.append(line).append("\n");
            }
        }

        int exit = proc.waitFor();
        if (exit != 0) {
            throw new IOException("Rust engine failed (exit " + exit + "): " + errBuf);
        }

        TelemetryGrid grid = null;
        if (telemetryBin.exists()) {
            grid = TelemetryGrid.readFromFile(telemetryBin);
        }
        AnalysisReport report = AnalysisReport.fromKvMap(kv, grid);
        report.originalUploadedImage = originalImage;
        if (craterGridBin.exists()) {
            report.earthCraterGrids = EarthCraterGrids.readFromFile(craterGridBin);
        }
        return report;
    }
}
