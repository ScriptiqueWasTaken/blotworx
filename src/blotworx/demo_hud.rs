//! Functional demo HUD. Keeps gameplay usable while the original Flash movie
//! runtime's repeated-damage crash is investigated; not original presentation.
#[derive(Default)]
pub struct State {
    ammo_visible: bool,
    pub unhandled: std::collections::BTreeSet<String>,
}
impl State {
    pub fn draw(&mut self, ui: &egui::Ui, rect: egui::Rect, world: &mut super::assets::World) {
        for (method, kind, target) in world.ability_hud_requests.drain(..) {
            match (method, kind, target) {
                ("Show", "HudElement", "AmmoBars") => self.ammo_visible = true,
                ("Hide", "HudElement", "AmmoBars") => self.ammo_visible = false,
                _ => {
                    self.unhandled.insert(format!("{method}({kind}, {target})"));
                }
            }
        }
        if world.demo_story.enabled && !world.demo_story.hud {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let scale = (rect.height() / 700.).clamp(0.65, 1.4);
        if world.countdown.visible {
            painter.text(
                egui::pos2(rect.center().x,rect.top()+18.*scale),
                egui::Align2::CENTER_TOP,
                &world.countdown.display,
                egui::FontId::monospace(28.*scale),
                egui::Color32::WHITE,
            );
        }
        let size = egui::vec2(180., 58.) * scale;
        let mut meter =
            |position: egui::Pos2, title: &str, value: f32, maximum: f32, color: egui::Color32| {
                let panel = egui::Rect::from_min_size(position, size);
                painter.rect_filled(panel, 8., egui::Color32::from_black_alpha(160));
                painter.text(
                    panel.min + egui::vec2(12., 9.) * scale,
                    egui::Align2::LEFT_TOP,
                    format!("{title}  {:.0} / {:.0}", value.max(0.), maximum),
                    egui::FontId::proportional(15. * scale),
                    egui::Color32::WHITE,
                );
                let bar = egui::Rect::from_min_size(
                    panel.min + egui::vec2(12., 34.) * scale,
                    egui::vec2(156., 11.) * scale,
                );
                painter.rect_filled(bar, 4., egui::Color32::from_white_alpha(45));
                let fraction = if maximum > 0. {
                    (value / maximum).clamp(0., 1.)
                } else {
                    0.
                };
                if fraction > 0. {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            bar.min,
                            egui::vec2(bar.width() * fraction, bar.height()),
                        ),
                        4.,
                        color,
                    );
                }
            };
        let inset = egui::vec2(20., 20.) * scale;
        if let Some(health) = world.runtime_health.get(&world.player_entity) {
            let maximum = world
                .authored_health
                .get(&world.player_entity)
                .map_or(health.health, |v| v.maximum);
            meter(
                rect.min + inset,
                "HEALTH",
                health.health as f32,
                maximum as f32,
                egui::Color32::from_rgb(244, 103, 85),
            );
        }
        if self.ammo_visible && world.input_presentation.mode_2d==0 {
            if let Some(ammo) = &world.ammo.inventory {
                meter(
                    egui::pos2(rect.left() + inset.x, rect.bottom() - inset.y - size.y),
                    "PAINT",
                    ammo.amounts[0],
                    ammo.maxima[0].unwrap_or(ammo.amounts[0]),
                    egui::Color32::from_rgb(65, 160, 245),
                );
                meter(
                    egui::pos2(
                        rect.right() - inset.x - size.x,
                        rect.bottom() - inset.y - size.y,
                    ),
                    "THINNER",
                    ammo.amounts[1],
                    ammo.maxima[1].unwrap_or(ammo.amounts[1]),
                    egui::Color32::from_rgb(135, 218, 50),
                );
            }
        }
    }
}
