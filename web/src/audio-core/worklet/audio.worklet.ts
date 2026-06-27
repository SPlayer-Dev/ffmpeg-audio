import { type AudioReader, createAudioReader } from "../queue";

class FFmpegAudioProcessor extends AudioWorkletProcessor {
	private audioReader: AudioReader | null = null;

	constructor() {
		super();
		this.port.onmessage = (event) => {
			if (event.data.type === "INIT_SAB") {
				const { sharedBuffer, channels } = event.data.payload;
				this.audioReader = createAudioReader(sharedBuffer, channels);
			}
		};
	}

	process(
		_inputs: Float32Array[][],
		outputs: Float32Array[][],
		_parameters: Record<string, Float32Array>,
	): boolean {
		if (this.audioReader && outputs[0]?.[0]) {
			const outLen = outputs[0][0].length;
			this.audioReader.read(outputs, outLen);
		}

		return true;
	}
}

registerProcessor("ffmpeg-audio", FFmpegAudioProcessor);
