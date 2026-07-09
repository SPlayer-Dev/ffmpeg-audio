import type { FFmpegAudioConfig, FFmpegAudioModule } from "../types.ts";

declare function createFFmpegAudio(
	config: FFmpegAudioConfig,
): Promise<FFmpegAudioModule>;
export default createFFmpegAudio;
