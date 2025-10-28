use egui::{Response, Slider, Ui, Widget};
use jgenesis_native_config::common::ConfigSavePath;
use rfd::FileDialog;
use std::ops::RangeInclusive;
use std::path::PathBuf;

pub struct SavePathSelect<'a> {
    label: &'a str,
    save_path: &'a mut ConfigSavePath,
    custom_path: &'a mut PathBuf,
}

impl<'a> SavePathSelect<'a> {
    pub fn new(
        label: &'a str,
        save_path: &'a mut ConfigSavePath,
        custom_path: &'a mut PathBuf,
    ) -> Self {
        Self { label, save_path, custom_path }
    }
}

impl Widget for SavePathSelect<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.group(|ui| {
            ui.label(self.label);

            ui.horizontal(|ui| {
                ui.radio_value(self.save_path, ConfigSavePath::RomFolder, "Same folder as ROM");
                ui.radio_value(self.save_path, ConfigSavePath::EmulatorFolder, "Emulator folder");
                ui.radio_value(self.save_path, ConfigSavePath::Custom, "Custom");
            });

            ui.add_enabled_ui(*self.save_path == ConfigSavePath::Custom, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Custom path:");

                    let button_label = self.custom_path.to_string_lossy();
                    if ui.button(button_label).clicked()
                        && let Some(path) = FileDialog::new().pick_folder()
                    {
                        *self.custom_path = path;
                    }
                });
            });
        })
        .response
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockModifier {
    Divider,
    Multiplier,
}

pub struct OverclockSlider<'a, Num> {
    pub label: &'a str,
    pub current_value: &'a mut Num,
    pub range: RangeInclusive<Num>,
    pub master_clock: f64,
    pub default_divider: f64,
    pub modifier: ClockModifier,
}

impl<Num: emath::Numeric> Widget for OverclockSlider<'_, Num> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.group(|ui| {
            ui.label(self.label);

            ui.add(Slider::new(self.current_value, self.range));

            let current_divider = self.current_value.to_f64();

            let (effective_speed_ratio, effective_speed_mhz) = match self.modifier {
                ClockModifier::Divider => {
                    let effective_speed_ratio = 100.0 * self.default_divider / current_divider;
                    let effective_speed_mhz = self.master_clock / current_divider / 1_000_000.0;
                    (effective_speed_ratio, effective_speed_mhz)
                }
                ClockModifier::Multiplier => {
                    let effective_speed_ratio = 100.0 * current_divider / self.default_divider;
                    let effective_speed_mhz = self.master_clock * current_divider / 1_000_000.0;
                    (effective_speed_ratio, effective_speed_mhz)
                }
            };

            ui.label(format!(
                "Effective speed: {effective_speed_mhz:.2} MHz ({}%)",
                effective_speed_ratio.round()
            ));
        })
        .response
    }
}
