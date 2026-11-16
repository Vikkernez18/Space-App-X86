package planetary.ui;

import java.awt.BasicStroke;
import java.awt.Color;
import java.awt.Dimension;
import java.awt.Font;
import java.awt.FontMetrics;
import java.awt.Graphics;
import java.awt.Graphics2D;
import java.awt.RenderingHints;
import java.awt.event.MouseAdapter;
import java.awt.event.MouseEvent;
import java.awt.event.MouseMotionAdapter;
import java.awt.geom.Ellipse2D;
import java.awt.geom.Path2D;
import java.awt.image.BufferedImage;
import java.util.Locale;
import java.util.function.Consumer;
import javax.swing.JPanel;

import planetary.model.ViabilityModels.AnalysisReport;
import planetary.model.ViabilityModels.DetectedCraterInfo;
import planetary.model.ViabilityModels.SiteEvaluation;
import planetary.model.ViabilityModels.TelemetryGrid;

public class MapCanvasPanel extends JPanel {
    private static final long serialVersionUID = 1L;

    public enum LayerMode {
        VIABILITY("Suitability Heatmap (0–100) + Hillshade"),
        ELEVATION("Elevation Hypsometric DEM + Hillshade"),
        SLOPE("Slope Gradient Angle (°)"),
        RESIDUAL_DIFF("Baseline vs. Upload Residual (Δz m)");

        public final String label;
        LayerMode(String label) {
            this.label = label;
        }
        @Override
        public String toString() {
            return label;
        }
    }

    private AnalysisReport report;
    private LayerMode layerMode = LayerMode.VIABILITY;
    private boolean heatmapEnabled = true;
    private BufferedImage cachedLayerImage;
    private int selectedSiteIndex = 0;
    private int selectedCraterIndex = 0;
    private int hoverGridX = -1;
    private int hoverGridY = -1;
    private Consumer<Integer> onSiteSelected;
    private Consumer<Integer> onCraterSelected;

    public MapCanvasPanel() {
        setBackground(new Color(14, 18, 26));
        setPreferredSize(new Dimension(640, 580));

        addMouseMotionListener(new MouseMotionAdapter() {
            @Override
            public void mouseMoved(MouseEvent e) {
                updateHoverCoords(e.getX(), e.getY());
            }
        });

        addMouseListener(new MouseAdapter() {
            @Override
            public void mousePressed(MouseEvent e) {
                handleMapClick(e.getX(), e.getY());
            }
        });
    }

    public void setOnSiteSelected(Consumer<Integer> listener) {
        this.onSiteSelected = listener;
    }

    public void setOnCraterSelected(Consumer<Integer> listener) {
        this.onCraterSelected = listener;
    }

    public void setHeatmapEnabled(boolean enabled) {
        if (this.heatmapEnabled != enabled) {
            this.heatmapEnabled = enabled;
            rebuildLayerImage();
            repaint();
        }
    }

    public boolean isHeatmapEnabled() {
        return heatmapEnabled;
    }

    public void setReport(AnalysisReport report) {
        this.report = report;
        if (this.selectedSiteIndex >= (report != null ? report.sites.size() : 0)) {
            this.selectedSiteIndex = 0;
        }
        if (this.selectedCraterIndex >= (report != null ? report.detectedCraters.size() : 0)) {
            this.selectedCraterIndex = 0;
        }
        if (report != null && report.grid != null) {
            hoverGridX = Math.max(0, Math.min(report.grid.width - 1, report.grid.width / 2));
            hoverGridY = Math.max(0, Math.min(report.grid.height - 1, report.grid.height / 2));
        }
        rebuildLayerImage();
        repaint();
    }

    public void setLayerMode(LayerMode mode) {
        this.layerMode = mode;
        rebuildLayerImage();
        repaint();
    }

    public LayerMode getLayerMode() {
        return layerMode;
    }

    public void setSelectedSiteIndex(int idx) {
        this.selectedSiteIndex = idx;
        if (report != null && !report.isImportedMap && idx >= 0 && idx < report.sites.size() && report.grid != null) {
            SiteEvaluation s = report.sites.get(idx);
            hoverGridX = (int) Math.round(s.normX * (report.grid.width - 1));
            hoverGridY = (int) Math.round(s.normY * (report.grid.height - 1));
        }
        repaint();
    }

    public void setSelectedCraterIndex(int idx) {
        this.selectedCraterIndex = idx;
        if (report != null && report.isImportedMap && idx >= 0 && idx < report.detectedCraters.size() && report.grid != null) {
            DetectedCraterInfo dc = report.detectedCraters.get(idx);
            hoverGridX = (int) Math.round(dc.normX * (report.grid.width - 1));
            hoverGridY = (int) Math.round(dc.normY * (report.grid.height - 1));
        }
        repaint();
    }

    private int[] computeMapViewport() {
        int w = getWidth();
        int h = getHeight();
        int profileHeight = 105;
        int topBar = 32;
        int availW = Math.max(100, w - 28);
        int availH = Math.max(100, h - profileHeight - topBar - 20);

        double gridAspect = 1.0;
        if (report != null && report.grid != null && report.grid.height > 0) {
            gridAspect = (double) report.grid.width / (double) report.grid.height;
        }

        int mw, mh;
        if ((availW / (double) availH) > gridAspect) {
            mh = availH;
            mw = Math.max(40, (int) Math.round(availH * gridAspect));
        } else {
            mw = availW;
            mh = Math.max(40, (int) Math.round(availW / gridAspect));
        }

        int mx = (w - mw) / 2;
        int my = topBar + (availH - mh) / 2;
        return new int[]{mx, my, mw, mh};
    }

    private void updateHoverCoords(int mouseX, int mouseY) {
        if (report == null || report.grid == null) return;
        int[] vp = computeMapViewport();
        if (mouseX >= vp[0] && mouseX < vp[0] + vp[2] && mouseY >= vp[1] && mouseY < vp[1] + vp[3]) {
            double u = (double) (mouseX - vp[0]) / vp[2];
            double v = (double) (mouseY - vp[1]) / vp[3];
            hoverGridX = Math.max(0, Math.min(report.grid.width - 1, (int) (u * report.grid.width)));
            hoverGridY = Math.max(0, Math.min(report.grid.height - 1, (int) (v * report.grid.height)));
            repaint();
        }
    }

    private void handleMapClick(int mouseX, int mouseY) {
        if (report == null) return;
        int[] vp = computeMapViewport();
        double u = (double) (mouseX - vp[0]) / vp[2];
        double v = (double) (mouseY - vp[1]) / vp[3];
        if (u < 0 || u > 1 || v < 0 || v > 1) return;

        if (report.isImportedMap) {
            if (report.detectedCraters.isEmpty()) return;
            int bestIdx = -1;
            double bestDist = Double.MAX_VALUE;
            double minSide = Math.min(vp[2], vp[3]);
            for (int i = 0; i < report.detectedCraters.size(); i++) {
                DetectedCraterInfo dc = report.detectedCraters.get(i);
                double dxPx = (u - dc.normX) * vp[2];
                double dyPx = (v - dc.normY) * vp[3];
                double dPx = Math.hypot(dxPx, dyPx);
                double radPx = dc.normRadius * minSide;
                if (dPx <= Math.max(radPx * 1.25, 22.0) && dPx < bestDist) {
                    bestDist = dPx;
                    bestIdx = i;
                }
            }
            if (bestIdx >= 0) {
                selectedCraterIndex = bestIdx;
                if (onCraterSelected != null) {
                    onCraterSelected.accept(bestIdx);
                }
                repaint();
            }
            return;
        }

        if (report.sites.isEmpty()) return;
        int bestIdx = -1;
        double bestDist = Double.MAX_VALUE;
        for (int i = 0; i < report.sites.size(); i++) {
            SiteEvaluation s = report.sites.get(i);
            double d = Math.hypot(u - s.normX, v - s.normY);
            if (d <= Math.max(s.normRadius * 1.35, 0.08) && d < bestDist) {
                bestDist = d;
                bestIdx = i;
            }
        }
        if (bestIdx >= 0) {
            selectedSiteIndex = bestIdx;
            if (onSiteSelected != null) {
                onSiteSelected.accept(bestIdx);
            }
            repaint();
        }
    }

    private void rebuildLayerImage() {
        if (report == null || report.grid == null) {
            cachedLayerImage = null;
            return;
        }
        if (!heatmapEnabled && report.originalUploadedImage != null) {
            cachedLayerImage = report.originalUploadedImage;
            return;
        }

        TelemetryGrid g = report.grid;
        int w = g.width;
        int h = g.height;
        BufferedImage img = new BufferedImage(w, h, BufferedImage.TYPE_INT_RGB);

        float minZ = (float) report.elevMinM;
        float maxZ = (float) report.elevMaxM;
        float spanZ = Math.max(1e-3f, maxZ - minZ);
        boolean isMars = "mars".equalsIgnoreCase(report.body);
        // Adaptive hillshade normalization so both meter-scale DEMs and Blender STLs shade crisply
        float shadeScale = Math.max(0.5f, spanZ * 0.045f);

        for (int y = 0; y < h; y++) {
            for (int x = 0; x < w; x++) {
                int i = y * w + x;
                int x0 = Math.max(0, x - 1);
                int x1 = Math.min(w - 1, x + 1);
                int y0 = Math.max(0, y - 1);
                int y1 = Math.min(h - 1, y + 1);
                float dzdx = g.elevation[y * w + x1] - g.elevation[y * w + x0];
                float dzdy = g.elevation[y1 * w + x] - g.elevation[y0 * w + x];
                float shade = (float) Math.tanh((-dzdx - dzdy) / shadeScale) * 0.28f + 1.0f;

                int rgb;
                if (!heatmapEnabled) {
                    float t = clamp01((g.elevation[i] - minZ) / spanZ);
                    int baseGray = lerp(38, 228, t);
                    rgb = packRgb(baseGray, baseGray, Math.min(255, baseGray + 6), shade);
                } else {
                    switch (layerMode) {
                        case ELEVATION: {
                            float t = clamp01((g.elevation[i] - minZ) / spanZ);
                            rgb = isMars ? marsElevationColor(t, shade) : moonElevationColor(t, shade);
                            break;
                        }
                        case SLOPE: {
                            float s = g.slopeDeg[i];
                            float t = clamp01(s / 16.0f);
                            rgb = slopeColor(t, shade);
                            break;
                        }
                        case RESIDUAL_DIFF: {
                            float d = g.residualDiffM[i];
                            rgb = diffColor(d);
                            break;
                        }
                        case VIABILITY:
                        default: {
                            float v = clamp01(g.viabilityScore[i] / 100.0f);
                            rgb = viabilityColor(v, shade);
                            break;
                        }
                    }
                }
                img.setRGB(x, y, rgb);
            }
        }
        cachedLayerImage = img;
    }

    @Override
    protected void paintComponent(Graphics g0) {
        super.paintComponent(g0);
        Graphics2D g = (Graphics2D) g0;
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
        g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BILINEAR);

        if (report == null || report.grid == null || cachedLayerImage == null) {
            g.setColor(new Color(160, 175, 195));
            g.drawString("Loading planetary telemetry...", 30, 40);
            return;
        }

        int[] vp = computeMapViewport();
        int mx = vp[0];
        int my = vp[1];
        int mw = vp[2];
        int mh = vp[3];

        // Top HUD Header
        g.setFont(new Font("SansSerif", Font.BOLD, 12));
        g.setColor(new Color(210, 225, 245));
        String heatTag = heatmapEnabled ? "Heatmap: ON" : "Heatmap: OFF (Natural)";
        String title = String.format(Locale.US,
                "%s  |  Active File: %s (%s, %dx%d)  |  %s",
                report.bodyDisplay, report.uploadedFile, report.uploadedFormat,
                report.uploadedWidth, report.uploadedHeight, heatTag);
        g.drawString(title, 14, 21);

        // Draw map raster preserving exact aspect ratio
        g.drawImage(cachedLayerImage, mx, my, mw, mh, null);

        // Subtle coordinate graticule grid
        g.setColor(new Color(255, 255, 255, 28));
        g.setStroke(new BasicStroke(1.0f));
        for (int k = 1; k < 4; k++) {
            int gx = mx + (mw * k) / 4;
            int gy = my + (mh * k) / 4;
            g.drawLine(gx, my, gx, my + mh);
            g.drawLine(mx, gy, mx + mw, gy);
        }

        // Draw hover crosshair
        if (hoverGridX >= 0 && hoverGridY >= 0) {
            int hx = mx + (int) ((hoverGridX / (double) Math.max(1, report.grid.width - 1)) * mw);
            int hy = my + (int) ((hoverGridY / (double) Math.max(1, report.grid.height - 1)) * mh);
            g.setColor(new Color(255, 255, 255, 75));
            g.setStroke(new BasicStroke(1.0f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER,
                    10.0f, new float[]{4.0f, 4.0f}, 0.0f));
            g.drawLine(mx, hy, mx + mw, hy);
            g.drawLine(hx, my, hx, my + mh);
        }

        if (!report.isImportedMap) {
            // Draw static Recommended Settlement Zones ONLY on the default baseline map
            drawBaselineSiteOverlays(g, mx, my, mw, mh);
        } else {
            // Static L1/L2/M1/M2 circles disappear on imported maps; highlight detected craters instead
            drawDetectedCraterOverlays(g, mx, my, mw, mh);
        }

        // Map border
        g.setColor(new Color(75, 92, 120));
        g.setStroke(new BasicStroke(1.5f));
        g.drawRect(mx, my, mw, mh);

        // Draw Colorbar Legend (or Natural Relief badge when heatmap is disabled)
        if (heatmapEnabled) {
            drawLegend(g, mx + mw - 155, my + mh - 28, 145, 18);
        } else {
            drawNaturalBadge(g, mx + mw - 188, my + mh - 28, 178, 18);
        }

        // Draw Bottom Cross-Section Profile + Hover Telemetry Bar
        drawCrossSectionProfile(g, 14, my + mh + 10, getWidth() - 28, getHeight() - (my + mh + 16));
    }

    private void drawBaselineSiteOverlays(Graphics2D g, int mx, int my, int mw, int mh) {
        for (int i = 0; i < report.sites.size(); i++) {
            SiteEvaluation s = report.sites.get(i);
            double cx = mx + s.normX * mw;
            double cy = my + s.normY * mh;
            double r = s.normRadius * Math.min(mw, mh);

            Color ringColor = statusColor(s.status);
            boolean isSelected = (i == selectedSiteIndex);

            g.setColor(new Color(ringColor.getRed(), ringColor.getGreen(), ringColor.getBlue(),
                    isSelected ? 58 : 30));
            g.fill(new Ellipse2D.Double(cx - r, cy - r, r * 2, r * 2));

            g.setColor(ringColor);
            g.setStroke(new BasicStroke(isSelected ? 2.8f : 1.8f));
            g.draw(new Ellipse2D.Double(cx - r, cy - r, r * 2, r * 2));

            if (isSelected) {
                g.setColor(Color.WHITE);
                g.setStroke(new BasicStroke(1.2f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER,
                        10.0f, new float[]{5.0f, 4.0f}, 0.0f));
                g.draw(new Ellipse2D.Double(cx - r - 4, cy - r - 4, (r + 4) * 2, (r + 4) * 2));
            }

            g.setColor(Color.WHITE);
            g.setStroke(new BasicStroke(1.4f));
            g.drawLine((int) cx - 5, (int) cy, (int) cx + 5, (int) cy);
            g.drawLine((int) cx, (int) cy - 5, (int) cx, (int) cy + 5);

            String tag = String.format(Locale.US, "%s (%.0f)", s.id, s.uploadScore);
            drawTagPill(g, tag, ringColor, cx, cy, r, my);
        }
    }

    private void drawDetectedCraterOverlays(Graphics2D g, int mx, int my, int mw, int mh) {
        String topEarthName = !report.earthMatches.isEmpty() ? report.earthMatches.get(0).craterNameShort() : "";
        for (int i = 0; i < report.detectedCraters.size(); i++) {
            DetectedCraterInfo dc = report.detectedCraters.get(i);
            double cx = mx + dc.normX * mw;
            double cy = my + dc.normY * mh;
            double r = dc.normRadius * Math.min(mw, mh);

            boolean isPrimary = (i == 0);
            boolean isSelected = (i == selectedCraterIndex);
            Color ringColor = icePotentialColor(dc.icePotentialLabel);

            g.setColor(new Color(ringColor.getRed(), ringColor.getGreen(), ringColor.getBlue(),
                    isSelected ? 42 : 18));
            g.fill(new Ellipse2D.Double(cx - r, cy - r, r * 2, r * 2));

            g.setColor(ringColor);
            g.setStroke(new BasicStroke(isSelected ? 2.5f : 1.5f, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND));
            g.draw(new Ellipse2D.Double(cx - r, cy - r, r * 2, r * 2));

            if (isSelected) {
                g.setColor(Color.WHITE);
                g.setStroke(new BasicStroke(1.2f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER,
                        10.0f, new float[]{5.0f, 4.0f}, 0.0f));
                g.draw(new Ellipse2D.Double(cx - r - 4, cy - r - 4, (r + 4) * 2, (r + 4) * 2));
            }

            g.setColor(Color.WHITE);
            g.setStroke(new BasicStroke(1.2f));
            g.drawLine((int) cx - 4, (int) cy, (int) cx + 4, (int) cy);
            g.drawLine((int) cx, (int) cy - 4, (int) cx, (int) cy + 4);

            String tag = isPrimary && !topEarthName.isEmpty()
                    ? String.format(Locale.US, "%s • Ice: %s (%s)", dc.id, dc.icePotentialLabel, topEarthName)
                    : String.format(Locale.US, "%s • Ice: %s", dc.id, dc.icePotentialLabel);
            drawTagPill(g, tag, ringColor, cx, cy, r, my);
        }
    }

    private void drawTagPill(Graphics2D g, String tag, Color ringColor, double cx, double cy, double r, int my) {
        g.setFont(new Font("SansSerif", Font.BOLD, 11));
        FontMetrics fm = g.getFontMetrics();
        int tw = fm.stringWidth(tag) + 10;
        int th = 18;
        int tx = (int) (cx - tw / 2.0);
        int ty = (int) (cy - r - th - 4);
        if (ty < my + 4) ty = (int) (cy + r + 4);

        g.setColor(new Color(12, 16, 24, 215));
        g.fillRoundRect(tx, ty, tw, th, 6, 6);
        g.setColor(ringColor);
        g.drawRoundRect(tx, ty, tw, th, 6, 6);
        g.setColor(Color.WHITE);
        g.drawString(tag, tx + 5, ty + 13);
    }

    private void drawNaturalBadge(Graphics2D g, int x, int y, int w, int h) {
        g.setColor(new Color(12, 16, 24, 215));
        g.fillRoundRect(x - 4, y - 3, w + 8, h + 6, 6, 6);
        g.setColor(new Color(90, 110, 140));
        g.drawRoundRect(x - 4, y - 3, w + 8, h + 6, 6, 6);
        g.setFont(new Font("SansSerif", Font.BOLD, 10));
        g.setColor(new Color(205, 220, 240));
        g.drawString("Heatmap OFF • Natural Terrain View", x + 4, y + 13);
    }

    private void drawLegend(Graphics2D g, int x, int y, int w, int h) {
        g.setColor(new Color(12, 16, 24, 210));
        g.fillRoundRect(x - 6, y - 4, w + 12, h + 8, 6, 6);
        for (int i = 0; i < w; i++) {
            float t = i / (float) Math.max(1, w - 1);
            int rgb;
            switch (layerMode) {
                case ELEVATION:
                    rgb = "mars".equalsIgnoreCase(report.body)
                            ? marsElevationColor(t, 1.0f) : moonElevationColor(t, 1.0f);
                    break;
                case SLOPE:
                    rgb = slopeColor(t, 1.0f);
                    break;
                case RESIDUAL_DIFF:
                    rgb = diffColor((t - 0.5f) * 120.0f);
                    break;
                default:
                    rgb = viabilityColor(t, 1.0f);
                    break;
            }
            g.setColor(new Color(rgb));
            g.drawLine(x + i, y, x + i, y + h);
        }
        g.setColor(new Color(220, 230, 245));
        g.setFont(new Font("SansSerif", Font.PLAIN, 10));
        String leftLbl = layerMode == LayerMode.SLOPE ? "0°"
                : layerMode == LayerMode.RESIDUAL_DIFF ? "-60m"
                : layerMode == LayerMode.ELEVATION ? String.format(Locale.US, "%.0fm", report.elevMinM) : "0 (Unsafe)";
        String rightLbl = layerMode == LayerMode.SLOPE ? "16°+"
                : layerMode == LayerMode.RESIDUAL_DIFF ? "+60m"
                : layerMode == LayerMode.ELEVATION ? String.format(Locale.US, "%.0fm", report.elevMaxM) : "100 (Ideal)";
        g.drawString(leftLbl, x + 2, y + h - 3);
        int rw = g.getFontMetrics().stringWidth(rightLbl);
        g.drawString(rightLbl, x + w - rw - 2, y + h - 3);
    }

    private void drawCrossSectionProfile(Graphics2D g, int x, int y, int w, int h) {
        if (h < 50 || report == null || report.grid == null) return;
        g.setColor(new Color(20, 26, 38));
        g.fillRoundRect(x, y, w, h, 8, 8);
        g.setColor(new Color(52, 66, 90));
        g.setStroke(new BasicStroke(1.0f));
        g.drawRoundRect(x, y, w, h, 8, 8);

        TelemetryGrid grid = report.grid;
        int row = Math.max(0, Math.min(grid.height - 1, hoverGridY >= 0 ? hoverGridY : grid.height / 2));
        int col = Math.max(0, Math.min(grid.width - 1, hoverGridX >= 0 ? hoverGridX : grid.width / 2));
        int idx = row * grid.width + col;

        g.setFont(new Font("Monospaced", Font.BOLD, 11));
        g.setColor(new Color(145, 205, 255));
        String hud = String.format(Locale.US,
                "CURSOR [%d, %d]  |  Elev: %+.1f  |  Slope: %.2f°  |  Roughness (TRI): %.2f  |  Viability: %.1f/100  |  Δz vs Base: %+.2f",
                col, row, grid.elevation[idx], grid.slopeDeg[idx], grid.roughnessM[idx],
                grid.viabilityScore[idx], grid.residualDiffM[idx]);
        g.drawString(hud, x + 10, y + 16);

        int gx = x + 10;
        int gy = y + 24;
        int gw = w - 20;
        int gh = h - 30;
        if (gh < 20) return;

        float minZ = (float) report.elevMinM;
        float maxZ = (float) report.elevMaxM;
        float span = Math.max(1e-3f, maxZ - minZ);

        Path2D.Double elevPath = new Path2D.Double();
        Path2D.Double viabPath = new Path2D.Double();
        int step = Math.max(1, grid.width / 400);
        boolean first = true;
        for (int ix = 0; ix < grid.width; ix += step) {
            double px = gx + (ix / (double) Math.max(1, grid.width - 1)) * gw;
            float z = grid.elevation[row * grid.width + ix];
            float v = grid.viabilityScore[row * grid.width + ix];
            double pyZ = gy + gh - ((z - minZ) / span) * (gh - 4) - 2;
            double pyV = gy + gh - (v / 100.0) * (gh - 4) - 2;
            if (first) {
                elevPath.moveTo(px, pyZ);
                viabPath.moveTo(px, pyV);
                first = false;
            } else {
                elevPath.lineTo(px, pyZ);
                viabPath.lineTo(px, pyV);
            }
        }

        g.setColor(new Color(245, 165, 75));
        g.setStroke(new BasicStroke(1.8f));
        g.draw(elevPath);

        g.setColor(new Color(45, 212, 145));
        g.setStroke(new BasicStroke(1.5f));
        g.draw(viabPath);

        g.setFont(new Font("SansSerif", Font.PLAIN, 10));
        g.setColor(new Color(245, 165, 75));
        g.drawString("— Elevation Profile (E-W)", gx + 6, gy + 12);
        g.setColor(new Color(45, 212, 145));
        g.drawString("— Viability Score (0-100)", gx + 155, gy + 12);
    }

    public static Color statusColor(String status) {
        if (status == null) return new Color(56, 189, 248);
        if (status.contains("CONFIRMED")) return new Color(34, 197, 94);
        if (status.contains("UPGRADED")) return new Color(20, 184, 166);
        if (status.contains("DOWNGRADED") || status.contains("HAZARD")) return new Color(239, 68, 68);
        return new Color(245, 158, 11);
    }

    public static Color icePotentialColor(String label) {
        if ("HIGH".equalsIgnoreCase(label)) return new Color(56, 189, 248);
        if ("MODERATE".equalsIgnoreCase(label)) return new Color(45, 212, 191);
        return new Color(250, 204, 21);
    }

    private static float clamp01(float v) {
        return Math.max(0.0f, Math.min(1.0f, v));
    }

    private static int packRgb(int r, int g, int b, float shade) {
        int rs = Math.max(0, Math.min(255, Math.round(r * shade)));
        int gs = Math.max(0, Math.min(255, Math.round(g * shade)));
        int bs = Math.max(0, Math.min(255, Math.round(b * shade)));
        return (rs << 16) | (gs << 8) | bs;
    }

    private static int lerp(int a, int b, float t) {
        return Math.round(a + (b - a) * clamp01(t));
    }

    private static int marsElevationColor(float t, float shade) {
        if (t < 0.35f) {
            float s = t / 0.35f;
            return packRgb(lerp(42, 128, s), lerp(52, 74, s), lerp(96, 62, s), shade);
        } else if (t < 0.70f) {
            float s = (t - 0.35f) / 0.35f;
            return packRgb(lerp(128, 212, s), lerp(74, 115, s), lerp(62, 56, s), shade);
        } else {
            float s = (t - 0.70f) / 0.30f;
            return packRgb(lerp(212, 248, s), lerp(115, 208, s), lerp(56, 148, s), shade);
        }
    }

    private static int moonElevationColor(float t, float shade) {
        if (t < 0.40f) {
            float s = t / 0.40f;
            return packRgb(lerp(30, 96, s), lerp(38, 110, s), lerp(72, 134, s), shade);
        } else if (t < 0.75f) {
            float s = (t - 0.40f) / 0.35f;
            return packRgb(lerp(96, 182, s), lerp(110, 192, s), lerp(134, 206, s), shade);
        } else {
            float s = (t - 0.75f) / 0.25f;
            return packRgb(lerp(182, 246, s), lerp(192, 248, s), lerp(206, 252, s), shade);
        }
    }

    private static int viabilityColor(float t, float shade) {
        if (t < 0.50f) {
            float s = t / 0.50f;
            return packRgb(lerp(195, 238, s), lerp(40, 168, s), lerp(48, 36, s), shade);
        } else {
            float s = (t - 0.50f) / 0.50f;
            return packRgb(lerp(238, 28, s), lerp(168, 204, s), lerp(36, 112, s), shade);
        }
    }

    private static int slopeColor(float t, float shade) {
        if (t < 0.45f) {
            float s = t / 0.45f;
            return packRgb(lerp(28, 240, s), lerp(188, 190, s), lerp(108, 42, s), shade);
        } else {
            float s = (t - 0.45f) / 0.55f;
            return packRgb(lerp(240, 225, s), lerp(190, 32, s), lerp(42, 48, s), shade);
        }
    }

    private static int diffColor(float diffM) {
        float norm = Math.max(-1.0f, Math.min(1.0f, diffM / 60.0f));
        if (norm < 0.0f) {
            float s = -norm;
            return packRgb(lerp(235, 35, s), lerp(238, 95, s), lerp(242, 225, s), 1.0f);
        } else {
            float s = norm;
            return packRgb(lerp(235, 228, s), lerp(238, 48, s), lerp(242, 52, s), 1.0f);
        }
    }
}
