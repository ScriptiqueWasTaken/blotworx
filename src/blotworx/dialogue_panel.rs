//! Shared functional dialogue panel; native game UI presentation remains separate.
pub(super) fn draw(
    ui: &egui::Ui,
    speaker: &str,
    text: &str,
    choices: &[String],
    selected: &mut usize,
    pad_edges: u16,
) -> Option<usize> {
    let mut answer = None;
    *selected = (*selected).min(choices.len().saturating_sub(1));
    if !choices.is_empty() {
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) || pad_edges & 2 != 0 {
            *selected = (*selected + 1) % choices.len();
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) || pad_edges & 1 != 0 {
            *selected = (*selected + choices.len() - 1) % choices.len();
        }
    }
    egui::Window::new("Conversation")
        .anchor(egui::Align2::CENTER_BOTTOM, [0., -45.])
        .collapsible(false)
        .resizable(false)
        .default_width(620.)
        .show(ui.ctx(), |ui| {
            ui.strong(speaker);
            ui.label(text);
            ui.add_space(8.);
            if choices.is_empty() {
                if ui.button("Continue [Enter / A]").clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    || pad_edges & 0x1000 != 0
                {
                    answer = Some(0);
                }
            } else {
                for (index, text) in choices.iter().enumerate() {
                    if ui.selectable_label(*selected == index, text).clicked() {
                        answer = Some(index);
                    }
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) || pad_edges & 0x1000 != 0 {
                    answer = Some(*selected);
                }
            }
        });
    answer
}
