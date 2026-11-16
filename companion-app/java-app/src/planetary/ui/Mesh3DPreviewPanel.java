package planetary.ui;

import java.awt.BasicStroke;
import java.awt.Color;
import java.awt.Font;
import java.awt.Graphics;
import java.awt.Graphics2D;
import java.awt.Polygon;
import java.awt.RenderingHints;
import java.awt.event.MouseAdapter;
import java.awt.event.MouseEvent;
import java.awt.event.MouseMotionAdapter;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import javax.swing.JPanel;

import planetary.model.ViabilityModels.AnalysisReport;
import planetary.model.ViabilityModels.DetectedCraterInfo;
import planetary.model.ViabilityModels.SiteEvaluation;
import planetary.model.ViabilityModels.TelemetryGrid;

public class Mesh3DPreviewPanel extends JPanel {
    private static final long serialVersionUID = 1L;

    private AnalysisReport report;
    private boolean heatmapEnabled = true;
    private double yawRad = Math.toRadians(-38.0);
    private double pitchRad = Math.toRadians(32.0);
    private double verticalExaggeration = 1.35;
    private int lastMouseX;
    private int lastMouseY;

    private static class QuadFace implements Comparable<QuadFace> {
        final int[] xs = new int[4];
        final int[] ys = new int[4];
        final double depth;
        final Color fillColor;
        final Color edgeColor;

        QuadFace(double depth, Color fill, Color edge) {
            this.depth = depth;
            this.fillColor = fill;
            this.edgeColor = edge;
        }

        @Override
        public int compareTo(QuadFace o) {
            return Double.compare(this.depth, o.depth);
        }
    }

    public Mesh3DPreviewPanel() {
        setBackground(new Color(12, 16, 24));

        addMouseListener(new MouseAdapter() {
            @Override
            public void mousePressed(MouseEvent e) {
                lastMouseX = e.getX();
                lastMouseY = e.getY();
            }
        });

        addMouseMotionListener(new MouseMotionAdapter() {
            @Override
            public void mouseDragged(MouseEvent e) {
                int dx = e.getX() - lastMouseX;
                int dy = e.getY() - lastMouseY;
                lastMouseX = e.getX();
                lastMouseY = e.getY();
                yawRad += dx * 0.012;
                pitchRad = Math.max(Math.toRadians(10.0),
                        Math.min(Math.toRadians(78.0), pitchRad + dy * 0.010));
                repaint();
            }
        });
    }

    public void setHeatmapEnabled(boolean enabled) {
        if (this.heatmapEnabled != enabled) {
            this.heatmapEnabled = enabled;
            repaint();
        }
    }

    public void setReport(AnalysisReport report) {
        this.report = report;
        repaint();
    }

    @Override
    protected void paintComponent(Graphics g0) {
        super.paintComponent(g0);
        Graphics2D g = (Graphics2D) g0;
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);

        if (report == null || report.grid == null) {
            g.setColor(Color.LIGHT_GRAY);
            g.drawString("No 3D terrain mesh loaded.", 25, 35);
            return;
        }

        TelemetryGrid grid = report.grid;
        int gw = grid.width;
        int gh = grid.height;
        int step = Math.max(1, Math.max(gw, gh) / 90);

        int w = getWidth();
        int h = getHeight();
        double scale = Math.min(w, h) * 0.64;
        double cx = w * 0.50;
        double cy = h * 0.56;

        float minZ = (float) report.elevMinM;
        float maxZ = (float) report.elevMaxM;
        float spanZ = Math.max(1e-3f, maxZ - minZ);

        double cosY = Math.cos(yawRad);
        double sinY = Math.sin(yawRad);
        double cosP = Math.cos(pitchRad);
        double sinP = Math.sin(pitchRad);

        List<QuadFace> faces = new ArrayList<>();

        for (int y = 0; y + 1 < gh; y += step) {
            int y1 = Math.min(gh - 1, y + step);
            for (int x = 0; x + 1 < gw; x += step) {
                int x1 = Math.min(gw - 1, x + step);

                double[][] corners = {
                        projectPoint(x, y, gw, gh, grid, minZ, spanZ, cosY, sinY, cosP, sinP, scale, cx, cy),
                        projectPoint(x1, y, gw, gh, grid, minZ, spanZ, cosY, sinY, cosP, sinP, scale, cx, cy),
                        projectPoint(x1, y1, gw, gh, grid, minZ, spanZ, cosY, sinY, cosP, sinP, scale, cx, cy),
                        projectPoint(x, y1, gw, gh, grid, minZ, spanZ, cosY, sinY, cosP, sinP, scale, cx, cy)
                };

                double avgDepth = (corners[0][2] + corners[1][2] + corners[2][2] + corners[3][2]) * 0.25;
                int idx = y * gw + x;
                float zNorm = (grid.elevation[idx] - minZ) / spanZ;

                int r, gr, b;
                if (heatmapEnabled) {
                    float viab = grid.viabilityScore[idx] / 100.0f;
                    r = (int) Math.max(20, Math.min(255, (1.0f - viab) * 215 + zNorm * 40));
                    gr = (int) Math.max(25, Math.min(255, viab * 195 + zNorm * 35));
                    b = (int) Math.max(35, Math.min(255, 65 + zNorm * 95));
                } else {
                    int gray = (int) Math.max(38, Math.min(240, 46 + zNorm * 192));
                    r = gray;
                    gr = gray;
                    b = Math.min(255, gray + 8);
                }

                QuadFace q = new QuadFace(
                        avgDepth,
                        new Color(r, gr, b, 232),
                        new Color(Math.min(255, r + 24), Math.min(255, gr + 24), Math.min(255, b + 24), 75));
                for (int k = 0; k < 4; k++) {
                    q.xs[k] = (int) Math.round(corners[k][0]);
                    q.ys[k] = (int) Math.round(corners[k][1]);
                }
                faces.add(q);
            }
        }

        Collections.sort(faces);

        for (QuadFace q : faces) {
            Polygon poly = new Polygon(q.xs, q.ys, 4);
            g.setColor(q.fillColor);
            g.fillPolygon(poly);
            g.setColor(q.edgeColor);
            g.drawPolygon(poly);
        }

        if (!report.isImportedMap) {
            // Draw 3D site beacons only for baseline map
            for (SiteEvaluation s : report.sites) {
                int sx = Math.max(0, Math.min(gw - 1, (int) Math.round(s.normX * (gw - 1))));
                int sy = Math.max(0, Math.min(gh - 1, (int) Math.round(s.normY * (gh - 1))));
                double[] pt = projectPoint(sx, sy, gw, gh, grid, minZ, spanZ, cosY, sinY, cosP, sinP, scale, cx, cy);
                int px = (int) Math.round(pt[0]);
                int py = (int) Math.round(pt[1]);

                Color c = MapCanvasPanel.statusColor(s.status);
                String lbl = String.format(Locale.US, "%s (%.0f/100)", s.id, s.uploadScore);
                draw3DBeacon(g, px, py, c, lbl);
            }
        } else if (!report.detectedCraters.isEmpty()) {
            // On imported maps, show detected crater beacons instead of static L1/M1 circles
            DetectedCraterInfo dc = report.detectedCraters.get(0);
            int sx = Math.max(0, Math.min(gw - 1, (int) Math.round(dc.normX * (gw - 1))));
            int sy = Math.max(0, Math.min(gh - 1, (int) Math.round(dc.normY * (gh - 1))));
            double[] pt = projectPoint(sx, sy, gw, gh, grid, minZ, spanZ, cosY, sinY, cosP, sinP, scale, cx, cy);
            int px = (int) Math.round(pt[0]);
            int py = (int) Math.round(pt[1]);

            Color c = MapCanvasPanel.icePotentialColor(dc.icePotentialLabel);
            String earthName = !report.earthMatches.isEmpty()
                    ? report.earthMatches.get(0).craterNameShort()
                    : "Crater";
            String lbl = String.format(Locale.US, "%s (%s • Ice: %s)", dc.id, earthName, dc.icePotentialLabel);
            draw3DBeacon(g, px, py, c, lbl);
        }

        g.setFont(new Font("SansSerif", Font.BOLD, 12));
        g.setColor(new Color(205, 220, 242));
        String modeLbl = heatmapEnabled ? "Viability Heatmap ON" : "Natural Shaded Mesh (Heatmap OFF)";
        g.drawString("3D Terrain Surface Mesh (" + modeLbl + " — Drag mouse to rotate)", 16, 24);
        g.setFont(new Font("Monospaced", Font.PLAIN, 11));
        g.setColor(new Color(145, 190, 235));
        g.drawString(String.format(Locale.US,
                "Source: %s (%dx%d)  |  Yaw: %.0f°  Pitch: %.0f°  |  Elev Range: [%.1f .. %.1f]",
                report.uploadedFile, gw, gh, Math.toDegrees(yawRad), Math.toDegrees(pitchRad),
                report.elevMinM, report.elevMaxM), 16, 42);
    }

    private void draw3DBeacon(Graphics2D g, int px, int py, Color c, String lbl) {
        g.setColor(c);
        g.setStroke(new BasicStroke(2.2f));
        g.drawLine(px, py, px, py - 28);
        g.fillOval(px - 5, py - 33, 10, 10);

        g.setFont(new Font("SansSerif", Font.BOLD, 11));
        g.setColor(new Color(10, 14, 22, 215));
        int tw = g.getFontMetrics().stringWidth(lbl) + 8;
        g.fillRoundRect(px - tw / 2, py - 52, tw, 17, 5, 5);
        g.setColor(c);
        g.drawRoundRect(px - tw / 2, py - 52, tw, 17, 5, 5);
        g.setColor(Color.WHITE);
        g.drawString(lbl, px - tw / 2 + 4, py - 39);
    }

    private double[] projectPoint(
            int gx, int gy, int gw, int gh, TelemetryGrid grid,
            float minZ, float spanZ,
            double cosY, double sinY, double cosP, double sinP,
            double scale, double cx, double cy) {
        // Normalize both X and Y by max(gw, gh) so non-square STLs/images retain true aspect ratio!
        double maxSpan = Math.max(1.0, Math.max(gw, gh) - 1.0);
        double nx = (gx - (gw - 1) * 0.5) / maxSpan;
        double ny = (gy - (gh - 1) * 0.5) / maxSpan;
        float z = grid.elevation[gy * gw + gx];
        double nz = (((z - minZ) / spanZ) - 0.35) * 0.28 * verticalExaggeration;

        double rx = nx * cosY - ny * sinY;
        double ry = nx * sinY + ny * cosY;

        double screenX = cx + rx * scale;
        double screenY = cy + (ry * sinP - nz * cosP) * scale;
        double depth = ry * cosP + nz * sinP;
        return new double[]{screenX, screenY, depth};
    }
}
