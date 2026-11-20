//! User preferences. Profiles change presentation only, never game assets.
use crate::render_support::scene_finish::Settings;
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub enum Profile {
    #[default]
    Classic,
    ClassicPlus,
    Remix,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub profile: Profile,
    pub grades: [Settings; 3],
    pub surfaces: [crate::render_support::surface_lighting::Settings; 3],
    pub hud: bool,
    pub sensitivity: f32,
    pub invert_y: bool,
    pub distance: f32,
    pub fov: f32,
    pub fullscreen: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            profile: Profile::Classic,
            surfaces: crate::render_support::surface_lighting::Settings::presets(),
            grades: [
                Settings::default(),
                Settings {
                    contrast: 1.03,
                    saturation: 1.04,
                    bloom: 0.12,
                    smoothing: 0.2,
                    sharpen: 0.15,
                    ..Default::default()
                },
                Settings {
                    exposure: 0.12,
                    contrast: 1.12,
                    saturation: 1.12,
                    bloom: 0.55,
                    threshold: 0.65,
                    vignette: 0.16,
                    smoothing: 0.3,
                    rolloff: 0.15,
                    warmth: 0.025,
                    radius: 5.,
                    sharpen: 0.25,
                    ..Default::default()
                },
            ],
            hud: true,
            sensitivity: 1.,
            invert_y: false,
            distance: 5.,
            fov: 60.,
            fullscreen: false,
        }
    }
}
impl Preferences {
    pub fn index(&self) -> usize {
        match self.profile {
            Profile::Classic => 0,
            Profile::ClassicPlus => 1,
            Profile::Remix => 2,
        }
    }
    pub fn surface(&self) -> crate::render_support::surface_lighting::Settings {
        if self.profile == Profile::Classic {
            Default::default()
        } else {
            let mut s = self.surfaces[self.index()];
            s.mode = self.index() as u32;
            s
        }
    }
    pub fn grade(&self) -> Settings {
        self.grades[self.index()]
    }
    fn path() -> std::path::PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("BlueThinner")
            .join("blotworx-options.json")
    }
    pub fn load() -> Self {
        let mut s: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        s.sanitize();
        s
    }
    pub fn sanitize(&mut self) {
        fn clamp(x: &mut f32, a: f32, b: f32, d: f32) {
            *x = if x.is_finite() { x.clamp(a, b) } else { d }
        }
        clamp(&mut self.sensitivity, 0.2, 3., 1.);
        clamp(&mut self.distance, 2., 16., 5.);
        clamp(&mut self.fov, 40., 100., 60.);
        for s in &mut self.surfaces {
            s.sanitize();
        }
        for s in &mut self.grades {
            clamp(&mut s.exposure, -2., 2., 0.);
            clamp(&mut s.contrast, 0.5, 1.5, 1.);
            clamp(&mut s.saturation, 0., 2., 1.);
            clamp(&mut s.gamma, 0.6, 1.6, 1.);
            clamp(&mut s.bloom, 0., 1.5, 0.);
            clamp(&mut s.threshold, 0.3, 1., 0.75);
            clamp(&mut s.vignette, 0., 0.6, 0.);
            clamp(&mut s.sharpen, 0., 1., 0.);
            clamp(&mut s.radius, 1., 12., 3.);
            clamp(&mut s.warmth, -0.3, 0.3, 0.);
            clamp(&mut s.rolloff, 0., 0.5, 0.);
            clamp(&mut s.smoothing, 0., 1., 0.);
        }
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let p = Self::path();
        std::fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, p)?;
        Ok(())
    }
    pub fn graphics(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (p, n) in [
                (Profile::Classic, "Classic"),
                (Profile::ClassicPlus, "Classic+"),
                (Profile::Remix, "Remix"),
            ] {
                ui.selectable_value(&mut self.profile, p, n);
            }
        });
        ui.add_space(8.);
        ui.label(match self.profile {
            Profile::Classic => "Original materials, lighting and color.",
            Profile::ClassicPlus => {
                "Authored color with soft surface lighting and restrained reflections."
            }
            Profile::Remix => "Per-pixel lighting, material highlights and geometry shadows.",
        });
        ui.add_space(12.);
        let i = self.index();
        if i != 0 {
            egui::CollapsingHeader::new("Lighting & materials")
                .default_open(true)
                .show(ui, |ui| self.surfaces[i].ui(ui));
        }
        egui::CollapsingHeader::new("Color & effects")
            .default_open(i == 0)
            .show(ui, |ui| {
                let s = &mut self.grades[i];
                ui.columns(2, |columns| {
                    columns[0].strong("Color");
                    egui::Grid::new("graphics_color")
                        .num_columns(2)
                        .spacing([18., 10.])
                        .show(&mut columns[0], |ui| {
                            for (n, v, r) in [
                                ("Exposure", &mut s.exposure, -2.0..=2.0),
                                ("Contrast", &mut s.contrast, 0.5..=1.5),
                                ("Saturation", &mut s.saturation, 0.0..=2.0),
                                ("Gamma", &mut s.gamma, 0.6..=1.6),
                                ("Temperature", &mut s.warmth, -0.3..=0.3),
                                ("Highlight rolloff", &mut s.rolloff, 0.0..=0.5),
                            ] {
                                ui.label(n);
                                ui.add(egui::Slider::new(v, r));
                                ui.end_row();
                            }
                        });
                    columns[1].strong("Effects");
                    egui::Grid::new("graphics_effects")
                        .num_columns(2)
                        .spacing([18., 10.])
                        .show(&mut columns[1], |ui| {
                            for (n, v, r) in [
                                ("Bloom", &mut s.bloom, 0.0..=1.5),
                                ("Bloom threshold", &mut s.threshold, 0.3..=1.0),
                                ("Bloom radius", &mut s.radius, 1.0..=12.0),
                                ("Vignette", &mut s.vignette, 0.0..=0.6),
                                ("Sharpening", &mut s.sharpen, 0.0..=1.0),
                                ("Edge smoothing", &mut s.smoothing, 0.0..=1.0),
                            ] {
                                ui.label(n);
                                ui.add(egui::Slider::new(v, r));
                                ui.end_row();
                            }
                        });
                });
                ui.add_space(12.);
            });
        if ui.button("Reset this profile").clicked() {
            self.grades[i] = Self::default().grades[i];
            self.surfaces[i] = Self::default().surfaces[i];
        }
    }
}
