use crate::app::RESERVED_HELP_TEXT_HEIGHT;
use egui::scroll_area::ScrollAreaOutput;
use egui::style::ScrollStyle;
use egui::{Context, Response, ScrollArea, TextEdit, Ui, Widget, WidgetText, Window};
use jgenesis_native_driver::extensions::Console;
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
    let screen_height = ui.input(|i| i.screen_rect.height());

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
