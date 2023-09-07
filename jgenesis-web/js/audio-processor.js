import { initSync, AudioProcessor } from "../pkg/jgenesis_web.js";

class JgnesAudioProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();

        let [module, memory, audioQueue] = options.processorOptions;
        initSync(module, memory);
        this.processor = new AudioProcessor(audioQueue);
    }
    process(inputs, outputs) {
        this.processor.process(outputs[0][0], outputs[0][1]);
        return true;
    }
}

registerProcessor("audio-processor", JgnesAudioProcessor);
