//! The launcher's window: which build it is updating from and to, how far the
//! download is, and every file it downloads.

use crate::progress::{Choice, FileState, Progress, State, mb};
use eframe::egui::{self, Align, Color32, Layout, Margin, RichText};

const ACCENT: Color32 = Color32::from_rgb(236, 168, 64);
const WARNING: Color32 = Color32::from_rgb(242, 196, 90);
const ERROR: Color32 = Color32::from_rgb(240, 108, 96);
const BACKGROUND: Color32 = Color32::from_rgb(24, 22, 20);
const LIST_BACKGROUND: Color32 = Color32::from_rgb(34, 31, 28);

/// Opens the window and runs `work` on its own thread, which reports to the
/// window through the `Progress` it is given. Returns once the window has
/// closed, or an error without calling `work` if the window could not open.
pub fn run(work: impl FnOnce(Progress) + Send + 'static, focus: bool) -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("ShitShooter")
        .with_app_id(crate::APP_ID)
        .with_inner_size([540.0, 420.0])
        .with_min_inner_size([420.0, 320.0])
        .with_active(focus);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(crate::ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native(
        "ShitShooter",
        options,
        Box::new(move |cc| {
            style(&cc.egui_ctx);
            let progress = Progress::window(cc.egui_ctx.clone());
            let worker = progress.clone();
            std::thread::spawn(move || work(worker));
            Ok(Box::new(Window { progress, scrolled_to: None }))
        }),
    )
}

fn style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|style| {
        style.visuals.panel_fill = BACKGROUND;
        style.visuals.window_fill = BACKGROUND;
        style.visuals.selection.bg_fill = ACCENT;
        style.visuals.hyperlink_color = ACCENT;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(14.0, 6.0);
    });
}

struct Window {
    progress: Progress,
    /// The file the list last scrolled to. It scrolls again only when the
    /// download moves on, so it can still be scrolled by hand.
    scrolled_to: Option<usize>,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let progress = self.progress.clone();
        let mut answer = None;
        let scrolled_to = &mut self.scrolled_to;
        {
            let state = progress.state();
            if let Some(question) = &state.question {
                egui::Panel::bottom("buttons")
                    .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(Margin::symmetric(20, 14)))
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            for &choice in question.choices.iter().rev() {
                                let text = RichText::new(choice.label()).size(15.0);
                                let button = if choice == Choice::TryAgain {
                                    egui::Button::new(text.color(Color32::BLACK)).fill(ACCENT)
                                } else {
                                    egui::Button::new(text)
                                };
                                if ui.add(button).clicked() {
                                    answer = Some(choice);
                                }
                            }
                        });
                    });
            }
            egui::CentralPanel::default_margins()
                .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(Margin::same(20)))
                .show(ui, |ui| contents(ui, &state, scrolled_to));
        }
        if let Some(choice) = answer {
            self.progress.answer(choice);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(BACKGROUND).to_array()
    }
}

fn contents(ui: &mut egui::Ui, state: &State, scrolled_to: &mut Option<usize>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("ShitShooter").size(28.0).strong().color(Color32::WHITE));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("launcher {}", crate::VERSION)).weak());
        });
    });
    ui.add_space(10.0);

    match &state.builds {
        Some((Some(from), to)) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(RichText::new(from).size(20.0).weak());
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 20.0), egui::Sense::hover());
                let stroke = egui::Stroke::new(2.0, ui.visuals().weak_text_color());
                ui.painter().arrow(rect.left_center(), egui::vec2(rect.width(), 0.0), stroke);
                ui.label(RichText::new(to).size(20.0).strong().color(ACCENT));
            });
        }
        Some((None, to)) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Installing").size(20.0));
                ui.label(RichText::new(to).size(20.0).strong().color(ACCENT));
            });
        }
        None => {}
    }
    if !state.headline.is_empty() {
        ui.label(RichText::new(&state.headline).size(15.0));
    }

    if !state.files.is_empty() {
        ui.add_space(8.0);
        let (done, total) = state.totals();
        let finished = state.files.iter().filter(|file| file.state == FileState::Done).count();
        let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
        let number = state.current().map_or(finished, |(index, _)| index + 1);
        let text = format!("{} / {}  ·  file {} of {}", size(done), size(total), number, state.files.len());
        ui.add(
            egui::ProgressBar::new(fraction)
                .text(RichText::new(text).color(Color32::WHITE))
                .fill(ACCENT.gamma_multiply(0.75))
                .desired_height(22.0)
                .corner_radius(4),
        );
        if let Some((_, file)) = state.current() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Downloading").weak());
                ui.label(&file.name);
            });
        }
        ui.add_space(4.0);
        file_list(ui, state, scrolled_to);
    }

    if let Some(warning) = &state.warning {
        ui.add_space(8.0);
        ui.label(RichText::new(warning).color(WARNING));
    }
    if let Some(question) = &state.question {
        ui.add_space(8.0);
        ui.label(RichText::new(&question.message).color(ERROR));
    }
}

/// Every file of the download, with a mark for done, downloading or waiting.
fn file_list(ui: &mut egui::Ui, state: &State, scrolled_to: &mut Option<usize>) {
    let reserve = if state.warning.is_some() || state.question.is_some() { 70.0 } else { 0.0 };
    let height = (ui.available_height() - reserve).max(60.0);
    egui::Frame::new()
        .fill(LIST_BACKGROUND)
        .corner_radius(6)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(height).auto_shrink([false, true]).show(ui, |ui| {
                for (index, file) in state.files.iter().enumerate() {
                    let (mark, color) = match file.state {
                        FileState::Done => ("✔", Color32::from_rgb(120, 200, 120)),
                        FileState::Downloading => ("⏵", ACCENT),
                        FileState::Waiting => ("·", ui.visuals().weak_text_color()),
                    };
                    let bytes = match (file.state, file.size) {
                        (FileState::Downloading, Some(total)) => format!("{} / {}", size(file.done), size(total)),
                        (FileState::Downloading, None) => size(file.done),
                        (_, Some(total)) => size(total),
                        (_, None) => String::new(),
                    };
                    let row = ui.horizontal(|ui| {
                        // A fixed width, so the names line up whatever the mark.
                        ui.add_sized([14.0, 18.0], egui::Label::new(RichText::new(mark).color(color)));
                        let name = RichText::new(&file.name);
                        ui.label(if file.state == FileState::Waiting { name.weak() } else { name });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(bytes).weak().monospace());
                        });
                    });
                    if file.state == FileState::Downloading && *scrolled_to != Some(index) {
                        row.response.scroll_to_me(Some(Align::Center));
                        *scrolled_to = Some(index);
                    }
                }
            });
        });
}

/// A size in KB below a megabyte, so small files do not all show as 0.0 MB.
fn size(bytes: u64) -> String {
    if bytes < 1_048_576 { format!("{} KB", bytes.div_ceil(1024)) } else { format!("{:.1} MB", mb(bytes)) }
}
