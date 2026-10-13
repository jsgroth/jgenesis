use crate::app::{App, OpenWindow};
use crate::emurunner::{EmuRunnerCommand, EmuRunnerStatus};
use directories::UserDirs;
use egui::{Color32, Context, Slider, Ui, Window};
use jgenesis_native_driver::{
    AudioCaptureFormat, CaptureType, VideoCaptureCodec, VideoCaptureCodecType,
};
use jgenesis_renderer::config::PreprocessShader;
use jgenesis_renderer::renderer::RendererCaptureOutput;
use rfd::FileDialog;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub struct CaptureAppState {
    renderer_capture_output: RendererCaptureOutput,
    video_codec_type: VideoCaptureCodecType,
    video_compression_level: u32,
    audio_format: AudioCaptureFormat,
    recording_path: Option<PathBuf>,
}

impl CaptureAppState {
    pub fn new() -> Self {
        Self {
            renderer_capture_output: RendererCaptureOutput::default(),
            video_codec_type: VideoCaptureCodecType::default(),
            video_compression_level: 3,
            audio_format: AudioCaptureFormat::default(),
            recording_path: None,
        }
    }

    fn video_codec(&self) -> VideoCaptureCodec {
        match self.video_codec_type {
            VideoCaptureCodecType::None => VideoCaptureCodec::None,
            VideoCaptureCodecType::CamStudio => {
                VideoCaptureCodec::CamStudio { level: self.video_compression_level }
            }
        }
    }
}

impl App {
    pub fn render_video_recording_window(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::RecordVideo.title()).open(&mut open).show(ctx, |ui| {
            let capturing_any = self.emu_runner.capturing().is_some();
            let capturing_video = self.emu_runner.capturing().is_some_and(CaptureType::is_video);
            let emu_running = self.emu_runner.status().is_running();

            ui.add_enabled_ui(!capturing_video, |ui| {
                ui.group(|ui| {
                    ui.label("Video capture type");

                    for (value, label) in [
                        (
                            RendererCaptureOutput::ConsoleNative,
                            "Direct console output (console native resolution)",
                        ),
                        (
                            RendererCaptureOutput::Emulator,
                            "Final emulator output (emulator window resolution)",
                        ),
                    ] {
                        ui.radio_value(
                            &mut self.state.capture.renderer_capture_output,
                            value,
                            label,
                        );
                    }
                });

                if self.state.capture.renderer_capture_output
                    == RendererCaptureOutput::ConsoleNative
                    && self.emu_runner.status() == EmuRunnerStatus::RunningNes
                    && self.config.common.preprocess_shader == PreprocessShader::NtscComposite
                {
                    ui.colored_label(
                        Color32::RED,
                        "Direct console output recording does not work correctly with NES + NTSC composite shader; record emulator output or select a different shader",
                    );
                }

                ui.group(|ui| {
                    ui.label("Video codec");

                    for (value, label) in [
                        (VideoCaptureCodecType::None, "Uncompressed RGB"),
                        (VideoCaptureCodecType::CamStudio, "CamStudio lossless (CSCD)"),
                    ] {
                        ui.radio_value(&mut self.state.capture.video_codec_type, value, label);
                    }

                    ui.add_enabled_ui(
                        self.state.capture.video_codec_type == VideoCaptureCodecType::CamStudio,
                        |ui| {
                            ui.label("Compression level (lower is faster, higher is better):");
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                ui.add(Slider::new(
                                    &mut self.state.capture.video_compression_level,
                                    1..=9,
                                ));
                            });
                        },
                    );
                });
            });

            render_audio_format_selector(capturing_any, &mut self.state.capture.audio_format, ui);

            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.add_enabled_ui(emu_running && !capturing_any, |ui| {
                    if ui.button("Start recording...").clicked() {
                        let user_dirs = UserDirs::new();
                        let video_dir = user_dirs.as_ref().and_then(UserDirs::video_dir);

                        if let Some(path) = save_file_for_recording(
                            &self.state.current_file_path,
                            "avi",
                            video_dir,
                            "video.avi",
                        ) {
                            self.emu_runner.push_command(EmuRunnerCommand::StartVideoCapture(
                                path.clone(),
                                self.state.capture.renderer_capture_output,
                                self.state.capture.video_codec(),
                                self.state.capture.audio_format,
                            ));

                            self.state.capture.recording_path = Some(path);
                        }
                    }
                });

                ui.add_enabled_ui(capturing_video, |ui| {
                    if ui.button("Stop recording").clicked() {
                        self.emu_runner.push_command(EmuRunnerCommand::EndCapture);
                    }
                });
            });

            render_recording_to(capturing_video, self.state.capture.recording_path.as_ref(), ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::RecordVideo);
        }
    }

    pub fn render_audio_recording_window(&mut self, ctx: &Context) {
        let mut open = true;
        Window::new(OpenWindow::RecordAudio.title()).open(&mut open).show(ctx, |ui| {
            let capturing_any = self.emu_runner.capturing().is_some();
            let capturing_audio = self.emu_runner.capturing() == Some(CaptureType::Audio);
            let emu_running = self.emu_runner.status().is_running();

            render_audio_format_selector(capturing_any, &mut self.state.capture.audio_format, ui);

            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.add_enabled_ui(emu_running && !capturing_any, |ui| {
                    if ui.button("Start recording...").clicked() {
                        let user_dirs = UserDirs::new();
                        let audio_dir = user_dirs.as_ref().and_then(UserDirs::audio_dir);

                        if let Some(path) = save_file_for_recording(
                            &self.state.current_file_path,
                            "wav",
                            audio_dir,
                            "audio.wav",
                        ) {
                            self.emu_runner.push_command(EmuRunnerCommand::StartAudioCapture(
                                path.clone(),
                                self.state.capture.audio_format,
                            ));

                            self.state.capture.recording_path = Some(path);
                        }
                    }
                });

                ui.add_enabled_ui(capturing_audio, |ui| {
                    if ui.button("Stop recording").clicked() {
                        self.emu_runner.push_command(EmuRunnerCommand::EndCapture);
                    }
                });
            });

            render_recording_to(capturing_audio, self.state.capture.recording_path.as_ref(), ui);
        });
        if !open {
            self.state.open_windows.remove(&OpenWindow::RecordAudio);
        }
    }
}

fn render_audio_format_selector(capturing_any: bool, format: &mut AudioCaptureFormat, ui: &mut Ui) {
    ui.add_enabled_ui(!capturing_any, |ui| {
        ui.group(|ui| {
            ui.label("Audio sample format");

            for (value, label) in [
                (AudioCaptureFormat::I16, "16-bit integer"),
                (AudioCaptureFormat::F32, "32-bit float"),
            ] {
                ui.radio_value(format, value, label);
            }
        });
    });
}

fn save_file_for_recording(
    current_file_path: &Path,
    extension: &str,
    directory: Option<&Path>,
    default_name: &str,
) -> Option<PathBuf> {
    let file_path_changed_ext = current_file_path.with_extension(extension);
    let default_file_name = file_path_changed_ext
        .file_name()
        .map(OsStr::to_string_lossy)
        .unwrap_or(default_name.into());

    let mut file_dialog = FileDialog::new()
        .add_filter(extension, &[extension])
        .add_filter("All Files", &["*"])
        .set_file_name(default_file_name);

    if let Some(directory) = directory {
        file_dialog = file_dialog.set_directory(directory);
    }

    file_dialog.save_file()
}

fn render_recording_to(recording: bool, path: Option<&PathBuf>, ui: &mut Ui) {
    ui.add_space(10.0);

    ui.label("Recording to:");
    ui.horizontal(|ui| {
        ui.add_space(10.0);

        if recording && let Some(path) = path {
            ui.label(path.display().to_string());
        } else {
            ui.label("(Not recording)");
        }
    });
}
