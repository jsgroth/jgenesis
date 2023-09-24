use crate::mainloop::{NativeEmulatorError, NativeEmulatorResult};
use jgenesis_traits::frontend;
use jgenesis_traits::frontend::{Color, EmulatorDebug};
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::WindowCanvas;
use sdl2::VideoSubsystem;
use std::time::{Duration, Instant};

pub struct CramDebug {
    canvas: WindowCanvas,
    buffer: Vec<Color>,
}

impl CramDebug {
    pub fn new<Emulator: EmulatorDebug>(video: &VideoSubsystem) -> NativeEmulatorResult<Self> {
        use sdl2::pixels::Color as SdlColor;

        let window_width = 30 * Emulator::PALETTE_LEN;
        let window_height = 30 * Emulator::NUM_PALETTES;
        let window = video.window("CRAM Debug", window_width, window_height).build()?;
        // Force software renderer because the hardware renderer will randomly segfault when multiple
        // windows are involved
        let mut canvas = window
            .into_canvas()
            .software()
            .build()
            .map_err(NativeEmulatorError::SdlCreateCanvas)?;

        canvas.set_draw_color(SdlColor::RGB(0, 0, 0));
        canvas.clear();
        canvas.present();

        let buffer =
            vec![Color::default(); (Emulator::PALETTE_LEN * Emulator::NUM_PALETTES) as usize];
        Ok(Self { canvas, buffer })
    }

    pub fn window_id(&self) -> u32 {
        self.canvas.window().id()
    }

    pub fn render<Emulator: EmulatorDebug>(
        &mut self,
        emulator: &Emulator,
    ) -> NativeEmulatorResult<()> {
        emulator.debug_cram(&mut self.buffer);

        let texture_creator = self.canvas.texture_creator();
        let mut texture = texture_creator.create_texture_streaming(
            PixelFormatEnum::RGB24,
            Emulator::PALETTE_LEN,
            Emulator::NUM_PALETTES,
        )?;

        texture
            .with_lock(None, |pixels, _pitch| {
                for (i, color) in self.buffer.iter().copied().enumerate() {
                    let start = 3 * i;
                    pixels[start] = color.r;
                    pixels[start + 1] = color.g;
                    pixels[start + 2] = color.b;
                }
            })
            .map_err(NativeEmulatorError::SdlCramDebug)?;

        self.canvas.clear();
        self.canvas.copy(&texture, None, None).map_err(NativeEmulatorError::SdlCramDebug)?;
        self.canvas.present();

        Ok(())
    }
}

pub struct VramDebug {
    canvas: WindowCanvas,
    buffer: Vec<Color>,
    palette: u8,
    num_palettes: u8,
    open_time: Instant,
}

impl VramDebug {
    pub fn new<Emulator: EmulatorDebug>(video: &VideoSubsystem) -> NativeEmulatorResult<Self> {
        use sdl2::pixels::Color as SdlColor;

        let rows = Emulator::PATTERN_TABLE_LEN / frontend::VRAM_DEBUG_ROW_LEN;
        let window_width = frontend::VRAM_DEBUG_ROW_LEN * 8 * 2;
        let window_height = rows * 8 * 2;
        let window = video.window("VRAM Debug - Palette 0", window_width, window_height).build()?;
        // Force software renderer because the hardware renderer will randomly segfault when multiple
        // windows are involved
        let mut canvas = window
            .into_canvas()
            .software()
            .build()
            .map_err(NativeEmulatorError::SdlCreateCanvas)?;

        canvas.set_draw_color(SdlColor::RGB(0, 0, 0));
        canvas.clear();
        canvas.present();

        let buffer = vec![Color::default(); (Emulator::PATTERN_TABLE_LEN * 64) as usize];
        Ok(Self {
            canvas,
            buffer,
            palette: 0,
            num_palettes: Emulator::NUM_PALETTES as u8,
            open_time: Instant::now(),
        })
    }

    pub fn window_id(&self) -> u32 {
        self.canvas.window().id()
    }

    pub fn toggle_palette(&mut self) -> NativeEmulatorResult<()> {
        // Prevent the input from triggering twice immediately after the window oppens
        if Instant::now().duration_since(self.open_time) < Duration::from_millis(300) {
            return Ok(());
        }

        self.palette = (self.palette + 1) % self.num_palettes;
        let title = format!("VRAM Debug - Palette {}", self.palette);
        self.canvas
            .window_mut()
            .set_title(&title)
            .map_err(|source| NativeEmulatorError::SdlSetWindowTitle { title, source })?;

        Ok(())
    }

    pub fn render<Emulator: EmulatorDebug>(
        &mut self,
        emulator: &Emulator,
    ) -> NativeEmulatorResult<()> {
        emulator.debug_vram(&mut self.buffer, self.palette);

        let texture_creator = self.canvas.texture_creator();
        let mut texture = texture_creator.create_texture_streaming(
            PixelFormatEnum::RGB24,
            frontend::VRAM_DEBUG_ROW_LEN * 8,
            Emulator::PATTERN_TABLE_LEN / frontend::VRAM_DEBUG_ROW_LEN * 8,
        )?;

        texture
            .with_lock(None, |pixels, _pitch| {
                for (i, color) in self.buffer.iter().copied().enumerate() {
                    let start = 3 * i;
                    pixels[start] = color.r;
                    pixels[start + 1] = color.g;
                    pixels[start + 2] = color.b;
                }
            })
            .map_err(NativeEmulatorError::SdlVramDebug)?;

        self.canvas.clear();
        self.canvas.copy(&texture, None, None).map_err(NativeEmulatorError::SdlVramDebug)?;
        self.canvas.present();

        Ok(())
    }
}
