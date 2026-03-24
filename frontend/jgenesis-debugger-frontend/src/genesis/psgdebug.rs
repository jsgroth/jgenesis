use egui::{Grid, Ui, Window};
use smsgg_core::psg::{NoiseMode, NoiseReload, Sn76489};
use std::borrow::Cow;

pub const WINDOW_TITLE: &str = "SN76489 Registers";

const NTSC_MCLK_FREQUENCY: f64 = 53_693_175.0;

// Roughly 223722 Hz
const SAMPLE_RATE: f64 = NTSC_MCLK_FREQUENCY / 15.0 / 16.0;

pub fn render_debug_window(ctx: &egui::Context, psg: &Sn76489, open: &mut bool) {
    Window::new(WINDOW_TITLE)
        .open(open)
        .constrain(false)
        .resizable([true, true])
        .default_pos(crate::rand_window_pos())
        .show(ctx, |ui| {
            for i in 0..3 {
                ui.group(|ui| {
                    render_tone_channel(i, psg, ui);
                });
            }

            ui.group(|ui| {
                render_noise_channel(psg, ui);
            });
        });
}

fn render_tone_channel(i: usize, psg: &Sn76489, ui: &mut Ui) {
    let period = psg.tone_frequencies()[i];
    let attenuation = psg.tone_attenuations()[i];

    ui.heading(format!("Tone {i}"));

    Grid::new(format!("psg_tone_{i}_grid")).show(ui, |ui| {
        ui.label("Period");
        ui.label(format!("{period} ({:.2} Hz)", tone_frequency(period)));
        ui.end_row();

        ui.label("Attenuation");
        ui.label(format!("{attenuation} ({})", attenuation_str(attenuation)));
        ui.end_row();
    });
}

fn render_noise_channel(psg: &Sn76489, ui: &mut Ui) {
    let mode = psg.noise_mode();
    let reload = psg.noise_reload();
    let attenuation = psg.noise_attenuation();

    let mode_str = match mode {
        NoiseMode::White => "White noise",
        NoiseMode::Periodic => "Periodic noise",
    };

    let reload_str: Cow<'static, str> = match reload {
        NoiseReload::Value(value) => format!("0x{value:02X}").into(),
        NoiseReload::Tone2 => "Tone 2".into(),
    };

    ui.heading("Noise");

    Grid::new("psg_noise_grid").show(ui, |ui| {
        ui.label("Mode");
        ui.label(mode_str);
        ui.end_row();

        ui.label("Counter reload");
        ui.label(reload_str);
        ui.end_row();

        ui.label("Attenuation");
        ui.label(format!("{attenuation} ({})", attenuation_str(attenuation)));
        ui.end_row();
    });
}

fn tone_frequency(period: u16) -> f64 {
    if period == 0 {
        return tone_frequency(1);
    }

    SAMPLE_RATE / 2.0 / f64::from(period)
}

fn attenuation_str(attenuation: u8) -> Cow<'static, str> {
    match attenuation {
        15 => "Muted".into(),
        _ => format!("{} dB", 2 * attenuation).into(),
    }
}
