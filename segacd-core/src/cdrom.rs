pub mod cdtime;
pub mod cue;
pub mod reader;

// Data: 16 header bytes + 2048 data bytes + 288 error detection/correction bytes
// Audio: 1176 signed 16-bit PCM samples, half for the left channel and half for the right channel
pub const BYTES_PER_SECTOR: u64 = 2352;
