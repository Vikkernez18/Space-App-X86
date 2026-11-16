package planetary.ui;

import java.awt.BasicStroke;
import java.awt.BorderLayout;
import java.awt.Color;
import java.awt.Dimension;
import java.awt.FlowLayout;
import java.awt.Font;
import java.awt.FontMetrics;
import java.awt.Graphics;
import java.awt.Graphics2D;
import java.awt.GridLayout;
import java.awt.RenderingHints;
import java.awt.geom.Ellipse2D;
import java.awt.geom.Path2D;
import java.awt.image.BufferedImage;
import java.util.Locale;
import javax.swing.BorderFactory;
import javax.swing.ButtonGroup;
import javax.swing.JLabel;
import javax.swing.JPanel;
import javax.swing.JScrollPane;
import javax.swing.JTextArea;
import javax.swing.JToggleButton;
import javax.swing.border.EmptyBorder;

import planetary.model.ViabilityModels.AnalysisReport;
import planetary.model.ViabilityModels.DetectedCraterInfo;
import planetary.model.ViabilityModels.EarthCraterGrids;
import planetary.model.ViabilityModels.EarthCraterMatchInfo;
import planetary.model.ViabilityModels.TelemetryGrid;

public class CraterComparisonPanel extends JPanel {
    private static final long serialVersionUID = 1L;

    private static final Color BG_DARK = new Color(14, 18, 26);
    private static final Color BG_CARD = new Color(22, 29, 43);
    private static final Color BORDER_COL = new Color(51, 65, 85);
    private static final Color FG_TEXT = new Color(226, 232, 240);
    private static final Color FG_MUTED = new Color(148, 163, 184);
    private static final Color ACCENT_CYAN = new Color(56, 189, 248);
    private static final Color ACCENT_GREEN = new Color(52, 211, 153);

    private AnalysisReport report;
    private int selectedMatchIndex = 0;

    private BufferedImage originalRenderImg;
    private BufferedImage earthCraterRenderImg;

    private JLabel headlineLabel;
    private JLabel iceBadgeLabel;
    private JPanel matchButtonsPanel;
    private DualMapCompareCanvas dualCanvas;
    private RadialProfileChartPanel profileChart;
    private JTextArea featureNotesArea;

    public CraterComparisonPanel() {
        setBackground(BG_DARK);
        setLayout(new BorderLayout(8, 8));
        setBorder(new EmptyBorder(8, 8, 8, 8));

        add(buildTopSummaryBar(), BorderLayout.NORTH);

        dualCanvas = new DualMapCompareCanvas();
        add(dualCanvas, BorderLayout.CENTER);

        add(buildBottomAnalysisStrip(), BorderLayout.SOUTH);
    }

    private JPanel buildTopSummaryBar() {
        JPanel top = new JPanel(new BorderLayout(8, 6));
        top.setBackground(BG_CARD);
        top.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(BORDER_COL),
                new EmptyBorder(8, 12, 8, 12)));

        JPanel left = new JPanel(new BorderLayout(4, 4));
        left.setOpaque(false);

        headlineLabel = new JLabel("Earth Crater Database Comparison — Upload or select a map to analyze craters");
        headlineLabel.setFont(new Font("SansSerif", Font.BOLD, 13));
        headlineLabel.setForeground(ACCENT_CYAN);

        matchButtonsPanel = new JPanel(new FlowLayout(FlowLayout.LEFT, 6, 0));
        matchButtonsPanel.setOpaque(false);

        left.add(headlineLabel, BorderLayout.NORTH);
        left.add(matchButtonsPanel, BorderLayout.SOUTH);

        iceBadgeLabel = new JLabel("ICE POTENTIAL: --");
        iceBadgeLabel.setFont(new Font("SansSerif", Font.BOLD, 12));
        iceBadgeLabel.setForeground(Color.WHITE);
        iceBadgeLabel.setOpaque(true);
        iceBadgeLabel.setBackground(new Color(30, 58, 138));
        iceBadgeLabel.setBorder(new EmptyBorder(6, 12, 6, 12));

        top.add(left, BorderLayout.CENTER);
        top.add(iceBadgeLabel, BorderLayout.EAST);
        return top;
    }

    private JPanel buildBottomAnalysisStrip() {
        JPanel bottom = new JPanel(new GridLayout(1, 2, 10, 0));
        bottom.setOpaque(false);
        bottom.setPreferredSize(new Dimension(800, 215));

        profileChart = new RadialProfileChartPanel();
        profileChart.setBorder(BorderFactory.createLineBorder(BORDER_COL));

        featureNotesArea = new JTextArea();
        featureNotesArea.setEditable(false);
        featureNotesArea.setLineWrap(true);
        featureNotesArea.setWrapStyleWord(true);
        featureNotesArea.setBackground(BG_CARD);
        featureNotesArea.setForeground(FG_TEXT);
        featureNotesArea.setFont(new Font("Monospaced", Font.PLAIN, 11));
        featureNotesArea.setBorder(new EmptyBorder(8, 10, 8, 10));

        JScrollPane notesScroll = new JScrollPane(featureNotesArea);
        notesScroll.setBorder(BorderFactory.createTitledBorder(
                BorderFactory.createLineBorder(BORDER_COL),
                "Morphological Feature Match & Ice-Deposit Assessment"));
        ((javax.swing.border.TitledBorder) notesScroll.getBorder()).setTitleColor(ACCENT_CYAN);
        ((javax.swing.border.TitledBorder) notesScroll.getBorder()).setTitleFont(
                new Font("SansSerif", Font.BOLD, 11));

        bottom.add(profileChart);
        bottom.add(notesScroll);
        return bottom;
    }

    public void setReport(AnalysisReport report) {
        this.report = report;
        this.selectedMatchIndex = 0;
        rebuildOriginalImage();
        rebuildMatchButtons();
        updateSelectedMatchView();
    }

    private void rebuildMatchButtons() {
        matchButtonsPanel.removeAll();
        if (report == null || report.earthMatches.isEmpty()) {
            JLabel none = new JLabel("No Earth crater match data yet.");
            none.setForeground(FG_MUTED);
            none.setFont(new Font("SansSerif", Font.PLAIN, 11));
            matchButtonsPanel.add(none);
            matchButtonsPanel.revalidate();
            matchButtonsPanel.repaint();
            return;
        }

        ButtonGroup group = new ButtonGroup();
        for (int i = 0; i < report.earthMatches.size(); i++) {
            EarthCraterMatchInfo em = report.earthMatches.get(i);
            final int idx = i;
            String label = String.format(Locale.US, "#%d %s (%.1f%%)",
                    i + 1, em.craterNameShort(), em.similarityScore);
            JToggleButton btn = new JToggleButton(label, i == selectedMatchIndex);
            btn.setFont(new Font("SansSerif", Font.BOLD, 11));
            btn.setFocusPainted(false);
            btn.setForeground(Color.WHITE);
            btn.setBackground(i == selectedMatchIndex ? new Color(14, 116, 144) : new Color(36, 48, 70));
            btn.addActionListener(e -> {
                selectedMatchIndex = idx;
                for (int k = 0; k < matchButtonsPanel.getComponentCount(); k++) {
                    if (matchButtonsPanel.getComponent(k) instanceof JToggleButton) {
                        JToggleButton b = (JToggleButton) matchButtonsPanel.getComponent(k);
                        b.setBackground(k == idx ? new Color(14, 116, 144) : new Color(36, 48, 70));
                    }
                }
                updateSelectedMatchView();
            });
            group.add(btn);
            matchButtonsPanel.add(btn);
        }
        matchButtonsPanel.revalidate();
        matchButtonsPanel.repaint();
    }

    private void updateSelectedMatchView() {
        if (report == null || report.detectedCraters.isEmpty() || report.earthMatches.isEmpty()) {
            headlineLabel.setText("No crater match available.");
            iceBadgeLabel.setText("ICE POTENTIAL: N/A");
            earthCraterRenderImg = null;
            featureNotesArea.setText("");
            dualCanvas.repaint();
            profileChart.repaint();
            return;
        }

        DetectedCraterInfo dc = report.detectedCraters.get(0);
        int idx = Math.max(0, Math.min(report.earthMatches.size() - 1, selectedMatchIndex));
        EarthCraterMatchInfo em = report.earthMatches.get(idx);

        headlineLabel.setText(String.format(Locale.US,
                "Uploaded Map [%s]  •  Identified Earth Analog #%d: %s (%s, %s)  —  %.1f%% Feature Match (r = %.3f)",
                report.uploadedFile, idx + 1, em.name, em.location, em.country,
                em.similarityScore, em.profileCorrelation));

        iceBadgeLabel.setText(String.format(Locale.US,
                "ICE DEPOSIT POTENTIAL: %s (%.0f/100)", dc.icePotentialLabel, dc.icePotentialScore));
        if ("HIGH".equalsIgnoreCase(dc.icePotentialLabel)) {
            iceBadgeLabel.setBackground(new Color(2, 132, 199));
        } else if ("MODERATE".equalsIgnoreCase(dc.icePotentialLabel)) {
            iceBadgeLabel.setBackground(new Color(13, 148, 136));
        } else {
            iceBadgeLabel.setBackground(new Color(180, 83, 9));
        }

        rebuildEarthCraterImage(idx);

        StringBuilder sb = new StringBuilder();
        sb.append(String.format(Locale.US, "DETECTED CRATER (%s) vs. EARTH ANALOG (%s)\n", dc.id, em.name));
        sb.append("-------------------------------------------------------------\n");
        sb.append(String.format(Locale.US, "Morphology Class   : %s\n", em.morphology));
        sb.append(String.format(Locale.US, "Depth / Diam (d/D) : Uploaded %.4f   |   Earth %.4f\n",
                dc.depthToDiameter, em.depthToDiameter));
        sb.append(String.format(Locale.US, "Rim / Depth Ratio  : Uploaded %.3f    |   Earth %.3f\n",
                dc.rimToDepth, em.rimToDepth));
        sb.append(String.format(Locale.US, "Circularity        : Uploaded %.3f    |   Earth %.3f\n",
                dc.circularity, em.circularity));
        sb.append(String.format(Locale.US, "Floor Flatness     : Uploaded %.3f    |   Earth %.3f\n",
                dc.floorFlatness, em.floorFlatness));
        sb.append(String.format(Locale.US, "Central Peak       : Uploaded %.3f    |   Earth %.3f\n",
                dc.centralPeak, em.centralPeak));
        sb.append(String.format(Locale.US, "Inner Wall Slope   : Uploaded %.1f°   |   Earth %.1f°\n",
                dc.wallSlopeDeg, em.wallSlopeDeg));
        sb.append(String.format(Locale.US, "Profile Corr (r)   : %.4f  (Overall Match: %.1f%%)\n",
                em.profileCorrelation, em.similarityScore));
        sb.append("-------------------------------------------------------------\n");
        sb.append("ICE DEPOSIT FINDING: ").append(dc.iceRationale).append("\n");
        sb.append("EARTH ICE ANALOG   : ").append(em.iceAnalogNotes).append("\n");
        sb.append("GEOLOGICAL CONTEXT : ").append(em.geologyNotes);

        featureNotesArea.setText(sb.toString());
        featureNotesArea.setCaretPosition(0);

        dualCanvas.repaint();
        profileChart.repaint();
    }

    private void rebuildOriginalImage() {
        if (report == null || report.grid == null) {
            originalRenderImg = null;
            return;
        }
        if (report.originalUploadedImage != null) {
            originalRenderImg = report.originalUploadedImage;
            return;
        }
        TelemetryGrid g = report.grid;
        originalRenderImg = renderShadedTerrain(g.width, g.height, g.elevation,
                (float) report.elevMinM, (float) report.elevMaxM, false);
    }

    private void rebuildEarthCraterImage(int matchIdx) {
        if (report == null || report.earthCraterGrids == null
                || matchIdx < 0 || matchIdx >= report.earthCraterGrids.count) {
            earthCraterRenderImg = null;
            return;
        }
        EarthCraterGrids grids = report.earthCraterGrids;
        float[] elev = grids.elevations[matchIdx];
        float minZ = Float.POSITIVE_INFINITY;
        float maxZ = Float.NEGATIVE_INFINITY;
        for (float v : elev) {
            if (v < minZ) minZ = v;
            if (v > maxZ) maxZ = v;
        }
        earthCraterRenderImg = renderShadedTerrain(grids.width, grids.height, elev, minZ, maxZ, true);
    }

    private static BufferedImage renderShadedTerrain(
            int w, int h, float[] elev, float minZ, float maxZ, boolean earthPalette) {
        BufferedImage img = new BufferedImage(w, h, BufferedImage.TYPE_INT_RGB);
        float span = Math.max(1e-3f, maxZ - minZ);
        float shadeScale = Math.max(0.5f, span * 0.045f);

        for (int y = 0; y < h; y++) {
            int y0 = Math.max(0, y - 1);
            int y1 = Math.min(h - 1, y + 1);
            for (int x = 0; x < w; x++) {
                int x0 = Math.max(0, x - 1);
                int x1 = Math.min(w - 1, x + 1);
                float dzdx = elev[y * w + x1] - elev[y * w + x0];
                float dzdy = elev[y1 * w + x] - elev[y0 * w + x];
                float shade = (float) Math.tanh((-dzdx - dzdy) / shadeScale) * 0.30f + 1.0f;
                float t = Math.max(0.0f, Math.min(1.0f, (elev[y * w + x] - minZ) / span));

                int r, g, b;
                if (earthPalette) {
                    // Natural Earth crater elevation relief (deep basin blue-slate -> olive-tan -> bright rim)
                    if (t < 0.40f) {
                        float s = t / 0.40f;
                        r = lerp(34, 78, s);
                        g = lerp(62, 118, s);
                        b = lerp(112, 124, s);
                    } else if (t < 0.72f) {
                        float s = (t - 0.40f) / 0.32f;
                        r = lerp(78, 168, s);
                        g = lerp(118, 152, s);
                        b = lerp(124, 108, s);
                    } else {
                        float s = (t - 0.72f) / 0.28f;
                        r = lerp(168, 238, s);
                        g = lerp(152, 224, s);
                        b = lerp(108, 198, s);
                    }
                } else {
                    int gray = lerp(36, 232, t);
                    r = gray;
                    g = gray;
                    b = Math.min(255, gray + 8);
                }

                int rs = Math.max(0, Math.min(255, Math.round(r * shade)));
                int gs = Math.max(0, Math.min(255, Math.round(g * shade)));
                int bs = Math.max(0, Math.min(255, Math.round(b * shade)));
                img.setRGB(x, y, (rs << 16) | (gs << 8) | bs);
            }
        }
        return img;
    }

    private static int lerp(int a, int b, float t) {
        return Math.round(a + (b - a) * Math.max(0.0f, Math.min(1.0f, t)));
    }

    private class DualMapCompareCanvas extends JPanel {
        private static final long serialVersionUID = 1L;

        DualMapCompareCanvas() {
            setBackground(BG_DARK);
        }

        @Override
        protected void paintComponent(Graphics g0) {
            super.paintComponent(g0);
            Graphics2D g = (Graphics2D) g0;
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
            g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BILINEAR);

            int w = getWidth();
            int h = getHeight();
            int gap = 14;
            int halfW = Math.max(100, (w - gap) / 2);

            drawSidePanel(g, 0, 0, halfW, h, true);
            drawSidePanel(g, halfW + gap, 0, w - (halfW + gap), h, false);
        }

        private void drawSidePanel(Graphics2D g, int x, int y, int w, int h, boolean isOriginal) {
            g.setColor(BG_CARD);
            g.fillRoundRect(x, y, w, h, 8, 8);
            g.setColor(BORDER_COL);
            g.setStroke(new BasicStroke(1.2f));
            g.drawRoundRect(x, y, w, h, 8, 8);

            g.setFont(new Font("SansSerif", Font.BOLD, 12));
            g.setColor(isOriginal ? ACCENT_CYAN : ACCENT_GREEN);

            String title;
            String sub;
            BufferedImage img;
            if (isOriginal) {
                img = originalRenderImg;
                if (report != null) {
                    title = "ORIGINAL IMPORTED MAP — " + report.uploadedFile;
                    sub = String.format(Locale.US, "%s (%dx%d) • Detected Crater Rim Highlighted",
                            report.uploadedFormat, report.uploadedWidth, report.uploadedHeight);
                } else {
                    title = "ORIGINAL IMPORTED MAP";
                    sub = "No map loaded";
                }
            } else {
                img = earthCraterRenderImg;
                if (report != null && !report.earthMatches.isEmpty()) {
                    int idx = Math.max(0, Math.min(report.earthMatches.size() - 1, selectedMatchIndex));
                    EarthCraterMatchInfo em = report.earthMatches.get(idx);
                    title = "IDENTIFIED EARTH CRATER — " + em.name;
                    sub = String.format(Locale.US, "%s, %s (%.2f°, %.2f°) • Ø %.2f km, Depth %.0f m • %s",
                            em.location, em.country, em.latDeg, em.lonDeg,
                            em.diameterKm, em.depthM, em.morphology);
                } else {
                    title = "IDENTIFIED EARTH CRATER";
                    sub = "No Earth crater match loaded";
                }
            }

            g.drawString(title, x + 12, y + 20);
            g.setFont(new Font("SansSerif", Font.PLAIN, 10));
            g.setColor(FG_MUTED);
            g.drawString(sub, x + 12, y + 35);

            if (img == null) {
                g.setColor(FG_MUTED);
                g.drawString("No raster image available.", x + 20, y + 70);
                return;
            }

            int pad = 12;
            int topOff = 44;
            int availW = Math.max(40, w - pad * 2);
            int availH = Math.max(40, h - topOff - pad);

            double imgAspect = img.getWidth() / (double) Math.max(1, img.getHeight());
            int dw, dh;
            if ((availW / (double) availH) > imgAspect) {
                dh = availH;
                dw = Math.max(20, (int) Math.round(availH * imgAspect));
            } else {
                dw = availW;
                dh = Math.max(20, (int) Math.round(availW / imgAspect));
            }
            int dx = x + (w - dw) / 2;
            int dy = y + topOff + (availH - dh) / 2;

            g.drawImage(img, dx, dy, dw, dh, null);
            g.setColor(new Color(90, 110, 140));
            g.setStroke(new BasicStroke(1.0f));
            g.drawRect(dx, dy, dw, dh);

            // Overlay detected crater ring on original OR canonical rim ring on Earth crater
            if (isOriginal && report != null && !report.detectedCraters.isEmpty()) {
                DetectedCraterInfo dc = report.detectedCraters.get(0);
                double cx = dx + dc.normX * dw;
                double cy = dy + dc.normY * dh;
                double rad = dc.normRadius * Math.min(dw, dh);
                drawCraterOverlay(g, cx, cy, rad, ACCENT_CYAN,
                        String.format(Locale.US, "%s (d/D=%.3f)", dc.id, dc.depthToDiameter));
            } else if (!isOriginal && report != null && !report.earthMatches.isEmpty()) {
                int idx = Math.max(0, Math.min(report.earthMatches.size() - 1, selectedMatchIndex));
                EarthCraterMatchInfo em = report.earthMatches.get(idx);
                double cx = dx + 0.5 * dw;
                double cy = dy + 0.5 * dh;
                double rad = 0.34 * Math.min(dw, dh);
                drawCraterOverlay(g, cx, cy, rad, ACCENT_GREEN,
                        String.format(Locale.US, "Rim 1.0R (d/D=%.3f)", em.depthToDiameter));
            }
        }

        private void drawCraterOverlay(Graphics2D g, double cx, double cy, double rad, Color col, String label) {
            g.setColor(col);
            g.setStroke(new BasicStroke(2.2f));
            g.draw(new Ellipse2D.Double(cx - rad, cy - rad, rad * 2, rad * 2));

            // Cross-section scan axis
            g.setColor(new Color(col.getRed(), col.getGreen(), col.getBlue(), 160));
            g.setStroke(new BasicStroke(1.2f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER,
                    10.0f, new float[]{4.0f, 3.0f}, 0.0f));
            g.drawLine((int) (cx - rad * 1.4), (int) cy, (int) (cx + rad * 1.4), (int) cy);

            g.setColor(Color.WHITE);
            g.setStroke(new BasicStroke(1.4f));
            g.drawLine((int) cx - 5, (int) cy, (int) cx + 5, (int) cy);
            g.drawLine((int) cx, (int) cy - 5, (int) cx, (int) cy + 5);

            g.setFont(new Font("SansSerif", Font.BOLD, 10));
            FontMetrics fm = g.getFontMetrics();
            int tw = fm.stringWidth(label) + 10;
            int tx = (int) (cx - tw / 2.0);
            int ty = (int) (cy - rad - 20);
            g.setColor(new Color(12, 16, 24, 215));
            g.fillRoundRect(tx, ty, tw, 16, 5, 5);
            g.setColor(col);
            g.drawRoundRect(tx, ty, tw, 16, 5, 5);
            g.setColor(Color.WHITE);
            g.drawString(label, tx + 5, ty + 12);
        }
    }

    private class RadialProfileChartPanel extends JPanel {
        private static final long serialVersionUID = 1L;

        RadialProfileChartPanel() {
            setBackground(BG_CARD);
        }

        @Override
        protected void paintComponent(Graphics g0) {
            super.paintComponent(g0);
            Graphics2D g = (Graphics2D) g0;
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);

            int w = getWidth();
            int h = getHeight();

            g.setFont(new Font("SansSerif", Font.BOLD, 11));
            g.setColor(ACCENT_CYAN);
            g.drawString("Normalized Radial Cross-Section Profile (0.0R Center → 1.0R Rim → 1.5R Ejecta)", 12, 18);

            if (report == null || report.detectedCraters.isEmpty() || report.earthMatches.isEmpty()) {
                g.setColor(FG_MUTED);
                g.drawString("No radial profile available.", 14, 48);
                return;
            }

            DetectedCraterInfo dc = report.detectedCraters.get(0);
            int idx = Math.max(0, Math.min(report.earthMatches.size() - 1, selectedMatchIndex));
            EarthCraterMatchInfo em = report.earthMatches.get(idx);

            int gx = 42;
            int gy = 30;
            int gw = Math.max(60, w - 58);
            int gh = Math.max(40, h - 56);

            g.setColor(new Color(15, 20, 30));
            g.fillRect(gx, gy, gw, gh);
            g.setColor(BORDER_COL);
            g.drawRect(gx, gy, gw, gh);

            float minY = -1.2f;
            float maxY = 0.8f;

            // Zero background line
            int yZero = gy + (int) Math.round(((maxY - 0.0f) / (maxY - minY)) * gh);
            g.setColor(new Color(255, 255, 255, 45));
            g.setStroke(new BasicStroke(1.0f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER,
                    10.0f, new float[]{4.0f, 4.0f}, 0.0f));
            g.drawLine(gx, yZero, gx + gw, yZero);

            // Rim crest vertical marker at r = 1.0R (bin 10 out of 15)
            int xRim = gx + (int) Math.round((10.0 / 15.0) * gw);
            g.setColor(new Color(245, 158, 11, 110));
            g.drawLine(xRim, gy, xRim, gy + gh);
            g.setFont(new Font("SansSerif", Font.PLAIN, 9));
            g.drawString("Rim (1.0R)", xRim - 24, gy + 11);

            drawProfileCurve(g, dc.radialProfile, gx, gy, gw, gh, minY, maxY, ACCENT_CYAN, 2.4f);
            drawProfileCurve(g, em.radialProfile, gx, gy, gw, gh, minY, maxY, ACCENT_GREEN, 2.2f);

            // Axis labels
            g.setFont(new Font("Monospaced", Font.PLAIN, 9));
            g.setColor(FG_MUTED);
            g.drawString("0.0R", gx - 8, gy + gh + 13);
            g.drawString("0.5R", gx + gw / 3 - 10, gy + gh + 13);
            g.drawString("1.0R", xRim - 10, gy + gh + 13);
            g.drawString("1.5R", gx + gw - 20, gy + gh + 13);
            g.drawString("+0.8", 8, gy + 10);
            g.drawString("0.0", 14, yZero + 3);
            g.drawString("-1.0", 8, gy + gh - 6);

            // Legend
            g.setFont(new Font("SansSerif", Font.BOLD, 10));
            g.setColor(ACCENT_CYAN);
            g.drawString("— Imported Crater (" + dc.id + ")", gx + 8, gy + gh - 20);
            g.setColor(ACCENT_GREEN);
            g.drawString("— Earth Match (" + em.craterNameShort() + ")", gx + 8, gy + gh - 7);
        }

        private void drawProfileCurve(
                Graphics2D g, float[] prof, int gx, int gy, int gw, int gh,
                float minY, float maxY, Color col, float strokeW) {
            Path2D.Double p = new Path2D.Double();
            for (int i = 0; i < 16; i++) {
                double px = gx + (i / 15.0) * gw;
                float clamped = Math.max(minY, Math.min(maxY, prof[i]));
                double py = gy + ((maxY - clamped) / (maxY - minY)) * gh;
                if (i == 0) {
                    p.moveTo(px, py);
                } else {
                    p.lineTo(px, py);
                }
            }
            g.setColor(col);
            g.setStroke(new BasicStroke(strokeW, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND));
            g.draw(p);
        }
    }
}
