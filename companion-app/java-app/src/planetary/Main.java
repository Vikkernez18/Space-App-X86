package planetary;

import java.awt.Graphics2D;
import java.awt.GraphicsEnvironment;
import java.awt.image.BufferedImage;
import java.io.File;
import java.util.Locale;
import javax.imageio.ImageIO;
import javax.swing.SwingUtilities;
import javax.swing.UIManager;

import planetary.bridge.RustEngineBridge;
import planetary.model.ViabilityModels.AnalysisReport;
import planetary.model.ViabilityModels.DetectedCraterInfo;
import planetary.model.ViabilityModels.EarthCraterMatchInfo;
import planetary.model.ViabilityModels.SiteEvaluation;
import planetary.model.ViabilityModels.SuitabilityCriteria;
import planetary.ui.CraterComparisonPanel;
import planetary.ui.MapCanvasPanel;
import planetary.ui.Mesh3DPreviewPanel;
import planetary.ui.PlanetaryWorkbenchFrame;

public class Main {

    public static void main(String[] args) throws Exception {
        File projectRoot = resolveProjectRoot();
        RustEngineBridge bridge = new RustEngineBridge(projectRoot);
        bridge.ensurePlaceholdersSeeded();

        boolean verifyMode = false;
        String cliBody = null;
        String cliUpload = null;

        for (int i = 0; i < args.length; i++) {
            if ("--verify".equalsIgnoreCase(args[i])) {
                verifyMode = true;
            } else if ("--body".equalsIgnoreCase(args[i]) && i + 1 < args.length) {
                cliBody = args[++i];
            } else if ("--upload".equalsIgnoreCase(args[i]) && i + 1 < args.length) {
                cliUpload = args[++i];
            }
        }

        if (verifyMode) {
            runAutomatedVerification(bridge);
            return;
        }

        if (GraphicsEnvironment.isHeadless()) {
            System.out.println("Headless display detected; running verification & report summary...");
            runAutomatedVerification(bridge);
            return;
        }

        try {
            UIManager.setLookAndFeel(UIManager.getCrossPlatformLookAndFeelClassName());
        } catch (Exception ignored) {
        }

        final String initBody = cliBody;
        final String initUpload = cliUpload;
        SwingUtilities.invokeLater(() -> {
            PlanetaryWorkbenchFrame frame = new PlanetaryWorkbenchFrame(bridge);
            if (initBody != null || initUpload != null) {
                File upFile = initUpload != null ? new File(initUpload) : null;
                frame.loadCustomFile(upFile, initBody != null ? initBody : "mars");
            }
            frame.setLocationRelativeTo(null);
            frame.setVisible(true);
        });
    }

    private static void runAutomatedVerification(RustEngineBridge bridge) throws Exception {
        System.out.println("=========================================================================");
        System.out.println("  JMARS PLANETARY SETTLEMENT VIABILITY WORKBENCH — JAVA + RUST VERIFIER");
        System.out.println("=========================================================================");

        SuitabilityCriteria criteria = new SuitabilityCriteria();
        File upDir = bridge.getSampleUploadsDir();

        // 1. Mars High-Res GeoTIFF (.tif)
        File marsHiResTif = new File(upDir, "mars_hires_hirise_scan.tif");
        AnalysisReport rMarsTif = bridge.analyze("mars", marsHiResTif, criteria);
        printReportSummary("1. MARS HIGH-RES GEOTIFF (.TIF) vs BASELINE", rMarsTif);

        // 2. Mars Binary 3D Mesh (.stl)
        File marsStl = new File(upDir, "mars_hires_custom_mesh.stl");
        AnalysisReport rMarsStl = bridge.analyze("mars", marsStl, criteria);
        printReportSummary("2. MARS BINARY 3D MESH (.STL) vs BASELINE", rMarsStl);

        // 3. Moon High-Res LOLA GeoTIFF (.tiff)
        File moonHiResTiff = new File(upDir, "moon_hires_lola_dem.tiff");
        AnalysisReport rMoonTiff = bridge.analyze("moon", moonHiResTiff, criteria);
        printReportSummary("3. MOON HIGH-RES LOLA GEOTIFF (.TIFF) vs BASELINE", rMoonTiff);

        // 4. Moon Crater Rim ASCII 3D Mesh (.stl)
        File moonAsciiStl = new File(upDir, "moon_hires_crater_rim_ascii.stl");
        AnalysisReport rMoonStl = bridge.analyze("moon", moonAsciiStl, criteria);
        printReportSummary("4. MOON ASCII 3D MESH (.STL) vs BASELINE", rMoonStl);

        // 5. PNG & JPG Image Upload Verification
        File testPng = createSyntheticCraterImage(bridge.getReportsDir(), "test_crater_upload.png", "png", 320, 200);
        AnalysisReport rPng = bridge.analyze("moon", testPng, criteria);
        printReportSummary("5. SYNTHETIC LUNAR CRATER IMAGE (.PNG, 320x200)", rPng);
        if (rPng.uploadedWidth != 320 || rPng.uploadedHeight != 200 || !rPng.isImportedMap) {
            throw new IllegalStateException("PNG upload dimension/import check failed!");
        }

        File testJpg = createSyntheticCraterImage(bridge.getReportsDir(), "test_crater_upload.jpg", "jpg", 240, 180);
        AnalysisReport rJpg = bridge.analyze("mars", testJpg, criteria);
        printReportSummary("6. SYNTHETIC MARS CRATER IMAGE (.JPG, 240x180)", rJpg);
        if (rJpg.uploadedWidth != 240 || rJpg.uploadedHeight != 180 || !rJpg.isImportedMap) {
            throw new IllegalStateException("JPG upload dimension/import check failed!");
        }

        // Render visual preview PNGs of the Java 2D Map, 3D Mesh, and Earth Crater Comparison panels
        renderPreviewImages(bridge.getReportsDir(), rMarsTif, rMoonTiff, rPng);
        System.out.println("ALL JAVA <-> RUST VERIFICATION CHECKS PASSED.");
    }

    private static File createSyntheticCraterImage(File dir, String filename, String fmt, int w, int h)
            throws Exception {
        BufferedImage img = new BufferedImage(w, h, BufferedImage.TYPE_INT_RGB);
        double cx = w * 0.45;
        double cy = h * 0.52;
        double rad = Math.min(w, h) * 0.30;
        for (int y = 0; y < h; y++) {
            for (int x = 0; x < w; x++) {
                double d = Math.hypot(x - cx, y - cy) / rad;
                double val = 0.58;
                if (d < 1.0) {
                    val = 0.15 + 0.58 * (d * d);
                } else if (d < 1.45) {
                    double t = (d - 1.0) / 0.45;
                    val = 0.58 + 0.15 * (1.0 - t) * (1.0 - t);
                }
                int c = Math.max(0, Math.min(255, (int) Math.round(val * 255.0)));
                img.setRGB(x, y, (c << 16) | (c << 8) | c);
            }
        }
        File f = new File(dir, filename);
        ImageIO.write(img, fmt, f);
        return f;
    }

    private static void printReportSummary(String title, AnalysisReport r) {
        System.out.println("\n--- " + title + " ---");
        System.out.printf(Locale.US,
                "Body: %s | Uploaded: %s (%s, %dx%d @ %.1f m/px) | Imported: %s%n",
                r.bodyDisplay, r.uploadedFile, r.uploadedFormat,
                r.uploadedWidth, r.uploadedHeight, r.uploadedResM, r.isImportedMap);
        System.out.printf(Locale.US,
                "Math Comparison: Pearson r=%.4f | SSIM=%.4f | RMSE=%.2f m | MAE=%.2f m | ResGain=%.2fx | Status=%s%n",
                r.pearsonR, r.ssim, r.rmseM, r.maeM, r.resolutionRatio, r.alignmentStatus);
        if (!r.detectedCraters.isEmpty()) {
            DetectedCraterInfo dc = r.detectedCraters.get(0);
            System.out.printf(Locale.US,
                    "  * Primary Crater [%s]: d/D=%.4f | Wall=%.1f° | Circ=%.2f | Ice Potential=%s (%.1f/100)%n",
                    dc.id, dc.depthToDiameter, dc.wallSlopeDeg, dc.circularity,
                    dc.icePotentialLabel, dc.icePotentialScore);
        }
        if (!r.earthMatches.isEmpty()) {
            EarthCraterMatchInfo em = r.earthMatches.get(0);
            System.out.printf(Locale.US,
                    "  * Best Earth Analog: %s (%s, %s) | Match=%.1f%% | r=%.3f | %s%n",
                    em.name, em.location, em.country, em.similarityScore, em.profileCorrelation, em.morphology);
        }
        for (SiteEvaluation s : r.sites) {
            System.out.printf(Locale.US,
                    "  * [%s] %-30s | Base: %5.1f -> Upload: %5.1f (%+5.1f) | Slope: %5.2f° (max %5.1f°) | SafeArea: %5.1f%% | %s%n",
                    s.id, s.name, s.baselineScore, s.uploadScore, s.scoreDelta,
                    s.uploadSlopeDeg, s.maxSlopeDeg, s.safeAreaFraction * 100.0, s.status);
        }
    }

    private static void renderPreviewImages(
            File reportsDir, AnalysisReport marsRep, AnalysisReport moonRep, AnalysisReport craterRep)
            throws Exception {
        MapCanvasPanel mapPanel = new MapCanvasPanel();
        mapPanel.setSize(720, 660);

        Mesh3DPreviewPanel meshPanel = new Mesh3DPreviewPanel();
        meshPanel.setSize(680, 660);

        // Render combined 1400x660 Mars Dashboard Preview (2D Viability Map + 3D Surface Mesh)
        mapPanel.setReport(marsRep);
        meshPanel.setReport(marsRep);
        BufferedImage marsImg = new BufferedImage(1400, 660, BufferedImage.TYPE_INT_RGB);
        Graphics2D gMars = marsImg.createGraphics();
        mapPanel.paint(gMars);
        gMars.translate(720, 0);
        meshPanel.paint(gMars);
        gMars.dispose();
        File fMars = new File(reportsDir, "mars_ui_preview.png");
        ImageIO.write(marsImg, "png", fMars);

        // Render combined 1400x660 Moon Dashboard Preview (with Heatmap OFF check too)
        mapPanel.setHeatmapEnabled(false);
        meshPanel.setHeatmapEnabled(false);
        mapPanel.setReport(moonRep);
        meshPanel.setReport(moonRep);
        BufferedImage moonImg = new BufferedImage(1400, 660, BufferedImage.TYPE_INT_RGB);
        Graphics2D gMoon = moonImg.createGraphics();
        mapPanel.paint(gMoon);
        gMoon.translate(720, 0);
        meshPanel.paint(gMoon);
        gMoon.dispose();
        File fMoon = new File(reportsDir, "moon_ui_preview.png");
        ImageIO.write(moonImg, "png", fMoon);

        // Render Earth Crater Identification & Comparison Tab Preview
        CraterComparisonPanel cmpPanel = new CraterComparisonPanel();
        cmpPanel.setSize(1240, 760);
        cmpPanel.setReport(craterRep);
        layoutTree(cmpPanel);
        BufferedImage cmpImg = new BufferedImage(1240, 760, BufferedImage.TYPE_INT_RGB);
        Graphics2D gCmp = cmpImg.createGraphics();
        cmpPanel.printAll(gCmp);
        gCmp.dispose();
        File fCmp = new File(reportsDir, "crater_comparison_preview.png");
        ImageIO.write(cmpImg, "png", fCmp);

        System.out.println("\nGenerated Java UI Preview Snapshots:");
        System.out.println("  -> " + fMars.getAbsolutePath());
        System.out.println("  -> " + fMoon.getAbsolutePath());
        System.out.println("  -> " + fCmp.getAbsolutePath());
    }

    private static void layoutTree(java.awt.Container c) {
        c.doLayout();
        for (java.awt.Component child : c.getComponents()) {
            if (child instanceof java.awt.Container) {
                layoutTree((java.awt.Container) child);
            }
        }
    }

    private static File resolveProjectRoot() {
        File cwd = new File(System.getProperty("user.dir"));
        if (new File(cwd, "rust-engine").exists()) {
            return cwd;
        }
        if (cwd.getParentFile() != null && new File(cwd.getParentFile(), "rust-engine").exists()) {
            return cwd.getParentFile();
        }
        return new File("/home/Raven-62/planetary-settler");
    }
}
