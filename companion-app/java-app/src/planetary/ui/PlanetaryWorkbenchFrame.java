package planetary.ui;

import java.awt.BorderLayout;
import java.awt.Color;
import java.awt.Component;
import java.awt.Dimension;
import java.awt.FlowLayout;
import java.awt.Font;
import java.awt.GraphicsEnvironment;
import java.awt.GridBagConstraints;
import java.awt.GridBagLayout;
import java.awt.GridLayout;
import java.awt.Insets;
import java.awt.Toolkit;
import java.awt.image.BufferedImage;
import java.io.File;
import java.util.Locale;
import javax.swing.BorderFactory;
import javax.swing.JButton;
import javax.swing.JComboBox;
import javax.swing.JFileChooser;
import javax.swing.JFrame;
import javax.swing.JLabel;
import javax.swing.JPanel;
import javax.swing.JScrollPane;
import javax.swing.JSlider;
import javax.swing.JTabbedPane;
import javax.swing.JTable;
import javax.swing.JTextArea;
import javax.swing.JToggleButton;
import javax.swing.ListSelectionModel;
import javax.swing.SwingConstants;
import javax.swing.border.EmptyBorder;
import javax.swing.border.TitledBorder;
import javax.swing.filechooser.FileNameExtensionFilter;
import javax.swing.table.DefaultTableCellRenderer;
import javax.swing.table.DefaultTableModel;

import planetary.bridge.RustEngineBridge;
import planetary.model.ViabilityModels.AnalysisReport;
import planetary.model.ViabilityModels.DetectedCraterInfo;
import planetary.model.ViabilityModels.EarthCraterMatchInfo;
import planetary.model.ViabilityModels.SiteEvaluation;
import planetary.model.ViabilityModels.SuitabilityCriteria;
import planetary.ui.MapCanvasPanel.LayerMode;

public class PlanetaryWorkbenchFrame extends JFrame {
    private static final long serialVersionUID = 1L;

    private static final Color BG_DARK = new Color(15, 20, 30);
    private static final Color BG_PANEL = new Color(22, 29, 43);
    private static final Color BG_CARD = new Color(28, 37, 54);
    private static final Color FG_TEXT = new Color(226, 232, 240);
    private static final Color FG_MUTED = new Color(148, 163, 184);
    private static final Color ACCENT_BLUE = new Color(56, 189, 248);

    private final RustEngineBridge bridge;
    private String currentBody = "mars";
    private File currentUploadedFile = null;
    private final SuitabilityCriteria criteria = new SuitabilityCriteria();
    private AnalysisReport currentReport;

    private final MapCanvasPanel mapCanvas = new MapCanvasPanel();
    private final Mesh3DPreviewPanel mesh3DPanel = new Mesh3DPreviewPanel();
    private final CraterComparisonPanel craterComparisonPanel = new CraterComparisonPanel();

    private JTabbedPane centerTabs;
    private JComboBox<String> bodySelector;
    private JComboBox<LayerMode> layerSelector;
    private JToggleButton heatmapToggle;
    private JLabel statusBannerLabel;

    // Mathematical Comparison Metrics Labels
    private JLabel lblPearson;
    private JLabel lblSsim;
    private JLabel lblRmse;
    private JLabel lblMae;
    private JLabel lblResRatio;
    private JLabel lblDetailGain;
    private JLabel lblAlignment;
    private JLabel lblGlobalScore;

    // Site / Crater Table & Inspector
    private JLabel rightHeaderLabel;
    private TitledBorder detailBorder;
    private DefaultTableModel siteTableModel;
    private JTable siteTable;
    private JTextArea siteDetailArea;

    // Criteria Sliders
    private JSlider sliderMaxSlope;
    private JSlider sliderMaxRough;
    private JSlider sliderMinScore;
    private JSlider sliderWSlope;
    private JSlider sliderWRough;
    private JSlider sliderWElev;
    private JSlider sliderWSolar;

    public PlanetaryWorkbenchFrame(RustEngineBridge bridge) {
        super("JMARS Planetary Settlement Site Viability Workbench (Rust + Java)");
        this.bridge = bridge;

        setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE);
        setSize(1440, 880);
        setMinimumSize(new Dimension(1180, 740));
        getContentPane().setBackground(BG_DARK);
        setLayout(new BorderLayout(8, 8));

        add(buildTopToolbar(), BorderLayout.NORTH);
        add(buildLeftCriteriaPanel(), BorderLayout.WEST);
        add(buildCenterViewportPanel(), BorderLayout.CENTER);
        add(buildRightSitesPanel(), BorderLayout.EAST);

        mapCanvas.setOnSiteSelected(idx -> {
            if (idx >= 0 && idx < siteTable.getRowCount()) {
                siteTable.setRowSelectionInterval(idx, idx);
                updateRightDetailCard(idx);
            }
        });
        mapCanvas.setOnCraterSelected(idx -> {
            if (idx >= 0 && idx < siteTable.getRowCount()) {
                siteTable.setRowSelectionInterval(idx, idx);
                updateRightDetailCard(idx);
            }
        });

        refreshAnalysis();
    }

    private JPanel buildTopToolbar() {
        JPanel bar = new JPanel(new BorderLayout(10, 6));
        bar.setBackground(BG_PANEL);
        bar.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createMatteBorder(0, 0, 1, 0, new Color(51, 65, 85)),
                new EmptyBorder(8, 12, 8, 12)));

        JPanel leftControls = new JPanel(new FlowLayout(FlowLayout.LEFT, 8, 0));
        leftControls.setOpaque(false);

        JLabel bodyLbl = createLabel("Planetary Body:", Font.BOLD, 12, FG_TEXT);
        bodySelector = new JComboBox<>(new String[]{
                "Mars — Jezero / Arcadia Sector",
                "Moon — Shackleton South Pole Rim"
        });
        bodySelector.addActionListener(e -> {
            String target = bodySelector.getSelectedIndex() == 0 ? "mars" : "moon";
            if (!target.equals(currentBody)) {
                currentBody = target;
                currentUploadedFile = null;
                refreshAnalysis();
            }
        });

        JButton btnBaseline = createStyledButton("Default JMARS Baseline", new Color(45, 62, 88));
        btnBaseline.addActionListener(e -> {
            currentUploadedFile = null;
            refreshAnalysis();
        });

        JButton btnUpload = createStyledButton("Upload Map (.tif/.stl/.png/.jpg)...", new Color(14, 116, 144));
        btnUpload.addActionListener(e -> openUploadChooser());

        // Quick-load preset selector for testing sample uploads immediately
        JComboBox<String> presetCombo = new JComboBox<>(new String[]{
                "Quick-Load Sample Upload...",
                "Mars HiRISE Scan (.tif) — Reveals M-2 Hazard",
                "Mars HiRISE Scan (.tiff)",
                "Mars 3D Surface Mesh (Binary .stl)",
                "Moon LOLA High-Res DEM (.tiff) — Reveals L-3 Hazard",
                "Moon Crater Rim 3D Mesh (ASCII .stl)"
        });
        presetCombo.addActionListener(e -> {
            int idx = presetCombo.getSelectedIndex();
            if (idx <= 0) return;
            File upDir = bridge.getSampleUploadsDir();
            switch (idx) {
                case 1:
                    switchBodySilent("mars", 0);
                    currentUploadedFile = new File(upDir, "mars_hires_hirise_scan.tif");
                    break;
                case 2:
                    switchBodySilent("mars", 0);
                    currentUploadedFile = new File(upDir, "mars_hires_hirise_scan.tiff");
                    break;
                case 3:
                    switchBodySilent("mars", 0);
                    currentUploadedFile = new File(upDir, "mars_hires_custom_mesh.stl");
                    break;
                case 4:
                    switchBodySilent("moon", 1);
                    currentUploadedFile = new File(upDir, "moon_hires_lola_dem.tiff");
                    break;
                case 5:
                    switchBodySilent("moon", 1);
                    currentUploadedFile = new File(upDir, "moon_hires_crater_rim_ascii.stl");
                    break;
                default:
                    break;
            }
            refreshAnalysis();
        });

        leftControls.add(bodyLbl);
        leftControls.add(bodySelector);
        leftControls.add(btnBaseline);
        leftControls.add(btnUpload);
        leftControls.add(presetCombo);

        JPanel rightControls = new JPanel(new FlowLayout(FlowLayout.RIGHT, 8, 0));
        rightControls.setOpaque(false);

        JLabel layerLbl = createLabel("2D Layer:", Font.BOLD, 12, FG_TEXT);
        layerSelector = new JComboBox<>(LayerMode.values());
        layerSelector.addActionListener(e -> {
            LayerMode m = (LayerMode) layerSelector.getSelectedItem();
            if (m != null) {
                mapCanvas.setLayerMode(m);
            }
        });

        heatmapToggle = new JToggleButton("Heatmap: ON", true);
        heatmapToggle.setFont(new Font("SansSerif", Font.BOLD, 11));
        heatmapToggle.setFocusPainted(false);
        heatmapToggle.setForeground(Color.WHITE);
        heatmapToggle.setBackground(new Color(22, 163, 74));
        heatmapToggle.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(new Color(80, 105, 140)),
                new EmptyBorder(5, 10, 5, 10)));
        heatmapToggle.addActionListener(e -> {
            boolean on = heatmapToggle.isSelected();
            heatmapToggle.setText(on ? "Heatmap: ON" : "Heatmap: OFF");
            heatmapToggle.setBackground(on ? new Color(22, 163, 74) : new Color(100, 116, 139));
            layerSelector.setEnabled(on);
            mapCanvas.setHeatmapEnabled(on);
            mesh3DPanel.setHeatmapEnabled(on);
        });

        rightControls.add(layerLbl);
        rightControls.add(layerSelector);
        rightControls.add(heatmapToggle);

        bar.add(leftControls, BorderLayout.WEST);
        bar.add(rightControls, BorderLayout.EAST);
        return bar;
    }

    private void switchBodySilent(String body, int comboIdx) {
        this.currentBody = body;
        if (bodySelector.getSelectedIndex() != comboIdx) {
            bodySelector.setSelectedIndex(comboIdx);
        }
    }

    private JPanel buildLeftCriteriaPanel() {
        JPanel panel = new JPanel(new BorderLayout(6, 6));
        panel.setBackground(BG_PANEL);
        panel.setPreferredSize(new Dimension(285, 600));
        panel.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createMatteBorder(0, 0, 0, 1, new Color(51, 65, 85)),
                new EmptyBorder(10, 10, 10, 10)));

        JLabel header = createLabel("Suitability Model Parameters", Font.BOLD, 13, ACCENT_BLUE);
        panel.add(header, BorderLayout.NORTH);

        JPanel slidersBox = new JPanel(new GridBagLayout());
        slidersBox.setBackground(BG_PANEL);
        GridBagConstraints gbc = new GridBagConstraints();
        gbc.gridx = 0;
        gbc.weightx = 1.0;
        gbc.fill = GridBagConstraints.HORIZONTAL;
        gbc.insets = new Insets(3, 2, 3, 2);

        int row = 0;
        sliderMaxSlope = addSliderRow(slidersBox, gbc, row++,
                "Max Safe Landing Slope (°)", 2, 25, (int) Math.round(criteria.maxSlopeDeg), "°");
        sliderMaxRough = addSliderRow(slidersBox, gbc, row++,
                "Max Roughness TRI Cutoff (m)", 3, 35, (int) Math.round(criteria.maxRoughnessM), " m");
        sliderMinScore = addSliderRow(slidersBox, gbc, row++,
                "Min Viability Score Cutoff", 40, 92, (int) Math.round(criteria.minViabilityScore), "/100");

        sliderWSlope = addSliderRow(slidersBox, gbc, row++,
                "Weight: Slope Flatness", 5, 80, (int) Math.round(criteria.weightSlope * 100), "%");
        sliderWRough = addSliderRow(slidersBox, gbc, row++,
                "Weight: Low Roughness (TRI)", 5, 80, (int) Math.round(criteria.weightRoughness * 100), "%");
        sliderWElev = addSliderRow(slidersBox, gbc, row++,
                "Weight: Elevation / Shielding", 0, 60, (int) Math.round(criteria.weightElevation * 100), "%");
        sliderWSolar = addSliderRow(slidersBox, gbc, row++,
                "Weight: Solar Illumination", 0, 60, (int) Math.round(criteria.weightSolar * 100), "%");

        JPanel btnPanel = new JPanel(new GridLayout(2, 1, 6, 6));
        btnPanel.setBackground(BG_PANEL);
        btnPanel.setBorder(new EmptyBorder(8, 0, 0, 0));

        JButton btnApply = createStyledButton("Re-Evaluate Mathematical Model", new Color(22, 163, 74));
        btnApply.addActionListener(e -> {
            criteria.maxSlopeDeg = sliderMaxSlope.getValue();
            criteria.maxRoughnessM = sliderMaxRough.getValue();
            criteria.minViabilityScore = sliderMinScore.getValue();
            criteria.weightSlope = sliderWSlope.getValue() / 100.0;
            criteria.weightRoughness = sliderWRough.getValue() / 100.0;
            criteria.weightElevation = sliderWElev.getValue() / 100.0;
            criteria.weightSolar = sliderWSolar.getValue() / 100.0;
            refreshAnalysis();
        });

        JButton btnReset = createStyledButton("Reset Default Engineering Weights", new Color(51, 65, 85));
        btnReset.addActionListener(e -> {
            SuitabilityCriteria def = new SuitabilityCriteria();
            sliderMaxSlope.setValue((int) Math.round(def.maxSlopeDeg));
            sliderMaxRough.setValue((int) Math.round(def.maxRoughnessM));
            sliderMinScore.setValue((int) Math.round(def.minViabilityScore));
            sliderWSlope.setValue((int) Math.round(def.weightSlope * 100));
            sliderWRough.setValue((int) Math.round(def.weightRoughness * 100));
            sliderWElev.setValue((int) Math.round(def.weightElevation * 100));
            sliderWSolar.setValue((int) Math.round(def.weightSolar * 100));
            btnApply.doClick();
        });

        btnPanel.add(btnApply);
        btnPanel.add(btnReset);

        panel.add(new JScrollPane(slidersBox), BorderLayout.CENTER);
        panel.add(btnPanel, BorderLayout.SOUTH);
        return panel;
    }

    private JSlider addSliderRow(JPanel parent, GridBagConstraints gbc, int row,
                                 String title, int min, int max, int val, String unit) {
        JPanel box = new JPanel(new BorderLayout(2, 2));
        box.setBackground(BG_CARD);
        box.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(new Color(45, 58, 78)),
                new EmptyBorder(5, 7, 5, 7)));

        JLabel lblTitle = createLabel(title, Font.PLAIN, 11, FG_TEXT);
        JLabel lblVal = createLabel(val + unit, Font.BOLD, 11, ACCENT_BLUE);
        lblVal.setHorizontalAlignment(SwingConstants.RIGHT);

        JPanel top = new JPanel(new BorderLayout());
        top.setOpaque(false);
        top.add(lblTitle, BorderLayout.WEST);
        top.add(lblVal, BorderLayout.EAST);

        JSlider slider = new JSlider(min, max, val);
        slider.setBackground(BG_CARD);
        slider.setForeground(ACCENT_BLUE);
        slider.addChangeListener(e -> lblVal.setText(slider.getValue() + unit));

        box.add(top, BorderLayout.NORTH);
        box.add(slider, BorderLayout.CENTER);

        gbc.gridy = row;
        parent.add(box, gbc);
        return slider;
    }

    private JPanel buildCenterViewportPanel() {
        JPanel center = new JPanel(new BorderLayout(6, 6));
        center.setBackground(BG_DARK);
        center.setBorder(new EmptyBorder(4, 4, 6, 4));

        centerTabs = new JTabbedPane();
        centerTabs.setBackground(BG_PANEL);
        centerTabs.setForeground(FG_TEXT);
        centerTabs.setFont(new Font("SansSerif", Font.BOLD, 12));
        centerTabs.addTab("2D Geospatial Map & Cross-Section", mapCanvas);
        centerTabs.addTab("3D Terrain & Settlement Mesh Viewer", mesh3DPanel);
        centerTabs.addTab("Earth Crater Identification & Comparison", craterComparisonPanel);

        Runnable updateTabColors = () -> {
            int sel = centerTabs.getSelectedIndex();
            Color selectedOrange = new Color(234, 88, 12);
            for (int i = 0; i < centerTabs.getTabCount(); i++) {
                centerTabs.setForegroundAt(i, i == sel ? selectedOrange : FG_TEXT);
            }
        };
        updateTabColors.run();
        centerTabs.addChangeListener(e -> updateTabColors.run());

        JPanel metricsCard = new JPanel(new BorderLayout(6, 6));
        metricsCard.setBackground(BG_PANEL);
        metricsCard.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(new Color(51, 65, 85)),
                new EmptyBorder(8, 10, 8, 10)));

        statusBannerLabel = createLabel("Mathematical Comparison vs. Default JMARS Baseline", Font.BOLD, 12, ACCENT_BLUE);
        metricsCard.add(statusBannerLabel, BorderLayout.NORTH);

        JPanel grid = new JPanel(new GridLayout(2, 4, 8, 6));
        grid.setBackground(BG_PANEL);

        lblPearson = createMetricTile(grid, "Pearson Correlation (r)");
        lblSsim = createMetricTile(grid, "Structural Similarity (SSIM)");
        lblRmse = createMetricTile(grid, "Elevation RMSE vs Base");
        lblMae = createMetricTile(grid, "Mean Abs Error (MAE)");
        lblResRatio = createMetricTile(grid, "Spatial Resolution Ratio");
        lblDetailGain = createMetricTile(grid, "High-Freq Detail Gain");
        lblGlobalScore = createMetricTile(grid, "Sector Mean Viability");
        lblAlignment = createMetricTile(grid, "Spatial Registration");

        metricsCard.add(grid, BorderLayout.CENTER);

        center.add(centerTabs, BorderLayout.CENTER);
        center.add(metricsCard, BorderLayout.SOUTH);
        return center;
    }

    private JLabel createMetricTile(JPanel parent, String title) {
        JPanel tile = new JPanel(new BorderLayout(2, 2));
        tile.setBackground(BG_CARD);
        tile.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(new Color(45, 58, 78)),
                new EmptyBorder(5, 8, 5, 8)));
        JLabel t = createLabel(title, Font.PLAIN, 10, FG_MUTED);
        JLabel v = createLabel("--", Font.BOLD, 13, FG_TEXT);
        tile.add(t, BorderLayout.NORTH);
        tile.add(v, BorderLayout.CENTER);
        parent.add(tile);
        return v;
    }

    private JPanel buildRightSitesPanel() {
        JPanel right = new JPanel(new BorderLayout(6, 8));
        right.setBackground(BG_PANEL);
        right.setPreferredSize(new Dimension(415, 600));
        right.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createMatteBorder(0, 1, 0, 0, new Color(51, 65, 85)),
                new EmptyBorder(10, 10, 10, 10)));

        rightHeaderLabel = createLabel("Recommended Settlement Zones & Viability", Font.BOLD, 13, ACCENT_BLUE);
        right.add(rightHeaderLabel, BorderLayout.NORTH);

        siteTableModel = new DefaultTableModel(
                new Object[]{"Site ID", "Zone Name", "Base", "Upload", "Slope", "Status"}, 0) {
            @Override
            public boolean isCellEditable(int row, int col) {
                return false;
            }
        };

        siteTable = new JTable(siteTableModel);
        siteTable.setBackground(BG_CARD);
        siteTable.setForeground(FG_TEXT);
        siteTable.setSelectionBackground(new Color(30, 58, 138));
        siteTable.setSelectionForeground(Color.WHITE);
        siteTable.setRowHeight(28);
        siteTable.setFont(new Font("SansSerif", Font.PLAIN, 11));
        siteTable.getTableHeader().setBackground(new Color(30, 41, 59));
        siteTable.getTableHeader().setForeground(FG_TEXT);
        siteTable.getTableHeader().setFont(new Font("SansSerif", Font.BOLD, 11));
        siteTable.setSelectionMode(ListSelectionModel.SINGLE_SELECTION);

        siteTable.getColumnModel().getColumn(5).setCellRenderer(new DefaultTableCellRenderer() {
            @Override
            public Component getTableCellRendererComponent(
                    JTable table, Object value, boolean isSelected, boolean hasFocus, int row, int column) {
                Component c = super.getTableCellRendererComponent(table, value, isSelected, hasFocus, row, column);
                String st = value != null ? value.toString() : "";
                if ("HIGH".equalsIgnoreCase(st) || "MODERATE".equalsIgnoreCase(st) || "LOW".equalsIgnoreCase(st)) {
                    c.setForeground(MapCanvasPanel.icePotentialColor(st));
                } else {
                    c.setForeground(MapCanvasPanel.statusColor(st));
                }
                setFont(new Font("SansSerif", Font.BOLD, 11));
                return c;
            }
        });

        siteTable.getSelectionModel().addListSelectionListener(e -> {
            if (!e.getValueIsAdjusting()) {
                int idx = siteTable.getSelectedRow();
                if (idx >= 0) {
                    if (currentReport != null && currentReport.isImportedMap) {
                        mapCanvas.setSelectedCraterIndex(idx);
                    } else {
                        mapCanvas.setSelectedSiteIndex(idx);
                    }
                    updateRightDetailCard(idx);
                }
            }
        });

        JScrollPane tableScroll = new JScrollPane(siteTable);
        tableScroll.setPreferredSize(new Dimension(395, 138));
        tableScroll.getViewport().setBackground(BG_CARD);

        siteDetailArea = new JTextArea();
        siteDetailArea.setEditable(false);
        siteDetailArea.setLineWrap(true);
        siteDetailArea.setWrapStyleWord(true);
        siteDetailArea.setBackground(BG_CARD);
        siteDetailArea.setForeground(FG_TEXT);
        siteDetailArea.setFont(new Font("Monospaced", Font.PLAIN, 12));
        siteDetailArea.setBorder(new EmptyBorder(10, 10, 10, 10));

        JScrollPane detailScroll = new JScrollPane(siteDetailArea);
        detailBorder = BorderFactory.createTitledBorder(
                BorderFactory.createLineBorder(new Color(51, 65, 85)),
                "Selected Zone Telemetry & Mathematical Finding");
        detailBorder.setTitleColor(ACCENT_BLUE);
        detailBorder.setTitleFont(new Font("SansSerif", Font.BOLD, 12));
        detailScroll.setBorder(detailBorder);

        JPanel centerSplit = new JPanel(new BorderLayout(6, 8));
        centerSplit.setOpaque(false);
        centerSplit.add(tableScroll, BorderLayout.NORTH);
        centerSplit.add(detailScroll, BorderLayout.CENTER);

        right.add(centerSplit, BorderLayout.CENTER);
        return right;
    }

    public void loadCustomFile(File file, String bodyOrNull) {
        if (bodyOrNull != null) {
            this.currentBody = bodyOrNull;
            bodySelector.setSelectedIndex("mars".equalsIgnoreCase(bodyOrNull) ? 0 : 1);
        }
        this.currentUploadedFile = file;
        refreshAnalysis();
    }

    public void refreshAnalysis() {
        try {
            currentReport = bridge.analyze(currentBody, currentUploadedFile, criteria);
            mapCanvas.setReport(currentReport);
            mesh3DPanel.setReport(currentReport);
            craterComparisonPanel.setReport(currentReport);

            if (currentReport.isImportedMap) {
                adaptWindowToDimensions(currentReport.uploadedWidth, currentReport.uploadedHeight);
            }

            boolean isBaselineSelf = !currentReport.isImportedMap;
            String craterBanner = (!currentReport.earthMatches.isEmpty())
                    ? String.format(Locale.US, " | Best Earth Crater Match: %s (%.1f%%)",
                    currentReport.earthMatches.get(0).name, currentReport.earthMatches.get(0).similarityScore)
                    : "";
            statusBannerLabel.setText(isBaselineSelf
                    ? "Viewing Default JMARS Baseline Map: " + currentReport.baselineFile + " (Upload .tif/.tiff/.stl/.png/.jpg to analyze)"
                    : "Imported Map [" + currentReport.uploadedFile + " • " + currentReport.uploadedWidth + "x" + currentReport.uploadedHeight + "]" + craterBanner);

            lblPearson.setText(String.format(Locale.US, "r = %.4f", currentReport.pearsonR));
            lblSsim.setText(String.format(Locale.US, "%.4f", currentReport.ssim));
            lblRmse.setText(String.format(Locale.US, "%.2f m", currentReport.rmseM));
            lblMae.setText(String.format(Locale.US, "%.2f m", currentReport.maeM));
            lblResRatio.setText(String.format(Locale.US, "%.2fx (%.1f m/px)",
                    currentReport.resolutionRatio, currentReport.uploadedResM));
            lblDetailGain.setText(String.format(Locale.US, "%.2fx HF Energy", currentReport.highFreqDetailRatio));
            lblGlobalScore.setText(String.format(Locale.US, "%.1f / 100", currentReport.globalViabilityScore));
            lblAlignment.setText(currentReport.alignmentStatus.split(" ")[0]);
            lblAlignment.setForeground(currentReport.pearsonR >= 0.70 ? new Color(34, 197, 94) : new Color(245, 158, 11));

            if (currentReport.isImportedMap) {
                rightHeaderLabel.setText("Detected Craters & Earth Ice-Analog Matches");
                detailBorder.setTitle("Selected Crater & Earth Analog Telemetry");
                siteTableModel.setColumnIdentifiers(
                        new Object[]{"Crater", "Best Earth Analog", "d/D", "Wall°", "Ice", "Potential"});
                siteTableModel.setRowCount(0);
                String topAnalog = !currentReport.earthMatches.isEmpty()
                        ? currentReport.earthMatches.get(0).craterNameShort()
                        : "Earth DB";
                for (int i = 0; i < currentReport.detectedCraters.size(); i++) {
                    DetectedCraterInfo dc = currentReport.detectedCraters.get(i);
                    siteTableModel.addRow(new Object[]{
                            dc.id,
                            i == 0 ? topAnalog : "Candidate Bowl",
                            String.format(Locale.US, "%.3f", dc.depthToDiameter),
                            String.format(Locale.US, "%.1f°", dc.wallSlopeDeg),
                            String.format(Locale.US, "%.0f", dc.icePotentialScore),
                            dc.icePotentialLabel
                    });
                }
            } else {
                rightHeaderLabel.setText("Recommended Settlement Zones & Viability");
                detailBorder.setTitle("Selected Zone Telemetry & Mathematical Finding");
                siteTableModel.setColumnIdentifiers(
                        new Object[]{"Site ID", "Zone Name", "Base", "Upload", "Slope", "Status"});
                siteTableModel.setRowCount(0);
                for (SiteEvaluation s : currentReport.sites) {
                    siteTableModel.addRow(new Object[]{
                            s.id,
                            s.name,
                            String.format(Locale.US, "%.1f", s.baselineScore),
                            String.format(Locale.US, "%.1f", s.uploadScore),
                            String.format(Locale.US, "%.2f°", s.uploadSlopeDeg),
                            s.status
                    });
                }
            }
            configureRightTableColumnWidths();

            if (siteTableModel.getRowCount() > 0) {
                siteTable.setRowSelectionInterval(0, 0);
                updateRightDetailCard(0);
            } else {
                siteDetailArea.setText("No features detected.");
            }
        } catch (Exception ex) {
            ex.printStackTrace();
            statusBannerLabel.setText("Error running Rust engine: " + ex.getMessage());
        }
    }

    private void configureRightTableColumnWidths() {
        if (siteTable.getColumnModel().getColumnCount() < 6) return;
        siteTable.getColumnModel().getColumn(0).setPreferredWidth(48);
        siteTable.getColumnModel().getColumn(1).setPreferredWidth(135);
        siteTable.getColumnModel().getColumn(2).setPreferredWidth(46);
        siteTable.getColumnModel().getColumn(3).setPreferredWidth(50);
        siteTable.getColumnModel().getColumn(4).setPreferredWidth(48);
        siteTable.getColumnModel().getColumn(5).setPreferredWidth(115);

        siteTable.getColumnModel().getColumn(5).setCellRenderer(new DefaultTableCellRenderer() {
            @Override
            public Component getTableCellRendererComponent(
                    JTable table, Object value, boolean isSelected, boolean hasFocus, int row, int column) {
                Component c = super.getTableCellRendererComponent(table, value, isSelected, hasFocus, row, column);
                String st = value != null ? value.toString() : "";
                if ("HIGH".equalsIgnoreCase(st) || "MODERATE".equalsIgnoreCase(st) || "LOW".equalsIgnoreCase(st)) {
                    c.setForeground(MapCanvasPanel.icePotentialColor(st));
                } else {
                    c.setForeground(MapCanvasPanel.statusColor(st));
                }
                setFont(new Font("SansSerif", Font.BOLD, 11));
                return c;
            }
        });
    }

    private void adaptWindowToDimensions(int mapW, int mapH) {
        if (GraphicsEnvironment.isHeadless() || mapW <= 0 || mapH <= 0) {
            return;
        }
        try {
            Dimension screen = Toolkit.getDefaultToolkit().getScreenSize();
            int maxFrameW = (int) Math.round(screen.width * 0.94);
            int maxFrameH = (int) Math.round(screen.height * 0.92);

            double aspect = mapW / (double) mapH;
            int sidePanelsW = 735;
            int chromeH = 315;

            int availCenterW = Math.max(460, maxFrameW - sidePanelsW);
            int availCenterH = Math.max(360, maxFrameH - chromeH);

            int targetCenterW;
            int targetCenterH;
            if (aspect >= 1.0) {
                targetCenterW = Math.min(availCenterW, Math.max(660, Math.min(mapW, availCenterW)));
                targetCenterH = (int) Math.round(targetCenterW / aspect);
                if (targetCenterH > availCenterH) {
                    targetCenterH = availCenterH;
                    targetCenterW = (int) Math.round(targetCenterH * aspect);
                }
            } else {
                targetCenterH = Math.min(availCenterH, Math.max(540, Math.min(mapH, availCenterH)));
                targetCenterW = (int) Math.round(targetCenterH * aspect);
                if (targetCenterW > availCenterW) {
                    targetCenterW = availCenterW;
                    targetCenterH = (int) Math.round(targetCenterW / aspect);
                }
            }

            int newW = Math.max(1180, Math.min(maxFrameW, targetCenterW + sidePanelsW));
            int newH = Math.max(740, Math.min(maxFrameH, targetCenterH + chromeH));

            setSize(newW, newH);
            validate();
            repaint();
        } catch (Exception ignored) {
        }
    }

    private void updateRightDetailCard(int idx) {
        if (currentReport == null) return;
        if (currentReport.isImportedMap) {
            updateCraterDetailCard(idx);
        } else {
            updateSiteDetailCard(idx);
        }
    }

    private void updateCraterDetailCard(int idx) {
        if (idx < 0 || idx >= currentReport.detectedCraters.size()) return;
        DetectedCraterInfo dc = currentReport.detectedCraters.get(idx);
        StringBuilder sb = new StringBuilder();
        sb.append(String.format(Locale.US, "[%s] DETECTED CRATER TELEMETRY\n", dc.id));
        sb.append("====================================================\n");
        sb.append(String.format(Locale.US, "ICE POTENTIAL   : %s (%.1f / 100)\n",
                dc.icePotentialLabel, dc.icePotentialScore));
        sb.append(String.format(Locale.US, "CONFIDENCE      : %.1f%%  |  Diameter: %.1f px\n",
                dc.confidence, dc.diameterPx));
        sb.append(String.format(Locale.US, "CENTER (NORM)   : (%.3f, %.3f)  Radius: %.3f\n",
                dc.normX, dc.normY, dc.normRadius));
        sb.append("----------------------------------------------------\n");
        sb.append(String.format(Locale.US, "DEPTH / DIAM    : %.4f  (Depth: %.2f | Rim: %.2f)\n",
                dc.depthToDiameter, dc.depthVal, dc.rimHeightVal));
        sb.append(String.format(Locale.US, "RIM / DEPTH     : %.3f   |  CIRCULARITY: %.3f\n",
                dc.rimToDepth, dc.circularity));
        sb.append(String.format(Locale.US, "FLOOR FLATNESS  : %.3f   |  CENTRAL PEAK: %.3f\n",
                dc.floorFlatness, dc.centralPeak));
        sb.append(String.format(Locale.US, "INNER WALL SLOPE: %.2f°\n", dc.wallSlopeDeg));
        sb.append("----------------------------------------------------\n");
        sb.append("ICE DEPOSIT ASSESSMENT:\n  ").append(dc.iceRationale).append("\n\n");

        if (!currentReport.earthMatches.isEmpty()) {
            sb.append("TOP EARTH CRATER ANALOG MATCHES:\n");
            for (int i = 0; i < currentReport.earthMatches.size(); i++) {
                EarthCraterMatchInfo em = currentReport.earthMatches.get(i);
                sb.append(String.format(Locale.US, "  #%d %s (%s, %s)\n",
                        i + 1, em.name, em.location, em.country));
                sb.append(String.format(Locale.US, "     Match: %.1f%% | r=%.3f | %s\n",
                        em.similarityScore, em.profileCorrelation, em.morphology));
            }
            sb.append("\nOpen the 'Earth Crater Identification & Comparison' tab\n");
            sb.append("at the top to visually compare maps & profiles.\n");
        }
        siteDetailArea.setText(sb.toString());
        siteDetailArea.setCaretPosition(0);
    }

    private void updateSiteDetailCard(int idx) {
        if (currentReport == null || idx < 0 || idx >= currentReport.sites.size()) return;
        SiteEvaluation s = currentReport.sites.get(idx);
        StringBuilder sb = new StringBuilder();
        sb.append(String.format(Locale.US, "[%s] %s\n", s.id, s.name));
        sb.append("====================================================\n");
        sb.append(String.format(Locale.US, "STATUS          : %s\n", s.status));
        sb.append(String.format(Locale.US, "COORDINATES     : Lat %.4f°, Lon %.4f° (Radius %.1f km)\n",
                s.latDeg, s.lonDeg, s.radiusKm));
        sb.append(String.format(Locale.US, "VIABILITY SCORE : %.2f / 100 (Baseline: %.2f, Delta: %+.2f)\n",
                s.uploadScore, s.baselineScore, s.scoreDelta));
        sb.append(String.format(Locale.US, "SAFE PAD AREA   : %.1f%% of zone meets slope & roughness cutoffs\n",
                s.safeAreaFraction * 100.0));
        sb.append("----------------------------------------------------\n");
        sb.append(String.format(Locale.US, "MEAN SLOPE      : %.2f° (Baseline: %.2f° | Max Peak: %.2f°)\n",
                s.uploadSlopeDeg, s.baseSlopeDeg, s.maxSlopeDeg));
        sb.append(String.format(Locale.US, "ROUGHNESS (TRI) : %.2f m (Baseline: %.2f m)\n",
                s.uploadRoughnessM, s.baseRoughnessM));
        sb.append(String.format(Locale.US, "MEAN ELEVATION  : %.1f m  |  SOLAR SCORE: %.1f / 100\n",
                s.meanElevationM, s.solarScore));
        sb.append(String.format(Locale.US, "LOCAL ELEV RMSE : %.2f m vs Baseline\n", s.localRmseM));
        sb.append("----------------------------------------------------\n");
        sb.append("MATHEMATICAL FINDING:\n  ").append(s.finding).append("\n\n");
        sb.append("SITE ENGINEERING RATIONALE:\n  ").append(s.rationale).append("\n\n");
        sb.append("PRIMARY ISRU RESOURCES:\n  ").append(s.resources).append("\n");

        siteDetailArea.setText(sb.toString());
        siteDetailArea.setCaretPosition(0);
    }

    private void openUploadChooser() {
        JFileChooser chooser = new JFileChooser(bridge.getSampleUploadsDir());
        chooser.setDialogTitle("Select Planetary Map, 3D STL Mesh, or Image (.tif, .tiff, .stl, .png, .jpg)");
        chooser.setFileFilter(new FileNameExtensionFilter(
                "Planetary Maps, STLs & Images (*.tif, *.tiff, *.stl, *.png, *.jpg, *.jpeg)",
                "tif", "tiff", "stl", "png", "jpg", "jpeg"));
        int res = chooser.showOpenDialog(this);
        if (res == JFileChooser.APPROVE_OPTION && chooser.getSelectedFile() != null) {
            currentUploadedFile = chooser.getSelectedFile();
            refreshAnalysis();
        }
    }

    public BufferedImage renderOffscreenSnapshot() {
        setSize(1440, 880);
        doLayout();
        validate();
        BufferedImage img = new BufferedImage(getWidth(), getHeight(), BufferedImage.TYPE_INT_RGB);
        paintAll(img.createGraphics());
        return img;
    }

    private static JLabel createLabel(String text, int style, int size, Color color) {
        JLabel l = new JLabel(text);
        l.setFont(new Font("SansSerif", style, size));
        l.setForeground(color);
        return l;
    }

    private static JButton createStyledButton(String text, Color bg) {
        JButton b = new JButton(text);
        b.setBackground(bg);
        b.setForeground(Color.WHITE);
        b.setFocusPainted(false);
        b.setFont(new Font("SansSerif", Font.BOLD, 11));
        b.setBorder(BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(new Color(80, 105, 140)),
                new EmptyBorder(5, 10, 5, 10)));
        return b;
    }
}
