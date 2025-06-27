mod helptext;

use crate::app::widgets::NumericTextEdit;
use crate::app::{App, OpenWindow, widgets};
use crate::emuthread::EmuThreadStatus;
use eframe::emath::Align;
use eframe::epaint::Color32;
use eframe::epaint::textures::TextureWrapMode;
use egui::epaint::ImageDelta;
use egui::{
    ColorImage, Context, ImageData, Layout, Slider, TextureFilter, TextureId, TextureOptions, Ui,
    Window,
};
use emath::Vec2;
use jgenesis_common::frontend::TimingMode;
use nes_config::palettes::PaletteGenerationArgs;
use nes_config::{NesAspectRatio, NesAudioResampler, NesPalette, Overscan, PaletteLoadError};
use rfd::FileDialog;
use std::mem;
use std::sync::Arc;

pub struct OverscanState {
    top_text: String,
    top_invalid: bool,
    bottom_text: String,
    bottom_invalid: bool,
    left_text: String,
    left_invalid: bool,
    right_text: String,
    right_invalid: bool,
}

impl From<Overscan> for OverscanState {
    fn from(value: Overscan) -> Self {
        Self {
            top_text: value.top.to_string(),
            top_invalid: false,
            bottom_text: value.bottom.to_string(),
            bottom_invalid: false,
            left_text: value.left.to_string(),
            left_invalid: false,
            right_text: value.right.to_string(),
            right_invalid: false,
        }
    }
}

pub struct NesPaletteState {
    texture_64_color: TextureId,
    texture_512_color: TextureId,
    display_512_color_palette: bool,
    load_error: Option<PaletteLoadError>,
    generate_args: PaletteGenerationArgs,
}

impl NesPaletteState {
    pub fn create(ctx: &Context, palette: &NesPalette) -> Self {
        let NesPaletteTextures { texture_64_color, texture_512_color } =
            create_palette_textures(ctx, palette);

        Self {
            texture_64_color,
            texture_512_color,
            display_512_color_palette: false,
            load_error: None,
            generate_args: PaletteGenerationArgs::default(),
        }
    }
}

impl App {
    pub(super) fn render_nes_general_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::NesGeneral;

        let mut open = true;
        Window::new("NES General Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .group(|ui| {
                    ui.add_enabled_ui(
                        self.emu_thread.status() != EmuThreadStatus::RunningNes,
                        |ui| {
                            ui.label("Timing / display mode");

                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut self.config.nes.forced_timing_mode,
                                    None,
                                    "Auto",
                                );
                                ui.radio_value(
                                    &mut self.config.nes.forced_timing_mode,
                                    Some(TimingMode::Ntsc),
                                    "NTSC (60Hz)",
                                );
                                ui.radio_value(
                                    &mut self.config.nes.forced_timing_mode,
                                    Some(TimingMode::Pal),
                                    "PAL (50Hz)",
                                );
                            });
                        },
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::TIMING_MODE);
            }

            let rect = ui
                .checkbox(
                    &mut self.config.nes.allow_opposing_joypad_inputs,
                    "Allow simultaneous opposing directional inputs",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::OPPOSING_DIRECTIONAL_INPUTS);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    pub(super) fn render_nes_video_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::NesVideo;

        let mut open = true;
        Window::new("NES Video Settings").open(&mut open).min_width(600.0).show(ctx, |ui| {
            widgets::render_vertical_scroll_area(ui, |ui| {
                self.render_aspect_ratio_setting(ui, WINDOW);

                let rect = ui
                    .checkbox(
                        &mut self.config.nes.ntsc_crop_vertical_overscan,
                        "(NTSC) Crop vertical overscan",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::NTSC_V_OVERSCAN);
                }

                let rect = ui
                    .checkbox(
                        &mut self.config.nes.remove_sprite_limit,
                        "Remove sprite-per-scanline limit",
                    )
                    .on_hover_text(
                        "Eliminates most sprite flickering but can cause visual glitches",
                    )
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::REMOVE_SPRITE_LIMIT);
                }

                let rect = ui
                    .checkbox(&mut self.config.nes.pal_black_border, "Render PAL black border")
                    .on_hover_text("Crops the image from 256x240 to 252x239")
                    .interact_rect;
                if ui.rect_contains_pointer(rect) {
                    self.state.help_text.insert(WINDOW, helptext::PAL_BLACK_BORDER);
                }

                ui.separator();
                self.render_overscan_setting(ui, WINDOW);

                ui.separator();
                self.render_palette_settings(ui, WINDOW);
            });

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    fn render_aspect_ratio_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Aspect ratio");

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.config.nes.aspect_ratio, NesAspectRatio::Ntsc, "NTSC")
                        .on_hover_text("8:7 pixel aspect ratio");
                    ui.radio_value(&mut self.config.nes.aspect_ratio, NesAspectRatio::Pal, "PAL")
                        .on_hover_text("11:8 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.nes.aspect_ratio,
                        NesAspectRatio::SquarePixels,
                        "Square pixels",
                    )
                    .on_hover_text("1:1 pixel aspect ratio");
                    ui.radio_value(
                        &mut self.config.nes.aspect_ratio,
                        NesAspectRatio::Stretched,
                        "Stretched",
                    )
                    .on_hover_text("Stretched to fill the window");
                });
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::ASPECT_RATIO);
        }
    }

    fn render_overscan_setting(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .group(|ui| {
                ui.label("Overscan in pixels");

                ui.vertical_centered(|ui| {
                    ui.label("Top");
                    ui.add(
                        NumericTextEdit::new(
                            &mut self.state.overscan.top_text,
                            &mut self.config.nes.overscan.top,
                            &mut self.state.overscan.top_invalid,
                        )
                        .desired_width(30.0),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Left");
                    ui.add(
                        NumericTextEdit::new(
                            &mut self.state.overscan.left_text,
                            &mut self.config.nes.overscan.left,
                            &mut self.state.overscan.left_invalid,
                        )
                        .desired_width(30.0),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label("Right");
                        ui.add(
                            NumericTextEdit::new(
                                &mut self.state.overscan.right_text,
                                &mut self.config.nes.overscan.right,
                                &mut self.state.overscan.right_invalid,
                            )
                            .desired_width(30.0),
                        );
                    });
                });

                ui.vertical_centered(|ui| {
                    ui.add(
                        NumericTextEdit::new(
                            &mut self.state.overscan.bottom_text,
                            &mut self.config.nes.overscan.bottom,
                            &mut self.state.overscan.bottom_invalid,
                        )
                        .desired_width(30.0),
                    );
                    ui.label("Bottom");
                });

                for (invalid, label) in [
                    (self.state.overscan.top_invalid, "Top"),
                    (self.state.overscan.bottom_invalid, "Bottom"),
                    (self.state.overscan.left_invalid, "Left"),
                    (self.state.overscan.right_invalid, "Right"),
                ] {
                    if invalid {
                        ui.colored_label(
                            Color32::RED,
                            format!("{label} value must be a non-negative integer"),
                        );
                    }
                }
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::OVERSCAN);
        }
    }

    fn render_palette_settings(&mut self, ui: &mut Ui, window: OpenWindow) {
        let rect = ui
            .scope(|ui| {
                ui.heading("Palette");

                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    if ui.button("Restore default").clicked() {
                        self.config.nes.palette = NesPalette::default();
                    }

                    if ui.button("Load from file...").clicked() {
                        self.state.nes_palette.load_error = None;
                        self.load_palette_from_file();
                    }

                    if ui.button("Export to file...").clicked() {
                        self.export_palette_to_file();
                    }
                });

                if let Some(err) = &self.state.nes_palette.load_error {
                    ui.colored_label(Color32::RED, format!("Error loading palette: {err}"));
                }

                ui.add_space(5.0);

                self.render_palette_generator(ui);

                ui.checkbox(
                    &mut self.state.nes_palette.display_512_color_palette,
                    "Display full 512-color palette",
                );

                let image = if self.state.nes_palette.display_512_color_palette {
                    (self.state.nes_palette.texture_512_color, Vec2::new(600.0, 500.0))
                } else {
                    (self.state.nes_palette.texture_64_color, Vec2::new(600.0, 500.0 / 8.0))
                };
                ui.image(image);
            })
            .response
            .interact_rect;
        if ui.rect_contains_pointer(rect) {
            self.state.help_text.insert(window, helptext::PALETTE);
        }
    }

    fn render_palette_generator(&mut self, ui: &mut Ui) {
        let original_slider_width = mem::replace(&mut ui.style_mut().spacing.slider_width, 450.0);

        let fmt_2f = |v: f64, _| format!("{v:.2}");

        ui.collapsing("Palette Generator", |ui| {
            ui.add(
                Slider::new(&mut self.state.nes_palette.generate_args.brightness, 0.0..=5.0)
                    .step_by(0.01)
                    .text("Brightness")
                    .custom_formatter(fmt_2f),
            );

            ui.add(
                Slider::new(&mut self.state.nes_palette.generate_args.saturation, 0.0..=5.0)
                    .step_by(0.01)
                    .text("Saturation")
                    .custom_formatter(fmt_2f),
            );

            ui.add(
                Slider::new(&mut self.state.nes_palette.generate_args.contrast, 0.0..=5.0)
                    .step_by(0.01)
                    .text("Contrast")
                    .custom_formatter(fmt_2f),
            );

            ui.add(
                Slider::new(&mut self.state.nes_palette.generate_args.hue_offset, -6.0..=6.0)
                    .step_by(0.01)
                    .text("Hue offset")
                    .custom_formatter(fmt_2f),
            );

            ui.add(
                Slider::new(&mut self.state.nes_palette.generate_args.gamma, 0.1..=3.0)
                    .step_by(0.1)
                    .text("Gamma")
                    .custom_formatter(|v, _| format!("{v:.1}")),
            );

            ui.horizontal(|ui| {
                if ui.button("Generate").clicked() {
                    self.config.nes.palette =
                        nes_config::palettes::generate(self.state.nes_palette.generate_args);
                }

                if ui.button("Default settings").clicked() {
                    self.state.nes_palette.generate_args = PaletteGenerationArgs::default();
                }
            });
        });

        ui.style_mut().spacing.slider_width = original_slider_width;
    }

    fn load_palette_from_file(&mut self) {
        let Some(path) = FileDialog::new()
            .add_filter("pal", &["pal"])
            .add_filter("All Files", &["*"])
            .pick_file()
        else {
            return;
        };

        match NesPalette::read_from(&path) {
            Ok(palette) => {
                self.config.nes.palette = palette;
            }
            Err(err) => {
                log::error!("Failed to load palette from path '{}': {err}", path.display());

                self.state.nes_palette.load_error = Some(err);
            }
        }
    }

    fn export_palette_to_file(&self) {
        let Some(path) = FileDialog::new()
            .set_file_name("nespalette.pal")
            .add_filter("pal", &["pal"])
            .save_file()
        else {
            return;
        };

        if let Err(err) = self.config.nes.palette.write_to(&path) {
            log::error!("Error saving palette to path '{}': {err}", path.display());
        }
    }

    pub(super) fn render_nes_audio_settings(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::NesAudio;

        let mut open = true;
        Window::new("NES Audio Settings").open(&mut open).resizable(false).show(ctx, |ui| {
            let rect = ui
                .checkbox(
                    &mut self.config.nes.silence_ultrasonic_triangle_output,
                    "Silence ultrasonic triangle channel output",
                )
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::ULTRASONIC_TRIANGLE);
            }

            let rect = ui
                .checkbox(&mut self.config.nes.audio_60hz_hack, "Enable audio sync timing hack")
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_TIMING_HACK);
            }

            ui.add_space(5.0);
            let rect = ui
                .group(|ui| {
                    ui.label("Audio resampling algorithm");

                    ui.radio_value(
                        &mut self.config.nes.audio_resampler,
                        NesAudioResampler::LowPassNearestNeighbor,
                        "Low-pass filter + nearest neighbor (Faster)",
                    );
                    ui.radio_value(
                        &mut self.config.nes.audio_resampler,
                        NesAudioResampler::WindowedSinc,
                        "Windowed sinc interpolation (Higher quality)",
                    );
                })
                .response
                .interact_rect;
            if ui.rect_contains_pointer(rect) {
                self.state.help_text.insert(WINDOW, helptext::AUDIO_RESAMPLING);
            }

            self.render_help_text(ui, WINDOW);
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }
}

fn nes_palette_to_pixels(palette: &NesPalette) -> Vec<Color32> {
    let mut pixels = vec![Color32::BLACK; 512];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let (r, g, b) = palette[i];
        *pixel = Color32::from_rgb(r, g, b);
    }

    pixels
}

const PALETTE_TEXTURE_SIZE_64: [usize; 2] = [16, 4];
const PALETTE_TEXTURE_SIZE_64_1D: usize = PALETTE_TEXTURE_SIZE_64[0] * PALETTE_TEXTURE_SIZE_64[1];
const PALETTE_TEXTURE_SIZE_512: [usize; 2] = [16, 32];

const PALETTE_TEXTURE_OPTIONS: TextureOptions = TextureOptions {
    magnification: TextureFilter::Nearest,
    minification: TextureFilter::Nearest,
    wrap_mode: TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

pub struct NesPaletteTextures {
    pub texture_64_color: TextureId,
    pub texture_512_color: TextureId,
}

pub fn create_palette_textures(ctx: &Context, palette: &NesPalette) -> NesPaletteTextures {
    let pixels = nes_palette_to_pixels(palette);

    let pixels_64_color = pixels[..PALETTE_TEXTURE_SIZE_64_1D].to_vec();
    let texture_64_color = ctx.tex_manager().write().alloc(
        "nes_palette_texture_64c".into(),
        ImageData::Color(Arc::new(ColorImage {
            size: PALETTE_TEXTURE_SIZE_64,
            pixels: pixels_64_color,
        })),
        PALETTE_TEXTURE_OPTIONS,
    );

    let texture_512_color = ctx.tex_manager().write().alloc(
        "nes_palette_texture_512c".into(),
        ImageData::Color(Arc::new(ColorImage { size: PALETTE_TEXTURE_SIZE_512, pixels })),
        PALETTE_TEXTURE_OPTIONS,
    );

    NesPaletteTextures { texture_64_color, texture_512_color }
}

pub fn update_palette_textures(ctx: &Context, state: &NesPaletteState, palette: &NesPalette) {
    let pixels = nes_palette_to_pixels(palette);

    let pixels_64_color = pixels[..PALETTE_TEXTURE_SIZE_64_1D].to_vec();
    ctx.tex_manager().write().set(
        state.texture_64_color,
        ImageDelta {
            image: ImageData::Color(Arc::new(ColorImage {
                size: PALETTE_TEXTURE_SIZE_64,
                pixels: pixels_64_color,
            })),
            options: PALETTE_TEXTURE_OPTIONS,
            pos: None,
        },
    );

    ctx.tex_manager().write().set(
        state.texture_512_color,
        ImageDelta {
            image: ImageData::Color(Arc::new(ColorImage {
                size: PALETTE_TEXTURE_SIZE_512,
                pixels,
            })),
            options: PALETTE_TEXTURE_OPTIONS,
            pos: None,
        },
    );
}
