use crate::AudioCaptureFormat;
use crate::mainloop::capture::{VideoCaptureCodec, WriteExt};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use jgenesis_common::frontend::{Color, FiniteF64, FrameSize};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::{io, iter};

const FOURCC_UNCOMPRESSED_VIDEO: &[u8; 4] = b"00db";
const FOURCC_COMPRESSED_VIDEO: &[u8; 4] = b"00dc";
const FOURCC_AUDIO: &[u8; 4] = b"01wb";

#[derive(Debug, Clone, Copy)]
struct IndexEntry {
    fourcc: [u8; 4],
    keyframe: bool,
    chunk_offset: u32,
    chunk_size: u32,
}

pub struct AviWriter {
    writer: BufWriter<File>,
    video_codec: VideoCaptureCodec,
    frame_size: FrameSize,
    padded_width_bytes: u32,
    audio_format: AudioCaptureFormat,
    prev_frame: Vec<Color>,
    compressor_input: Vec<u8>,
    compressor_output: Vec<u8>,
    index: Vec<IndexEntry>,
    frame_count: u32,
    sample_count: u32,
    current_index_offset: u32,
}

impl AviWriter {
    pub fn new(
        output_path: &Path,
        video_codec: VideoCaptureCodec,
        frame_size: FrameSize,
        pixel_aspect_ratio: Option<FiniteF64>,
        fps: f64,
        audio_sample_rate: u32,
        audio_format: AudioCaptureFormat,
    ) -> io::Result<Self> {
        let file = File::create(output_path)?;
        let mut writer = BufWriter::new(file);

        write_avi_header(
            &mut writer,
            video_codec,
            frame_size,
            pixel_aspect_ratio,
            fps,
            audio_sample_rate,
            audio_format,
        )?;

        // AVI frames must be padded to a width of 4 bytes, both uncompressed and compressed
        let mut padded_width_bytes = 3 * frame_size.width;
        if !padded_width_bytes.is_multiple_of(4) {
            padded_width_bytes += 4 - (padded_width_bytes % 4);
        }

        Ok(Self {
            writer,
            video_codec,
            frame_size,
            padded_width_bytes,
            audio_format,
            prev_frame: Vec::with_capacity(frame_size.len() as usize),
            compressor_input: Vec::with_capacity(3 * frame_size.len() as usize),
            compressor_output: Vec::with_capacity(3 * frame_size.len() as usize),
            index: Vec::new(),
            frame_count: 0,
            sample_count: 0,
            current_index_offset: 4, // First chunk always begins at offset 4 ("movi" is first 4 bytes)
        })
    }

    pub fn write_video_chunk(&mut self, frame_buffer: &[Color]) -> io::Result<()> {
        let idx_entry = match self.video_codec {
            VideoCaptureCodec::None => self.write_video_chunk_uncompressed(frame_buffer)?,
            VideoCaptureCodec::CamStudio { level } => {
                self.write_video_chunk_camstudio(frame_buffer, level.clamp(1, 9))?
            }
        };
        self.index.push(idx_entry);

        self.frame_count += 1;

        // For CamStudio codec, make a keyframe every 2 seconds (leave prev_frame cleared for the next frame)
        self.prev_frame.clear();
        if matches!(self.video_codec, VideoCaptureCodec::CamStudio { .. })
            && !self.frame_count.is_multiple_of(120)
        {
            self.prev_frame.extend_from_slice(frame_buffer);
        }

        Ok(())
    }

    fn write_video_chunk_uncompressed(&mut self, frame_buffer: &[Color]) -> io::Result<IndexEntry> {
        let width_bytes = 3 * self.frame_size.width;
        let padded_width_bytes = self.padded_width_bytes;

        self.writer.write_all(FOURCC_UNCOMPRESSED_VIDEO)?;
        let chunk_size = padded_width_bytes * self.frame_size.height;
        self.writer.write_u32(chunk_size)?;

        // AVI video is bottom-to-top left-to-right, BGR888
        for row in (0..self.frame_size.height).rev() {
            let row_idx = (row * self.frame_size.width) as usize;
            for &Color { r, g, b, .. } in
                &frame_buffer[row_idx..row_idx + self.frame_size.width as usize]
            {
                self.writer.write_all(&[b, g, r])?;
            }

            for _ in width_bytes..padded_width_bytes {
                self.writer.write_all(&[0])?;
            }
        }

        let idx_entry = IndexEntry {
            fourcc: *FOURCC_UNCOMPRESSED_VIDEO,
            keyframe: true,
            chunk_offset: self.current_index_offset,
            chunk_size,
        };
        self.current_index_offset += chunk_size + 8;

        Ok(idx_entry)
    }

    fn write_video_chunk_camstudio(
        &mut self,
        frame_buffer: &[Color],
        compression_level: u32,
    ) -> io::Result<IndexEntry> {
        let width_bytes = 3 * self.frame_size.width;
        let padded_width_bytes = self.padded_width_bytes;

        let keyframe = self.prev_frame.is_empty();

        // AVI video is bottom-to-top left-to-right, BGR888
        self.compressor_input.clear();
        if keyframe {
            // For keyframes, directly store pixel colors
            for row in (0..self.frame_size.height).rev() {
                let row_idx = (row * self.frame_size.width) as usize;
                for &Color { r, g, b, .. } in
                    &frame_buffer[row_idx..row_idx + self.frame_size.width as usize]
                {
                    self.compressor_input.extend_from_slice(&[b, g, r]);
                }

                for _ in width_bytes..padded_width_bytes {
                    self.compressor_input.push(0);
                }
            }
        } else {
            // For non-keyframes, store color differences from the previous frame
            for row in (0..self.frame_size.height).rev() {
                let start_idx = (row * self.frame_size.width) as usize;
                let end_idx = start_idx + self.frame_size.width as usize;

                for (&prev_color, &color) in iter::zip(
                    &self.prev_frame[start_idx..end_idx],
                    &frame_buffer[start_idx..end_idx],
                ) {
                    self.compressor_input.extend_from_slice(&[
                        color.b.wrapping_sub(prev_color.b),
                        color.g.wrapping_sub(prev_color.g),
                        color.r.wrapping_sub(prev_color.r),
                    ]);
                }

                for _ in width_bytes..padded_width_bytes {
                    self.compressor_input.push(0);
                }
            }
        }

        self.compressor_output.clear();
        let mut encoder =
            ZlibEncoder::new(&mut self.compressor_output, Compression::new(compression_level));
        encoder.write_all(&self.compressor_input)?;
        encoder.finish()?;

        self.writer.write_all(FOURCC_COMPRESSED_VIDEO)?;
        let chunk_size = 2 + self.compressor_output.len() as u32;
        self.writer.write_u32(chunk_size)?;

        // Set bit 1 to indicate compression is zlib
        let header_byte = ((compression_level as u8) << 4) | (1 << 1) | u8::from(keyframe);
        self.writer.write_all(&[header_byte])?;
        self.writer.write_all(&[8])?; // Bits per color component? Decoders seem to ignore this byte
        self.writer.write_all(&self.compressor_output)?;

        let idx_entry = IndexEntry {
            fourcc: *FOURCC_COMPRESSED_VIDEO,
            keyframe,
            chunk_offset: self.current_index_offset,
            chunk_size,
        };
        self.current_index_offset += chunk_size + 8;

        // AVI requires that all chunks are padded to a multiple of 2 bytes
        // Padding should not be included in the stored chunk size
        if !chunk_size.is_multiple_of(2) {
            self.writer.write_all(&[0])?;
            self.current_index_offset += 1;
        }

        Ok(idx_entry)
    }

    pub fn write_audio_chunk(&mut self, samples: &[(f32, f32)]) -> io::Result<()> {
        if samples.is_empty() {
            return Ok(());
        }

        self.writer.write_all(FOURCC_AUDIO)?;
        let chunk_size = 2 * self.audio_format.bytes_per_sample() * samples.len() as u32;
        self.writer.write_u32(chunk_size)?;

        self.audio_format.encode_samples(samples, &mut self.writer)?;

        self.sample_count += samples.len() as u32;

        self.index.push(IndexEntry {
            fourcc: *FOURCC_AUDIO,
            keyframe: false,
            chunk_offset: self.current_index_offset,
            chunk_size,
        });
        self.current_index_offset += chunk_size + 8;

        Ok(())
    }

    pub fn finish(self) -> io::Result<()> {
        finalize_avi_file(self.writer, &self.index, self.frame_count, self.sample_count)
    }

    pub fn current_file_len(&self) -> u32 {
        self.current_index_offset + 396
    }
}

fn write_avi_header(
    writer: &mut BufWriter<File>,
    video_codec: VideoCaptureCodec,
    frame_size: FrameSize,
    pixel_aspect_ratio: Option<FiniteF64>,
    fps: f64,
    audio_sample_rate: u32,
    audio_format: AudioCaptureFormat,
) -> io::Result<()> {
    let aspect_ratio = determine_screen_aspect_ratio(frame_size, pixel_aspect_ratio);

    // File header
    writer.write_all(b"RIFF")?;
    writer.write_u32(0)?; // File size, fill in later
    writer.write_all(b"AVI ")?;

    // Header list header
    writer.write_all(b"LIST")?;
    writer.write_u32(368)?; // Header list size
    writer.write_all(b"hdrl")?;

    // AVI main header
    writer.write_all(b"avih")?;
    writer.write_u32(56)?; // AVI main header size
    writer.write_u32((1e6 / fps).round() as u32)?; // Microseconds per frame
    writer.write_u32(0)?; // Max bytes per frame, ignore
    writer.write_u32(0)?; // Padding granularity, ignore
    writer.write_u32(0x110)?; // Flags (AVIF_HASINDEX + AVIF_ISINTERLEAVED)
    writer.write_u32(0)?; // Total frames, fill in later
    writer.write_u32(0)?; // Initial frame
    writer.write_u32(2)?; // Number of streams (2 for video+audio)
    writer.write_u32(0)?; // Suggested buffer size, ignore
    writer.write_u32(frame_size.width)?; // Width in pixels
    writer.write_u32(frame_size.height)?; // Height in pixels
    writer.write_all(&[0; 16])?; // Reserved

    // Video AVI stream header
    writer.write_all(b"LIST")?;
    writer.write_u32(192)?; // Video header list size
    writer.write_all(b"strl")?;
    writer.write_all(b"strh")?;
    writer.write_u32(56)?; // strh chunk size
    writer.write_all(b"vids")?;
    writer.write_all(&video_codec.fourcc())?; // Data handler
    writer.write_u32(0)?; // Flags
    writer.write_u16(0)?; // Priority
    writer.write_u16(0)?; // Language tag
    writer.write_u32(0)?; // Initial frames / audio skew
    writer.write_u32(1_000_000)?; // Frame rate scale
    writer.write_u32((fps * 1e6).round() as u32)?; // Frame rate
    writer.write_u32(0)?; // Start time
    writer.write_u32(0)?; // Length in frames, fill in later
    writer.write_u32(0)?; // Suggested buffer size, ignore
    writer.write_u32(!0)?; // Quality
    writer.write_u32(0)?; // Sample size, only required if multiple frames per video chunk
    writer.write_all(&[0; 8])?; // Destination rectangle, ignore

    // Bitmap info
    writer.write_all(b"strf")?;
    writer.write_u32(40)?; // strf chunk size
    writer.write_u32(40)?; // Bitmap info size
    writer.write_u32(frame_size.width)?; // Width in pixels
    writer.write_u32(frame_size.height)?; // Height in pixels
    writer.write_u16(1)?; // Number of planes
    writer.write_u16(24)?; // Bits per pixel
    writer.write_all(&video_codec.fourcc())?; // Compression
    writer.write_u32(4 * frame_size.len())?; // Image size in bytes
    writer.write_u32(0)?; // Target device horizontal resolution, ignore
    writer.write_u32(0)?; // Target device vertical resolution, ignore
    writer.write_u32(0)?; // Number of color table indices used, ignore
    writer.write_u32(0)?; // Number of important color table indices, ignore

    match aspect_ratio {
        Some((aspect_x, aspect_y)) => {
            // AVI 2.0 / OpenDML video properties
            writer.write_all(b"vprp")?;
            writer.write_u32(68)?; // Properties chunk size
            writer.write_u32(0)?; // Video format token (0 = unknown)
            writer.write_u32(0)?; // Video standard (0 = unknown)
            writer.write_u32(if fps > 55.0 { 60 } else { 50 })?; // Vertical refresh rate in Hz
            writer.write_u32(frame_size.width)?; // Horizontal total
            writer.write_u32(frame_size.height)?; // Vertical total
            writer.write_u16(aspect_y)?; // Screen aspect ratio Y
            writer.write_u16(aspect_x)?; // Screen aspect ratio X
            writer.write_u32(frame_size.width)?; // Width in pixels
            writer.write_u32(frame_size.height)?; // Height in pixels
            writer.write_u32(1)?; // Fields per frame

            // Field info
            writer.write_u32(frame_size.width)?; // Compressed bitmap width
            writer.write_u32(frame_size.height)?; // Compressed bitmap height
            writer.write_u32(frame_size.width)?; // Valid bitmap width
            writer.write_u32(frame_size.height)?; // Valid bitmap height
            writer.write_u32(0)?; // Valid bitmap X offset
            writer.write_u32(0)?; // Valid bitmap Y offset
            writer.write_u32(0)?; // Valid X offset
            writer.write_u32(0)?; // Valid Y start line
        }
        None => {
            // Insert a junk chunk so the rest of the header is always located at the same offsets
            writer.write_all(b"JUNK")?;
            writer.write_u32(68)?; // Junk chunk size
            writer.write_all(&[0; 68])?; // Junk data
        }
    }

    // Audio AVI stream header
    writer.write_all(b"LIST")?;
    writer.write_u32(92)?; // Audio stream header list size
    writer.write_all(b"strl")?;
    writer.write_all(b"strh")?;
    writer.write_u32(56)?; // strh chunk size
    writer.write_all(b"auds")?;
    writer.write_u32(0)?; // Data handler
    writer.write_u32(0)?; // Flags
    writer.write_u16(0)?; // Priority
    writer.write_u16(0)?; // Language tag
    writer.write_u32(0)?; // Initial frames / audio skew
    writer.write_u32(4)?; // Sample rate scale
    writer.write_u32(4 * audio_sample_rate)?; // Sample rate
    writer.write_u32(0)?; // Start time
    writer.write_u32(0)?; // Length in samples, fill in later
    writer.write_u32(0)?; // Suggested buffer size, ignore
    writer.write_u32(!0)?; // Quality
    writer.write_u32(2 * audio_format.bytes_per_sample())?; // Sample size
    writer.write_all(&[0; 8])?; // Destination rectangle, ignore

    // Wave format
    writer.write_all(b"strf")?;
    writer.write_u32(16)?; // strf chunk size
    writer.write_u16(audio_format as u16)?; // Audio format type
    writer.write_u16(2)?; // Number of channels
    writer.write_u32(audio_sample_rate)?; // Samples per second
    writer.write_u32(2 * audio_format.bytes_per_sample() * audio_sample_rate)?; // Bytes per second
    writer.write_u16(2 * audio_format.bytes_per_sample() as u16)?; // Block alignment
    writer.write_u16(8 * audio_format.bytes_per_sample() as u16)?; // Bits per sample

    // Stream data header
    writer.write_all(b"LIST")?;
    writer.write_u32(0)?; // Stream data list size, fill in later
    writer.write_all(b"movi")?;

    Ok(())
}

fn determine_screen_aspect_ratio(
    frame_size: FrameSize,
    pixel_aspect_ratio: Option<FiniteF64>,
) -> Option<(u16, u16)> {
    let screen_aspect_ratio =
        pixel_aspect_ratio?.get() * f64::from(frame_size.width) / f64::from(frame_size.height);

    let mut closest_denominator = 1.0;
    let mut closest_int_diff = 1.0;

    // This is really dumb and (potentially) slow, but it only runs once when starting video capture
    for denominator in 1..=1000 {
        let denominator: f64 = denominator.into();
        let numerator = screen_aspect_ratio * denominator;

        let int_diff = (numerator - numerator.round()).abs();
        if int_diff < 1e-5 {
            // Close enough
            return Some((numerator.round() as u16, denominator.round() as u16));
        }

        if int_diff < closest_int_diff {
            closest_denominator = denominator;
            closest_int_diff = int_diff;
        }
    }

    let closest_numerator = screen_aspect_ratio * closest_denominator;
    Some((closest_numerator.round() as u16, closest_denominator.round() as u16))
}

fn finalize_avi_file(
    mut writer: BufWriter<File>,
    index: &[IndexEntry],
    frame_count: u32,
    sample_count: u32,
) -> io::Result<()> {
    writer.flush()?;
    let file_len_before_index = writer.get_ref().metadata()?.len() as u32;

    // Write index to end of file
    writer.write_all(b"idx1")?;
    writer.write_u32(16 * index.len() as u32)?; // Chunk size

    for &entry in index {
        writer.write_all(&entry.fourcc)?;
        writer.write_u32(u32::from(entry.keyframe) << 4)?; // Flags (AVIIF_KEYFRAME)
        writer.write_u32(entry.chunk_offset)?;
        writer.write_u32(entry.chunk_size)?;
    }

    let mut file = writer.into_inner()?;
    let file_len = file.metadata()?.len() as u32;

    // Set header values that were initially left empty
    file.seek(SeekFrom::Start(4))?;
    file.write_u32(file_len - 8)?; // File size

    file.seek(SeekFrom::Start(48))?;
    file.write_u32(frame_count)?; // Total frames (AVI main header)

    file.seek(SeekFrom::Start(140))?;
    file.write_u32(frame_count)?; // Length in frames (video stream header)

    file.seek(SeekFrom::Start(340))?;
    file.write_u32(sample_count)?; // Length in samples (audio stream header)

    file.seek(SeekFrom::Start(392))?;
    file.write_u32(file_len_before_index - 396)?; // Stream data list size

    Ok(())
}
