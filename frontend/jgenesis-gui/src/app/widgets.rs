use crate::app::RESERVED_HELP_TEXT_HEIGHT;
use egui::scroll_area::ScrollAreaOutput;
use egui::style::ScrollStyle;
use egui::{
    Color32, Context, Response, ScrollArea, Slider, TextEdit, Ui, Widget, WidgetText, Window,
};
use jgenesis_native_config::common::ConfigSavePath;
use jgenesis_native_driver::extensions::Console;
use rfd::FileDialog;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::str::FromStr;

pub struct NumericTextEdit<'a, T> {
    text: &'a mut String,
    value: &'a mut T,
    invalid: &'a mut bool,
    validation_fn: Box<dyn Fn(T) -> bool>,
    desired_width: Option<f32>,
}

impl<'a, T> NumericTextEdit<'a, T> {
    pub fn new(text: &'a mut String, value: &'a mut T, invalid: &'a mut bool) -> Self {
        Self { text, value, invalid, validation_fn: Box::new(|_| true), desired_width: None }
    }

    pub fn with_validation(mut self, validation_fn: impl Fn(T) -> bool + 'static) -> Self {
        self.validation_fn = Box::new(validation_fn);
        self
    }

    pub fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = Some(desired_width);
        self
    }
}

impl<T: Copy + FromStr> Widget for NumericTextEdit<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut text_edit = TextEdit::singleline(self.text);
        if let Some(desired_width) = self.desired_width {
            text_edit = text_edit.desired_width(desired_width);
        }

        let response = text_edit.ui(ui);
        if response.changed() {
            match self.text.parse::<T>() {
                Ok(value) if (self.validation_fn)(value) => {
                    *self.value = value;
                    *self.invalid = false;
                }
                _ => {
                    *self.invalid = true;
                }
            }
        }

        response
    }
}

pub struct OptionalPathSelector<'a> {
    label: &'static str,
    path: &'a mut Option<PathBuf>,
    pick_bios_path: fn() -> Option<PathBuf>,
}

impl<'a> OptionalPathSelector<'a> {
    pub fn new(
        label: &'static str,
        path: &'a mut Option<PathBuf>,
        pick_bios_path: fn() -> Option<PathBuf>,
    ) -> Self {
        Self { label, path, pick_bios_path }
    }
}

impl Widget for OptionalPathSelector<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.label(self.label);

            let button_label = match self.path {
                Some(path) => path.to_string_lossy(),
                None => "<None>".into(),
            };
            if ui.button(button_label).clicked()
                && let Some(path) = (self.pick_bios_path)()
            {
                *self.path = Some(path);
            }
        })
        .response
    }
}

pub fn render_vertical_scroll_area<R>(
    ui: &mut Ui,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> ScrollAreaOutput<R> {
    let screen_height = ui.input(|i| i.content_rect().height());

    let mut scroll_area = ScrollArea::vertical().auto_shrink([false, true]);

    let max_scroll_height = screen_height - RESERVED_HELP_TEXT_HEIGHT - 75.0;
    if max_scroll_height >= 100.0 {
        scroll_area = scroll_area.max_height(max_scroll_height);
    }

    ui.scope(|ui| {
        ui.style_mut().spacing.scroll = ScrollStyle::solid();
        scroll_area.show(ui, add_contents)
    })
    .inner
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderErrorEffect {
    None,
    LaunchEmulator(Console),
}

pub struct BiosErrorStrings<S1: Into<WidgetText>, S2: Into<WidgetText>, S3: Into<WidgetText>> {
    pub title: S1,
    pub text: S2,
    pub button_label: S3,
}

pub fn render_bios_error<S1, S2, S3>(
    ctx: &Context,
    open: &mut bool,
    BiosErrorStrings { title, text, button_label }: BiosErrorStrings<S1, S2, S3>,
    path: &mut Option<PathBuf>,
    console: Console,
    pick_path: fn() -> Option<PathBuf>,
) -> RenderErrorEffect
where
    S1: Into<WidgetText>,
    S2: Into<WidgetText>,
    S3: Into<WidgetText>,
{
    let mut path_configured = false;
    Window::new(title).open(open).resizable(false).show(ctx, |ui| {
        ui.label(text);

        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.label("Configure now:");
            if ui.button(button_label).clicked()
                && let Some(bios_path) = pick_path()
            {
                *path = Some(bios_path);
                path_configured = true;
            }
        });
    });

    if path_configured {
        *open = false;
        RenderErrorEffect::LaunchEmulator(console)
    } else {
        RenderErrorEffect::None
    }
}

pub fn render_custom_path_select(ui: &mut Ui, custom_path: &mut PathBuf) {
    ui.horizontal(|ui| {
        ui.label("Custom path:");

        let button_label = custom_path.to_string_lossy();
        if ui.button(button_label).clicked()
            && let Some(path) = FileDialog::new().pick_folder()
        {
            *custom_path = path;
        }
    });
}

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
                render_custom_path_select(ui, self.custom_path);
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

#[derive(Debug, Clone)]
pub struct VolumeAdjustmentState {
    pub text: String,
    pub invalid: bool,
}

impl Default for VolumeAdjustmentState {
    fn default() -> Self {
        Self { text: "0.0".into(), invalid: false }
    }
}

impl VolumeAdjustmentState {
    pub fn from_config_value(value: f64) -> Self {
        let text = format!("{value:.1}");

        Self { text, invalid: false }
    }
}

pub struct VolumeAdjustmentWidget<'a> {
    pub label: &'a str,
    pub config_value: &'a mut f64,
    pub state: &'a mut VolumeAdjustmentState,
}

impl<'a> VolumeAdjustmentWidget<'a> {
    pub fn new(
        label: &'a str,
        config_value: &'a mut f64,
        state: &'a mut VolumeAdjustmentState,
    ) -> Self {
        Self { label, config_value, state }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(
                NumericTextEdit::new(
                    &mut self.state.text,
                    self.config_value,
                    &mut self.state.invalid,
                )
                .desired_width(40.0)
                .with_validation(f64::is_finite),
            );

            ui.label(self.label);
        });
    }
}

pub fn render_volume_adjustments<const VALUES: usize>(
    mut values: [VolumeAdjustmentWidget<'_>; VALUES],
    ui: &mut Ui,
) -> Response {
    ui.group(|ui| {
        ui.label("Volume adjustments (dB) (+/-)");
        ui.add_space(2.0);

        for value in &mut values {
            value.ui(ui);
        }

        if ui.button("Clear all").clicked() {
            for value in &mut values {
                *value.config_value = 0.0;
                *value.state = VolumeAdjustmentState::default();
            }
        }

        let any_invalid = values.iter().any(|value| value.state.invalid);
        if any_invalid {
            ui.colored_label(Color32::RED, "Values must be numbers");
        }
    })
    .response
}
