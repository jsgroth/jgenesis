use crate::mainloop::capture::{AudioCaptureFormat, WriteExt};
use std::fs::File;
use std::io;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

pub struct WaveWriter {
    writer: BufWriter<File>,
    format: AudioCaptureFormat,
}

impl WaveWriter {
    pub fn new(
        output_path: &Path,
        sample_rate: u32,
        format: AudioCaptureFormat,
    ) -> io::Result<Self> {
        let file = File::create(output_path)?;
        let mut writer = BufWriter::new(file);

        write_wav_header(&mut writer, sample_rate, format)?;

        Ok(Self { writer, format })
    }

    pub fn write_audio_chunk(&mut self, samples: &[(f32, f32)]) -> io::Result<()> {
        self.format.encode_samples(samples, &mut self.writer)
    }

    pub fn finish(self) -> io::Result<()> {
        finalize_wav_file(self.writer)
    }
}

fn write_wav_header(
    writer: &mut impl Write,
    sample_rate: u32,
    format: AudioCaptureFormat,
) -> io::Result<()> {
    // File header
    writer.write_all(b"RIFF")?;
    writer.write_u32(0)?; // File size, fill in later
    writer.write_all(b"WAVE")?;

    // Format chunk
    writer.write_all(b"fmt ")?;
    writer.write_u32(16)?; // Format chunk size
    writer.write_u16(format as u16)?; // Sample format
    writer.write_u16(2)?; // Number of channels
    writer.write_u32(sample_rate)?; // Samples per second
    writer.write_u32(2 * format.bytes_per_sample() * sample_rate)?; // Bytes per second
    writer.write_u16(2 * format.bytes_per_sample() as u16)?; // Block alignment
    writer.write_u16(8 * format.bytes_per_sample() as u16)?; // Bits per sample

    // Data chunk header
    writer.write_all(b"data")?;
    writer.write_u32(0)?; // Data chunk size, fill in later

    Ok(())
}

fn finalize_wav_file(writer: BufWriter<File>) -> io::Result<()> {
    let mut file = writer.into_inner()?;
    let file_len = file.metadata()?.len() as u32;

    // Fill in header values that were initially left blank
    file.seek(SeekFrom::Start(4))?;
    file.write_u32(file_len - 8)?; // File size

    file.seek(SeekFrom::Start(40))?;
    file.write_u32(file_len - 44)?; // Data chunk size

    Ok(())
}
