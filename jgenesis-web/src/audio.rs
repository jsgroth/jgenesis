use js_sys::{Array, Atomics, SharedArrayBuffer, Uint32Array};
use std::cmp;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, AudioWorkletNode, AudioWorkletNodeOptions, ChannelCountMode};

const HEADER_LEN: u32 = 2;
const HEADER_LEN_BYTES: u32 = HEADER_LEN * 4;
const START_INDEX: u32 = 0;
const END_INDEX: u32 = 1;

const BUFFER_LEN: u32 = 8192;
const BUFFER_LEN_BYTES: u32 = BUFFER_LEN * 4;
const BUFFER_INDEX_MASK: u32 = BUFFER_LEN - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueResult {
    Successful,
    BufferFull,
}

// A very simple lock-free queue implemented using a circular buffer.
// The header contains two 32-bit integers containing the current start and exclusive end indices.
#[wasm_bindgen]
pub struct AudioQueue {
    header: SharedArrayBuffer,
    header_typed: Uint32Array,
    buffer: SharedArrayBuffer,
    buffer_typed: Uint32Array,
}

impl Default for AudioQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<JsValue> for AudioQueue {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        let array = value.dyn_into::<Array>()?;
        let header = array.get(0).dyn_into::<SharedArrayBuffer>()?;
        let buffer = array.get(1).dyn_into::<SharedArrayBuffer>()?;
        Ok(Self::from_buffers(header, buffer))
    }
}

impl AudioQueue {
    pub fn new() -> Self {
        let header = SharedArrayBuffer::new(HEADER_LEN_BYTES);
        let buffer = SharedArrayBuffer::new(BUFFER_LEN_BYTES);
        Self::from_buffers(header, buffer)
    }

    pub fn from_buffers(header: SharedArrayBuffer, buffer: SharedArrayBuffer) -> Self {
        let header_typed = Uint32Array::new(&header);
        let buffer_typed = Uint32Array::new(&buffer);
        Self { header, header_typed, buffer, buffer_typed }
    }

    pub fn push_if_space(&self, sample: f32) -> Result<EnqueueResult, JsValue> {
        let end = Atomics::load(&self.header_typed, END_INDEX)? as u32;
        let start = Atomics::load(&self.header_typed, START_INDEX)? as u32;

        if end == start.wrapping_sub(1) & BUFFER_INDEX_MASK {
            return Ok(EnqueueResult::BufferFull);
        }

        Atomics::store(&self.buffer_typed, end, sample.to_bits() as i32)?;
        let new_end = (end + 1) & BUFFER_INDEX_MASK;
        Atomics::store(&self.header_typed, END_INDEX, new_end as i32)?;

        Ok(EnqueueResult::Successful)
    }

    pub fn drain_into(&self, out: &mut Vec<f32>, limit: u32) -> Result<(), JsValue> {
        let loaded_start = Atomics::load(&self.header_typed, START_INDEX)? as u32;
        let end = Atomics::load(&self.header_typed, END_INDEX)? as u32;

        let queue_len =
            if loaded_start <= end { end - loaded_start } else { end + BUFFER_LEN - loaded_start };
        let drain_len = cmp::min(queue_len as usize, limit as usize);

        let mut start = loaded_start;
        for _ in 0..drain_len {
            let value = Atomics::load(&self.buffer_typed, start)?;
            let sample = f32::from_bits(value as u32);
            out.push(sample);

            start = (start + 1) & BUFFER_INDEX_MASK;
        }

        if start != loaded_start {
            Atomics::store(&self.header_typed, START_INDEX, start as i32)?;
        }

        Ok(())
    }

    pub fn len(&self) -> Result<u32, JsValue> {
        let end = Atomics::load(&self.header_typed, END_INDEX)? as u32;
        let start = Atomics::load(&self.header_typed, START_INDEX)? as u32;

        if start <= end { Ok(end - start) } else { Ok(end + BUFFER_LEN - start) }
    }

    fn to_js_value(&self) -> JsValue {
        Array::of2(&self.header, &self.buffer).into()
    }
}

#[wasm_bindgen]
pub struct AudioProcessor {
    audio_queue: AudioQueue,
    buffer: Vec<f32>,
}

#[wasm_bindgen]
impl AudioProcessor {
    #[wasm_bindgen(constructor)]
    pub fn new(audio_queue: JsValue) -> AudioProcessor {
        let audio_queue = AudioQueue::try_from(audio_queue).unwrap();

        AudioProcessor { audio_queue, buffer: Vec::with_capacity(BUFFER_LEN as usize) }
    }

    pub fn process(&mut self, output_l: &mut [f32], output_r: &mut [f32]) {
        self.buffer.clear();
        self.audio_queue
            .drain_into(&mut self.buffer, 2 * output_l.len() as u32)
            .expect("Unable to drain audio queue");

        for (chunk, (out_l, out_r)) in
            self.buffer.chunks_exact(2).zip(output_l.iter_mut().zip(output_r.iter_mut()))
        {
            let &[sample_l, sample_r] = chunk else { unreachable!("chunks_exact(2)") };
            *out_l = sample_l;
            *out_r = sample_r;
        }
    }
}

pub async fn initialize_audio_worklet(
    audio_ctx: &AudioContext,
    audio_queue: &AudioQueue,
) -> Result<AudioWorkletNode, JsValue> {
    // Append a random query parameter because Firefox caches this file way too aggressively and
    // Ctrl+Shift+R doesn't force a reload because it's not loaded on page load. The file itself is
    // less than 1KB and is only loaded at most once per page load, so not a big deal to not cache it.
    let module_url = format!("./js/audio-processor.js?r={}", rand::random::<u32>());
    JsFuture::from(audio_ctx.audio_worklet()?.add_module(&module_url)?).await?;

    let mut node_options = AudioWorkletNodeOptions::new();
    node_options
        .channel_count_mode(ChannelCountMode::Explicit)
        .output_channel_count(&Array::of1(&JsValue::from(2)))
        .processor_options(Some(&Array::of3(
            &wasm_bindgen::module(),
            &wasm_bindgen::memory(),
            &audio_queue.to_js_value(),
        )));

    let worklet_node =
        AudioWorkletNode::new_with_options(audio_ctx, "audio-processor", &node_options)?;
    worklet_node.connect_with_audio_node(&audio_ctx.destination())?;

    Ok(worklet_node)
}
