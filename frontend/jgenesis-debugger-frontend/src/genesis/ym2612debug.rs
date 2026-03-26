use egui::emath::OrderedFloat;
use egui::scroll_area::ScrollBarVisibility;
use egui::style::ScrollStyle;
use egui::{Grid, ScrollArea, Ui, Window};
use genesis_core::ym2612::{
    Channel3FrequencyMode, ChannelRegisters, GlobalRegisters, OperatorRegisters, TimerState,
    Ym2612DebugView,
};
use std::iter;

const WINDOW_TITLE: &str = "YM2612 Registers";

const NTSC_MCLK_FREQUENCY: f64 = 53_693_175.0;

// Roughly 53267 Hz
const SAMPLE_RATE: f64 = NTSC_MCLK_FREQUENCY / 7.0 / 6.0 / 24.0;

const ALGORITHM_IMAGES: &[egui::ImageSource<'static>; 8] = &[
    egui::include_image!("ym2612/algorithm0.png"),
    egui::include_image!("ym2612/algorithm1.png"),
    egui::include_image!("ym2612/algorithm2.png"),
    egui::include_image!("ym2612/algorithm3.png"),
    egui::include_image!("ym2612/algorithm4.png"),
    egui::include_image!("ym2612/algorithm5.png"),
    egui::include_image!("ym2612/algorithm6.png"),
    egui::include_image!("ym2612/algorithm7.png"),
];

pub struct Ym2612DebugWindowState {
    pub open: bool,
    pub channel: u8,
    pub ever_rendered: bool,
}

impl Ym2612DebugWindowState {
    pub fn new() -> Self {
        Self { open: false, channel: 0, ever_rendered: false }
    }

    pub fn open_window(&mut self, ctx: &egui::Context) {
        self.open = true;
        crate::move_to_top(ctx, WINDOW_TITLE);
    }
}

pub fn render_debug_window(
    ctx: &egui::Context,
    ym2612: Ym2612DebugView<'_>,
    state: &mut Ym2612DebugWindowState,
) {
    if state.open && !state.ever_rendered {
        // Preload algorithm images to avoid flicker due to egui asynchronously loading images
        for image in ALGORITHM_IMAGES {
            if let egui::ImageSource::Bytes { uri, bytes } = image {
                ctx.include_bytes(uri.clone(), bytes.clone());
                let _ = ctx.try_load_image(uri, egui::SizeHint::Scale(OrderedFloat(1.0)));
            }
        }
    }
    state.ever_rendered |= state.open;

    Window::new(WINDOW_TITLE)
        .open(&mut state.open)
        .constrain(false)
        .resizable([true, true])
        .default_pos([50.0, 50.0])
        .default_size([700.0, 650.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Channel:");

                for i in 0..6 {
                    ui.radio_value(&mut state.channel, i, (i + 1).to_string());
                }
            });

            ui.spacing_mut().scroll = ScrollStyle::solid();

            ScrollArea::vertical()
                .auto_shrink([true, false])
                .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    let global = ym2612.global_registers();
                    let channel = ym2612.channel_registers();
                    let operator = ym2612.operator_registers();

                    ui.horizontal(|ui| {
                        render_operator_registers(
                            state.channel,
                            &operator[state.channel as usize],
                            &channel[state.channel as usize],
                            &global,
                            ui,
                        );

                        render_algorithm_image(channel[state.channel as usize].algorithm, ui);
                    });

                    ui.horizontal(|ui| {
                        render_channel_registers(
                            state.channel,
                            &channel[state.channel as usize],
                            ui,
                        );
                        render_global_registers(&global, ui);
                    });

                    render_timer_registers(&global, ui);
                });
        });
}

fn render_algorithm_image(algorithm: u8, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.heading(format!("Algorithm {algorithm}"));

        ui.add_space(10.0);

        let image = ALGORITHM_IMAGES[algorithm as usize].clone();
        ui.add(egui::Image::new(image).show_loading_spinner(false));
    });
}

fn render_operator_registers(
    channel_idx: u8,
    operators: &[OperatorRegisters; 4],
    channel: &ChannelRegisters,
    global: &GlobalRegisters,
    ui: &mut Ui,
) {
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.heading(format!("Channel {} Operator Registers", channel_idx + 1));

            Grid::new("ym2612_operator_grid").min_col_width(70.0).show(ui, |ui| {
                ui.label("");
                ui.label("Operator 1");
                ui.label("Operator 2");
                ui.label("Operator 3");
                ui.label("Operator 4");
                ui.end_row();

                ui.label("Keyed on");
                for operator in operators {
                    fake_checkbox(operator.key_on, ui);
                }
                ui.end_row();

                let mut f_numbers = [channel.f_number; 4];
                let mut blocks = [channel.block; 4];
                if channel_idx == 2
                    && matches!(
                        global.channel_3_frequency_mode,
                        Channel3FrequencyMode::PerOperator | Channel3FrequencyMode::Csm
                    )
                {
                    f_numbers[..3].copy_from_slice(&global.channel_3_f_numbers);
                    blocks[..3].copy_from_slice(&global.channel_3_blocks);
                }

                ui.label("F-number");
                for f_number in f_numbers {
                    ui.label(f_number.to_string());
                }
                ui.end_row();

                ui.label("Block");
                for block in blocks {
                    ui.label(block.to_string());
                }
                ui.end_row();

                ui.label("Frequency");
                for (f_number, block) in iter::zip(f_numbers, blocks) {
                    let frequency = channel_frequency(f_number, block);
                    ui.label(format!("{frequency:.2} Hz"));
                }
                ui.end_row();

                ui.label("Detune");
                for operator in operators {
                    let magnitude = operator.detune & 3;
                    let sign = if operator.detune & 4 != 0 { "-" } else { "+" };
                    ui.label(format!("{} ({sign}{magnitude})", operator.detune));
                }
                ui.end_row();

                ui.label("Multiple");
                for operator in operators {
                    ui.label(operator.multiple.to_string());
                }
                ui.end_row();

                ui.label("Attack rate");
                for operator in operators {
                    ui.label(hex_and_decimal(operator.attack_rate));
                }
                ui.end_row();

                ui.label("Decay rate");
                for operator in operators {
                    ui.label(hex_and_decimal(operator.decay_rate));
                }
                ui.end_row();

                ui.label("Sustain rate");
                for operator in operators {
                    ui.label(hex_and_decimal(operator.sustain_rate));
                }
                ui.end_row();

                ui.label("Release rate");
                for operator in operators {
                    ui.label(format!(
                        "0x{:02X} ({})",
                        operator.release_rate,
                        2 * operator.release_rate + 1
                    ));
                }
                ui.end_row();

                ui.label("Sustain level");
                for operator in operators {
                    ui.label(format!("0x{:02X}", operator.sustain_level));
                }
                ui.end_row();

                ui.label("Total level");
                for operator in operators {
                    ui.label(format!("0x{:02X}", operator.total_level));
                }
                ui.end_row();

                ui.label("Key scaling level");
                for operator in operators {
                    ui.label(operator.key_scale_level.to_string());
                }
                ui.end_row();

                ui.label("LFO AM enabled");
                for operator in operators {
                    fake_checkbox(operator.tremolo_enabled, ui);
                }
                ui.end_row();

                ui.label("SSG-EG enabled");
                for operator in operators {
                    fake_checkbox(operator.ssg_enabled, ui);
                }
                ui.end_row();

                ui.label("SSG-EG attack");
                for operator in operators {
                    fake_checkbox(operator.ssg_attack, ui);
                }
                ui.end_row();

                ui.label("SSG-EG alternate");
                for operator in operators {
                    fake_checkbox(operator.ssg_alternate, ui);
                }
                ui.end_row();

                ui.label("SSG-EG hold");
                for operator in operators {
                    fake_checkbox(operator.ssg_hold, ui);
                }
                ui.end_row();
            });
        });
    });
}

fn render_channel_registers(channel_idx: u8, channel: &ChannelRegisters, ui: &mut Ui) {
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.heading(format!("Channel {} Registers", channel_idx + 1));

            Grid::new("ym2612_channel_grid").min_col_width(70.0).show(ui, |ui| {
                ui.label("Algorithm");
                ui.label(channel.algorithm.to_string());
                ui.end_row();

                ui.label("Feedback level");
                ui.label(channel.feedback_level.to_string());
                ui.end_row();

                ui.label("LFO FM sensitivity");
                ui.label(channel.fm_sensitivity.to_string());
                ui.end_row();

                ui.label("LFO AM sensitivity");
                ui.label(channel.am_sensitivity.to_string());
                ui.end_row();

                ui.label("L output");
                fake_checkbox(channel.l_output, ui);
                ui.end_row();

                ui.label("R output");
                fake_checkbox(channel.r_output, ui);
                ui.end_row();
            });
        });
    });
}

fn render_global_registers(global: &GlobalRegisters, ui: &mut Ui) {
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.heading("Global Registers");

            Grid::new("ym2612_global_grid").min_col_width(70.0).show(ui, |ui| {
                ui.label("DAC channel enabled");
                fake_checkbox(global.dac_channel_enabled, ui);
                ui.end_row();

                ui.label("DAC channel sample");
                ui.label(global.dac_channel_sample.to_string());
                ui.end_row();

                ui.label("LFO enabled");
                fake_checkbox(global.lfo.enabled, ui);
                ui.end_row();

                ui.label("LFO frequency");
                ui.label(format!(
                    "{} ({:.2} Hz)",
                    global.lfo.frequency,
                    lfo_frequency(global.lfo.divider)
                ));
                ui.end_row();

                ui.label("Channel 3 frequency mode");
                ui.label(
                    if matches!(
                        global.channel_3_frequency_mode,
                        Channel3FrequencyMode::PerOperator | Channel3FrequencyMode::Csm
                    ) {
                        "Per-operator"
                    } else {
                        "Normal"
                    },
                );
                ui.end_row();

                ui.label("CSM enabled");
                fake_checkbox(global.channel_3_frequency_mode == Channel3FrequencyMode::Csm, ui);
                ui.end_row();
            });
        });
    });
}

fn render_timer_registers(global: &GlobalRegisters, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.heading("Timer A");

                Grid::new("ym2612_timer_a_grid").show(ui, |ui| {
                    render_timer_grid(&global.timer_a, timer_a_frequency, ui);
                });
            })
        });

        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.heading("Timer B");

                Grid::new("ym2612_timer_b_grid").show(ui, |ui| {
                    render_timer_grid(&global.timer_b, timer_b_frequency, ui);
                });
            });
        });
    });
}

fn render_timer_grid(timer: &TimerState, frequency_fn: impl Fn(u16) -> f64, ui: &mut Ui) {
    ui.label("Enabled");
    fake_checkbox(timer.enabled, ui);
    ui.end_row();

    ui.label("Frequency");
    ui.label(format!("{} ({:.2} Hz)", timer.interval, frequency_fn(timer.interval)));
    ui.end_row();

    ui.label("Overflow flag enabled");
    fake_checkbox(timer.overflow_flag_enabled, ui);
    ui.end_row();

    ui.label("Overflow flag");
    fake_checkbox(timer.overflow_flag, ui);
    ui.end_row();
}

fn fake_checkbox(mut value: bool, ui: &mut Ui) {
    ui.scope(|ui| {
        ui.visuals_mut().disabled_alpha = 1.0;

        ui.add_enabled_ui(false, |ui| ui.checkbox(&mut value, ""));
    });
}

fn channel_frequency(f_number: u16, block: u8) -> f64 {
    // fnote = Fnum * 2^(B-1) / 2^20 * sample_rate
    SAMPLE_RATE * f64::from((f_number << block) >> 1) / f64::from(1 << 20)
}

fn timer_a_frequency(interval: u16) -> f64 {
    SAMPLE_RATE / f64::from(1024 - interval)
}

fn timer_b_frequency(interval: u16) -> f64 {
    SAMPLE_RATE / 16.0 / f64::from(256 - interval)
}

fn lfo_frequency(divider: u8) -> f64 {
    SAMPLE_RATE / f64::from(divider) / 128.0
}

fn hex_and_decimal(value: u8) -> String {
    format!("0x{value:02X} ({value})")
}
