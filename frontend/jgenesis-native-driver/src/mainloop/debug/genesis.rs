use crate::mainloop::debug;
use crate::mainloop::debug::memviewer::{MemoryViewer, MemoryViewerState};
use crate::mainloop::debug::{DebugRenderContext, DebugRenderFn};
use egui::panel::TopBottomSide;
use egui::{FontId, Grid, RichText, ScrollArea, TopBottomPanel, Ui, Vec2, Window, menu};
use egui_extras::{Column, TableBuilder};
use genesis_core::GenesisEmulator;
use jgenesis_common::frontend::{Color, ViewableBytes, ViewableWordsBigEndian};
use m68000_emu::disassembler::Disassembly;
use s32x_core::api::Sega32XEmulator;
use segacd_core::api::SegaCdEmulator;

struct State {
    vram_palette: u8,
    cram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    vram_texture: Option<(wgpu::Texture, egui::TextureId)>,
    cram_buffer: Box<[Color; 64]>,
    vram_buffer: Box<[Color; 2048 * 64]>,
    wram_viewer_state: MemoryViewerState,
    aram_viewer_state: MemoryViewerState,
    vram_viewer_state: MemoryViewerState,
    cram_viewer_state: MemoryViewerState,
    vsram_viewer_state: MemoryViewerState,
    prg_ram_viewer_state: MemoryViewerState,
    word_ram_viewer_state: MemoryViewerState,
    cdc_ram_viewer_state: MemoryViewerState,
    pcm_ram_viewer_state: MemoryViewerState,
    sdram_viewer_state: MemoryViewerState,
    fb0_viewer_state: MemoryViewerState,
    fb1_viewer_state: MemoryViewerState,
    s32x_cram_viewer_state: MemoryViewerState,
    m68k_debugger_open: bool,
    m68k_disassembly: Vec<Disassembly>,
    vram_open: bool,
    cram_open: bool,
    vdp_registers_open: bool,
}

impl State {
    fn new() -> Self {
        Self {
            vram_palette: 0,
            cram_texture: None,
            vram_texture: None,
            cram_buffer: vec![Color::default(); 64].into_boxed_slice().try_into().unwrap(),
            vram_buffer: vec![Color::default(); 2048 * 64].into_boxed_slice().try_into().unwrap(),
            wram_viewer_state: MemoryViewerState::new(),
            aram_viewer_state: MemoryViewerState::new(),
            vram_viewer_state: MemoryViewerState::new(),
            cram_viewer_state: MemoryViewerState::new(),
            vsram_viewer_state: MemoryViewerState::new(),
            prg_ram_viewer_state: MemoryViewerState::new(),
            word_ram_viewer_state: MemoryViewerState::new(),
            cdc_ram_viewer_state: MemoryViewerState::new(),
            pcm_ram_viewer_state: MemoryViewerState::new(),
            sdram_viewer_state: MemoryViewerState::new(),
            fb0_viewer_state: MemoryViewerState::new(),
            fb1_viewer_state: MemoryViewerState::new(),
            s32x_cram_viewer_state: MemoryViewerState::new(),
            m68k_debugger_open: true,
            m68k_disassembly: Vec::with_capacity(15),
            vram_open: true,
            cram_open: true,
            vdp_registers_open: false,
        }
    }
}

pub(crate) trait GenesisBase {
    fn copy_cram(&self, out: &mut [Color]);

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize);

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)]));

    fn working_ram_viewer(&mut self) -> ViewableBytes<'_>;

    fn audio_ram_viewer(&mut self) -> ViewableBytes<'_>;

    fn vram_viewer(&mut self) -> ViewableBytes<'_>;

    fn cram_viewer(&mut self) -> ViewableWordsBigEndian<'_>;

    fn vsram_viewer(&mut self) -> ViewableBytes<'_>;

    fn prg_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        None
    }

    fn word_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        None
    }

    fn pcm_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        None
    }

    fn cdc_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        None
    }

    fn sdram_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        None
    }

    fn frame_buffer_0_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        None
    }

    fn frame_buffer_1_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        None
    }

    fn s32x_cram_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        None
    }

    fn m68k_disassemble(&mut self, _out: &mut Vec<Disassembly>) {}
}

impl GenesisBase for GenesisEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        GenesisEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        GenesisEmulator::copy_vram(self, out, palette, row_len);
    }

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        GenesisEmulator::dump_vdp_registers(self, callback);
    }

    fn working_ram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_view().working_ram_view()
    }

    fn audio_ram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_view().audio_ram_view()
    }

    fn vram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_view().vram_view()
    }

    fn cram_viewer(&mut self) -> ViewableWordsBigEndian<'_> {
        self.debug_view().cram_view()
    }

    fn vsram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_view().vsram_view()
    }

    fn m68k_disassemble(&mut self, out: &mut Vec<Disassembly>) {
        let debug_view = self.debug_view();
        let pc = debug_view.m68k_pc();
        debug_view.m68k_disassemble(pc, out, 20);
    }
}

impl GenesisBase for SegaCdEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        SegaCdEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        SegaCdEmulator::copy_vram(self, out, palette, row_len);
    }

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        SegaCdEmulator::dump_vdp_registers(self, callback);
    }

    fn working_ram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_working_ram_view()
    }

    fn audio_ram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_audio_ram_view()
    }

    fn vram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_vram_view()
    }

    fn cram_viewer(&mut self) -> ViewableWordsBigEndian<'_> {
        self.debug_cram_view()
    }

    fn vsram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_vsram_view()
    }

    fn prg_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        Some(self.debug_prg_ram_view())
    }

    fn word_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        Some(self.debug_word_ram_view())
    }

    fn pcm_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        Some(self.debug_pcm_ram_view())
    }

    fn cdc_ram_viewer(&mut self) -> Option<ViewableBytes<'_>> {
        Some(self.debug_cdc_ram_view())
    }
}

impl GenesisBase for Sega32XEmulator {
    fn copy_cram(&self, out: &mut [Color]) {
        Sega32XEmulator::copy_cram(self, out);
    }

    fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        Sega32XEmulator::copy_vram(self, out, palette, row_len);
    }

    fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        Sega32XEmulator::dump_vdp_registers(self, callback);
    }

    fn working_ram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_working_ram_view()
    }

    fn audio_ram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_audio_ram_view()
    }

    fn vram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_vram_view()
    }

    fn cram_viewer(&mut self) -> ViewableWordsBigEndian<'_> {
        self.debug_cram_view()
    }

    fn vsram_viewer(&mut self) -> ViewableBytes<'_> {
        self.debug_vsram_view()
    }

    fn sdram_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        Some(self.debug_sdram_view())
    }

    fn frame_buffer_0_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        Some(self.debug_fb0_view())
    }

    fn frame_buffer_1_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        Some(self.debug_fb1_view())
    }

    fn s32x_cram_viewer(&mut self) -> Option<ViewableWordsBigEndian<'_>> {
        Some(self.debug_32x_cram_view())
    }
}

pub(crate) fn render_fn<Emulator: GenesisBase>() -> Box<DebugRenderFn<Emulator>> {
    let mut state = State::new();
    Box::new(move |ctx| render(ctx, &mut state))
}

fn render<Emulator: GenesisBase>(mut ctx: DebugRenderContext<'_, Emulator>, state: &mut State) {
    TopBottomPanel::new(TopBottomSide::Top, "genesis_debug_menu").show(ctx.egui_ctx, |ui| {
        render_menu_bar(&mut ctx, state, ui);
    });

    render_memory_viewers(&mut ctx, state);

    update_cram_texture(&mut ctx, state);
    update_vram_texture(&mut ctx, state);

    let screen_width = debug::screen_width(ctx.egui_ctx);

    render_cram_window(
        ctx.egui_ctx,
        state.cram_texture.as_ref().unwrap().1,
        screen_width,
        &mut state.cram_open,
    );

    render_vram_window(
        ctx.egui_ctx,
        &mut state.vram_palette,
        state.vram_texture.as_ref().unwrap().1,
        screen_width,
        &mut state.vram_open,
    );

    render_vdp_registers_window(ctx.egui_ctx, ctx.emulator, &mut state.vdp_registers_open);

    render_m68k_debugger_window(
        ctx.egui_ctx,
        ctx.emulator,
        &mut state.m68k_disassembly,
        &mut state.m68k_debugger_open,
    );
}

fn render_memory_viewers<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
) {
    MemoryViewer::new(
        "Working RAM Viewer",
        &mut state.wram_viewer_state,
        ctx.emulator.working_ram_viewer(),
    )
    .show(ctx.egui_ctx);
    MemoryViewer::new(
        "Audio RAM Viewer",
        &mut state.aram_viewer_state,
        ctx.emulator.audio_ram_viewer(),
    )
    .show(ctx.egui_ctx);
    MemoryViewer::new("VRAM Viewer", &mut state.vram_viewer_state, ctx.emulator.vram_viewer())
        .show(ctx.egui_ctx);
    MemoryViewer::new("CRAM Viewer", &mut state.cram_viewer_state, ctx.emulator.cram_viewer())
        .show(ctx.egui_ctx);
    MemoryViewer::new("VSRAM Viewer", &mut state.vsram_viewer_state, ctx.emulator.vsram_viewer())
        .show(ctx.egui_ctx);

    if let Some(prg_ram_viewer) = ctx.emulator.prg_ram_viewer() {
        MemoryViewer::new("PRG RAM Viewer", &mut state.prg_ram_viewer_state, prg_ram_viewer)
            .show(ctx.egui_ctx);
    }
    if let Some(word_ram_viewer) = ctx.emulator.word_ram_viewer() {
        MemoryViewer::new("Word RAM Viewer", &mut state.word_ram_viewer_state, word_ram_viewer)
            .show(ctx.egui_ctx);
    }
    if let Some(cdc_ram_viewer) = ctx.emulator.cdc_ram_viewer() {
        MemoryViewer::new("CDC Buffer RAM Viewer", &mut state.cdc_ram_viewer_state, cdc_ram_viewer)
            .show(ctx.egui_ctx);
    }
    if let Some(pcm_ram_viewer) = ctx.emulator.pcm_ram_viewer() {
        MemoryViewer::new(
            "PCM Waveform RAM Viewer",
            &mut state.pcm_ram_viewer_state,
            pcm_ram_viewer,
        )
        .show(ctx.egui_ctx);
    }
    if let Some(sdram_viewer) = ctx.emulator.sdram_viewer() {
        MemoryViewer::new("32X SDRAM Viewer", &mut state.sdram_viewer_state, sdram_viewer)
            .show(ctx.egui_ctx);
    }
    if let Some(fb0_viewer) = ctx.emulator.frame_buffer_0_viewer() {
        MemoryViewer::new("Frame Buffer 0 Viewer", &mut state.fb0_viewer_state, fb0_viewer)
            .show(ctx.egui_ctx);
    }
    if let Some(fb1_viewer) = ctx.emulator.frame_buffer_1_viewer() {
        MemoryViewer::new("Frame Buffer 1 Viewer", &mut state.fb1_viewer_state, fb1_viewer)
            .show(ctx.egui_ctx);
    }
    if let Some(s32x_cram_viewer) = ctx.emulator.s32x_cram_viewer() {
        MemoryViewer::new("32X CRAM Viewer", &mut state.s32x_cram_viewer_state, s32x_cram_viewer)
            .show(ctx.egui_ctx);
    }
}

fn render_menu_bar<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
    ui: &mut Ui,
) {
    menu::bar(ui, |ui| {
        render_memory_viewers_menu(ctx, state, ui);

        ui.menu_button("Graphics Views", |ui| {
            if ui.button("VRAM").clicked() {
                state.vram_open = true;
                ui.close_menu();
            }

            if ui.button("CRAM").clicked() {
                state.cram_open = true;
                ui.close_menu();
            }
        });

        ui.menu_button("Register Views", |ui| {
            if ui.button("VDP").clicked() {
                state.vdp_registers_open = true;
                ui.close_menu();
            }
        });
    });
}

fn render_memory_viewers_menu<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
    ui: &mut Ui,
) {
    ui.menu_button("Memory Viewers", |ui| {
        if ui.button("Working RAM").clicked() {
            state.wram_viewer_state.open = true;
            ui.close_menu();
        }

        if ui.button("Audio RAM").clicked() {
            state.aram_viewer_state.open = true;
            ui.close_menu();
        }

        if ui.button("VRAM").clicked() {
            state.vram_viewer_state.open = true;
            ui.close_menu();
        }

        if ui.button("CRAM").clicked() {
            state.cram_viewer_state.open = true;
            ui.close_menu();
        }

        if ui.button("VSRAM").clicked() {
            state.vsram_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.prg_ram_viewer().is_some() && ui.button("PRG RAM").clicked() {
            state.prg_ram_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.prg_ram_viewer().is_some() && ui.button("Word RAM").clicked() {
            state.word_ram_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.prg_ram_viewer().is_some() && ui.button("PCM RAM").clicked() {
            state.pcm_ram_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.cdc_ram_viewer().is_some() && ui.button("CDC RAM").clicked() {
            state.cdc_ram_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.sdram_viewer().is_some() && ui.button("32X SDRAM").clicked() {
            state.sdram_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.frame_buffer_0_viewer().is_some() && ui.button("Frame Buffer 0").clicked() {
            state.fb0_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.frame_buffer_1_viewer().is_some() && ui.button("Frame Buffer 1").clicked() {
            state.fb1_viewer_state.open = true;
            ui.close_menu();
        }

        if ctx.emulator.s32x_cram_viewer().is_some() && ui.button("32X CRAM").clicked() {
            state.s32x_cram_viewer_state.open = true;
            ui.close_menu();
        }
    });
}

fn render_cram_window(
    ctx: &egui::Context,
    cram_texture: egui::TextureId,
    screen_width: f32,
    open: &mut bool,
) {
    Window::new("CRAM").open(open).default_width(screen_width * 0.95).show(ctx, |ui| {
        let mut height = ui.available_width() * 0.25;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 4.0;

        ui.image((cram_texture, Vec2::new(width, height)));
    });
}

fn render_vram_window(
    ctx: &egui::Context,
    palette: &mut u8,
    vram_texture: egui::TextureId,
    screen_width: f32,
    open: &mut bool,
) {
    Window::new("VRAM").open(open).default_width(screen_width * 0.95).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Palette");

            for i in 0..4 {
                ui.radio_value(palette, i, format!("{i}"));
            }
        });

        let mut height = ui.available_width() * 0.5;
        if height > ui.available_height() {
            height = ui.available_height();
        }
        let width = height * 2.0;

        ui.image((vram_texture, Vec2::new(width, height)));
    });
}

fn render_vdp_registers_window(ctx: &egui::Context, emulator: &impl GenesisBase, open: &mut bool) {
    Window::new("VDP Registers").open(open).show(ctx, |ui| {
        ScrollArea::vertical().show(ui, |ui| {
            Grid::new("genesis_vdp_registers").num_columns(2).show(ui, |ui| {
                emulator.dump_vdp_registers(|register, values| {
                    ui.heading(register);
                    ui.end_row();

                    for &(field, value) in values {
                        ui.label(format!("  {field}:"));
                        ui.label(value);
                        ui.end_row();
                    }
                });
            });
        });
    });
}

fn render_m68k_debugger_window(
    ctx: &egui::Context,
    emulator: &mut impl GenesisBase,
    m68k_disassembly: &mut Vec<Disassembly>,
    open: &mut bool,
) {
    m68k_disassembly.clear();
    emulator.m68k_disassemble(m68k_disassembly);

    Window::new("68000 Debugger").open(open).show(ctx, |ui| {
        TableBuilder::new(ui)
            .column(Column::auto().at_least(60.0))
            .column(Column::auto().at_least(180.0))
            .column(Column::remainder().at_least(230.0))
            .body(|mut body| {
                for disassembly in m68k_disassembly {
                    body.row(15.0, |mut row| {
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format!("${:06X}", disassembly.pc & 0xFFFFFF))
                                    .font(FontId::monospace(12.0)),
                            );
                        });

                        row.col(|ui| {
                            let mut s = String::with_capacity(disassembly.words.len() * 4);
                            for word in disassembly.words.iter() {
                                if !s.is_empty() {
                                    s.push(' ');
                                }
                                s.push_str(&format!("{word:04X}"));
                            }
                            ui.label(RichText::new(s).font(FontId::monospace(12.0)));
                        });

                        row.col(|ui| {
                            ui.label(
                                RichText::new(&disassembly.string).font(FontId::monospace(12.0)),
                            );
                        });
                    });
                }
            });
    });
}

fn update_cram_texture<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
) {
    ctx.emulator.copy_cram(state.cram_buffer.as_mut());

    if state.cram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_genesis_cram", 16, 4, ctx.device, ctx.renderer);
        state.cram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.cram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.cram_buffer.as_ref()),
        ctx,
    );
}

fn update_vram_texture<Emulator: GenesisBase>(
    ctx: &mut DebugRenderContext<'_, Emulator>,
    state: &mut State,
) {
    ctx.emulator.copy_vram(state.vram_buffer.as_mut(), state.vram_palette, 64);

    if state.vram_texture.is_none() {
        let (wgpu_texture, egui_texture) =
            debug::create_texture("debug_genesis_vram", 64 * 8, 32 * 8, ctx.device, ctx.renderer);
        state.vram_texture = Some((wgpu_texture, egui_texture));
    }

    let (wgpu_texture, egui_texture) = state.vram_texture.as_ref().unwrap();

    debug::write_textures(
        wgpu_texture,
        *egui_texture,
        bytemuck::cast_slice(state.vram_buffer.as_ref()),
        ctx,
    );
}
