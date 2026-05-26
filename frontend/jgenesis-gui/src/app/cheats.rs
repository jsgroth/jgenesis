use crate::app::{App, OpenWindow};
use egui::{
    CentralPanel, Context, FontFamily, Label, RichText, ScrollArea, Sense, SidePanel, TextEdit,
    TextStyle, TopBottomPanel, Ui, Window,
};
use egui_extras::{Column, TableBuilder};
use genesis_config::cheats::{GenesisCheat, GenesisCheatCodeType, GenesisCheats};
use jgenesis_native_driver::extensions::Console;
use std::borrow::Cow;
use std::path::Path;
use std::sync::{Arc, LazyLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveCheats {
    None,
    Genesis(GenesisCheats),
}

impl ActiveCheats {
    pub fn genesis_or_default(&self) -> &GenesisCheats {
        static DEFAULT: LazyLock<GenesisCheats> = LazyLock::new(GenesisCheats::default);

        match self {
            Self::Genesis(cheats) => cheats,
            Self::None => &DEFAULT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheatState {
    name: String,
    enabled: bool,
    codes: Vec<String>,
    codes_buffer: String,
    code_messages: Vec<Cow<'static, str>>,
}

impl CheatState {
    fn new() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            codes: vec![],
            codes_buffer: String::new(),
            code_messages: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheatWindowState {
    cheats: Vec<CheatState>,
    selected_cheat: usize,
    active: Arc<ActiveCheats>,
}

impl CheatWindowState {
    pub fn new() -> Self {
        Self { cheats: vec![], selected_cheat: 0, active: Arc::new(ActiveCheats::None) }
    }
}

impl From<GenesisCheats> for CheatWindowState {
    fn from(value: GenesisCheats) -> Self {
        Self {
            cheats: value
                .cheats
                .iter()
                .map(|cheat| {
                    let codes: Vec<_> =
                        cheat.codes.iter().map(|code| code.as_str().to_string()).collect();
                    let codes_buffer = codes.join("\n");
                    let code_messages =
                        codes.iter().map(|code| genesis_code_message(code)).collect();

                    CheatState {
                        name: cheat.name.clone(),
                        enabled: cheat.enabled,
                        codes,
                        codes_buffer,
                        code_messages,
                    }
                })
                .collect(),
            selected_cheat: 0,
            active: Arc::new(ActiveCheats::Genesis(value)),
        }
    }
}

impl App {
    pub(super) fn render_cheats_window(&mut self, ctx: &Context) {
        const WINDOW: OpenWindow = OpenWindow::Cheats;

        let mut open = true;
        Window::new(WINDOW.title()).open(&mut open).default_width(800.0).show(ctx, |ui| {
            let cheats_state = &mut self.state.cheats;

            TopBottomPanel::top("cheats_top_panel").show_inside(ui, |ui| {
                ui.checkbox(&mut self.config.common.cheats_enabled, "Cheats enabled");
            });

            let emu_thread_status = self.emu_thread.status();
            if !emu_thread_status.is_running() {
                render_centered_message(ui, "Load a game to edit cheats");
                return;
            }

            if !emu_thread_status.is_running_genesis_like() {
                // TODO update when other systems are supported
                render_centered_message(ui, "Cheats are not currently supported for this console");
                return;
            }

            ui.add_enabled_ui(self.config.common.cheats_enabled, |ui| {
                let mut cheat_just_created = false;

                SidePanel::left("cheats_left_panel").show_inside(ui, |ui| {
                    render_left_panel(ui, cheats_state, &mut cheat_just_created);
                });

                CentralPanel::default().show_inside(ui, |ui| {
                    let Some(cheat_state) =
                        cheats_state.cheats.get_mut(cheats_state.selected_cheat)
                    else {
                        return;
                    };

                    render_central_panel(ui, cheat_state, cheat_just_created);
                });
            });

            self.update_active_cheats();
        });
        if !open {
            self.state.open_windows.remove(&WINDOW);
        }
    }

    fn update_active_cheats(&mut self) {
        // TODO update when other consoles are supported
        let prev_active_cheats = Arc::clone(&self.state.cheats.active);
        let prev_active_cheats = prev_active_cheats.genesis_or_default();

        let genesis_cheats = GenesisCheats {
            cheats: self
                .state
                .cheats
                .cheats
                .iter()
                .map(|cheat| GenesisCheat {
                    name: cheat.name.clone(),
                    enabled: cheat.enabled,
                    codes: cheat.codes.clone(),
                })
                .collect(),
        };

        let changed = *prev_active_cheats != genesis_cheats;

        if changed {
            if let Err(err) = self.config.save_cheats(
                &self.config_path,
                &self.state.current_file_path,
                &genesis_cheats,
            ) {
                log::error!("Error saving cheats file: {err}");
            }

            let new_memory_override_vec = genesis_cheats.to_memory_override_vec();
            self.state.cheats.active = Arc::new(ActiveCheats::Genesis(genesis_cheats));

            let prev_memory_override_vec = prev_active_cheats.to_memory_override_vec();
            if prev_memory_override_vec != new_memory_override_vec {
                self.reload_config();
            }
        }
    }

    pub(super) fn load_cheats_for_game(&mut self, console: Console, rom_file_path: &Path) {
        // TODO change when other consoles are supported
        match console {
            Console::Genesis | Console::SegaCd | Console::Sega32X => {
                let Some(cheats) =
                    self.config.try_load_cheats::<GenesisCheats>(&self.config_path, rom_file_path)
                else {
                    return;
                };

                self.state.cheats = cheats.into();
            }
            _ => {
                self.state.cheats.active = Arc::new(ActiveCheats::None);
            }
        }
    }

    pub(super) fn active_cheats(&self) -> &Arc<ActiveCheats> {
        static NONE: LazyLock<Arc<ActiveCheats>> = LazyLock::new(|| Arc::new(ActiveCheats::None));

        if !self.config.common.cheats_enabled {
            return &NONE;
        }

        &self.state.cheats.active
    }
}

fn render_left_panel(
    ui: &mut Ui,
    cheats_state: &mut CheatWindowState,
    cheat_just_created: &mut bool,
) {
    ui.add_space(5.0);

    if ui.button("Add New").clicked() {
        cheats_state.cheats.push(CheatState::new());

        cheats_state.selected_cheat = cheats_state.cheats.len() - 1;
        *cheat_just_created = true;
    }

    ui.add_space(5.0);

    let mut rows_to_remove = Vec::<usize>::new();

    TableBuilder::new(ui)
        .column(Column::auto())
        .column(Column::auto().at_least(200.0))
        .column(Column::remainder().at_least(60.0))
        .sense(Sense::click())
        .body(|body| {
            body.rows(15.0, cheats_state.cheats.len(), |mut row| {
                let row_index = row.index();
                row.set_selected(row_index == cheats_state.selected_cheat);

                let cheat = &mut cheats_state.cheats[row_index];

                row.col(|ui| {
                    ui.checkbox(&mut cheat.enabled, "");
                });

                row.col(|ui| {
                    let name = if !cheat.name.is_empty() { &cheat.name } else { "(No name)" };
                    ui.add(Label::new(name).selectable(false));
                });

                if row.response().clicked() {
                    cheats_state.selected_cheat = row_index;
                }

                row.col(|ui| {
                    if ui.button("Delete").clicked() {
                        rows_to_remove.push(row_index);
                    }
                });
            });
        });

    for i in rows_to_remove {
        if i < cheats_state.cheats.len() {
            cheats_state.cheats.remove(i);

            if cheats_state.selected_cheat > i {
                cheats_state.selected_cheat -= 1;
            }
        }
    }
}

fn render_central_panel(ui: &mut Ui, cheat_state: &mut CheatState, cheat_just_created: bool) {
    const TEXTEDIT_WIDTH: f32 = 150.0;
    const TEXTEDIT_ROWS: usize = 10;

    ui.horizontal(|ui| {
        ui.label("Name");

        let resp = ui.add(TextEdit::singleline(&mut cheat_state.name).hint_text("New Cheat"));
        if cheat_just_created {
            resp.request_focus();
        }
    });

    ui.add_space(10.0);

    ui.label("Codes");

    ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            let resp = ui.add(
                TextEdit::multiline(&mut cheat_state.codes_buffer)
                    .font(TextStyle::Monospace)
                    .desired_width(TEXTEDIT_WIDTH)
                    .desired_rows(TEXTEDIT_ROWS),
            );

            if resp.changed() {
                cheat_state.codes.clear();
                cheat_state.code_messages.clear();

                for line in cheat_state.codes_buffer.split('\n') {
                    let line = line.trim();
                    if line.is_empty() {
                        cheat_state.code_messages.push("".into());
                        continue;
                    }

                    cheat_state.codes.push(line.into());

                    // TODO change when other consoles are supported
                    let message = genesis_code_message(line);
                    cheat_state.code_messages.push(message);
                }
            }

            ui.vertical(|ui| {
                ui.add_space(3.0);

                ui.spacing_mut().item_spacing.y = 0.0;

                for message in &cheat_state.code_messages {
                    ui.label(RichText::new(message.as_ref()).family(FontFamily::Monospace));
                }
            });
        });

        ui.add_space(5.0);

        // TODO change when other consoles are supported
        ui.label("Enter one code per line.");

        ui.add_space(5.0);

        ui.label("Supported code formats:");
        ui.label(" • Game Genie (ABCD-EGHJ)");
        ui.label(" • Action Replay (02468 ABCDE)");
        ui.label(" • Memory address/value (02468A:BCDE)");
    });
}

fn genesis_code_message(line: &str) -> Cow<'static, str> {
    const INVALID: &str = "Invalid code";

    let Some(code_type) = GenesisCheatCodeType::guess_from(line) else {
        return INVALID.into();
    };

    let Some((address, value)) = code_type.decode(line) else {
        return INVALID.into();
    };

    format!("{code_type}: ${address:06X} = 0x{value:04X}").into()
}

fn render_centered_message(ui: &mut Ui, message: &str) {
    CentralPanel::default().show_inside(ui, |ui| {
        ui.centered_and_justified(|ui| {
            ui.label(message);
        });
    });
}
