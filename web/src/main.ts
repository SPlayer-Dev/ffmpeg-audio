import { FFmpegAudioEngine } from "./audio-core";
import workerUrl from "./audio-core/worker/decoder.worker.ts?worker&url";
import wasmUrl from "./audio-core/worker/wasm/ffmpeg_wasm.wasm?url";
import workletUrl from "./audio-core/worklet/audio.worklet.ts?worker&url";
import { AppUI } from "./ui.ts";

async function bootstrap() {
	const AudioContextClass =
		// biome-ignore lint/suspicious/noExplicitAny: For compatibility
		window.AudioContext || (window as any).webkitAudioContext;
	const audioCtx = new AudioContextClass();
	await audioCtx.suspend();

	const engine = new FFmpegAudioEngine({
		audioContext: audioCtx,
		assets: {
			workerUrl,
			workletUrl,
			wasmUrl,
		},
	});

	new AppUI(engine);
}

bootstrap().catch((err) => {
	console.error("Application failed to bootstrap:", err);
});
