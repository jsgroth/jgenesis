use crate::app::{App, OpenWindow};
use crate::emurunner::EmuRunnerCommand;
use directories::UserDirs;
use egui::{Context, Window};
use jgenesis_native_driver::AudioCaptureFormat;
use rfd::FileDialog;
use std::ffi::OsStr;

impl App {
    pub fn render_audio_recording_window(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::RecordAudio.title()).open(&mut open).show(ctx, |ui| {
            let capturing_audio = self.emu_runner.is_capturing_audio();
            let emu_running = self.emu_runner.status().is_running();

            ui.add_enabled_ui(!capturing_audio, |ui| {
                ui.group(|ui| {
                    ui.label("Audio sample format");

                    for (value, label) in [
                        (AudioCaptureFormat::I16, "16-bit integer"),
                        (AudioCaptureFormat::F32, "32-bit float"),
                    ] {
                        ui.radio_value(&mut self.state.audio_capture_format, value, label);
                    }
                });
            });

            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.add_enabled_ui(emu_running && !capturing_audio, |ui| {
                    if ui.button("Start recording...").clicked() {
                        let file_path_wav_ext = self.state.current_file_path.with_extension("wav");
                        let default_file_name = file_path_wav_ext
                            .file_name()
                            .map(OsStr::to_string_lossy)
                            .unwrap_or("audio.wav".into());

                        let mut file_dialog = FileDialog::new()
                            .add_filter("WAV", &["wav"])
                            .add_filter("All Files", &["*"])
                            .set_file_name(default_file_name);

                        let user_dirs = directories::UserDirs::new();
                        if let Some(audio_dir) = user_dirs.as_ref().and_then(UserDirs::audio_dir) {
                            file_dialog = file_dialog.set_directory(audio_dir);
                        }

                        if let Some(path) = file_dialog.save_file() {
                            self.emu_runner.push_command(EmuRunnerCommand::StartAudioCapture(
                                path,
                                self.state.audio_capture_format,
                            ));
                        }
                    }
                });

                ui.add_enabled_ui(capturing_audio, |ui| {
                    if ui.button("Stop recording").clicked() {
                        self.emu_runner.push_command(EmuRunnerCommand::EndAudioCapture);
                    }
                });
            });
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::RecordAudio);
        }
    }
}
