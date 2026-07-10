import { AudioRenderer, DecoderWorkerClient } from "./core";
import {
	allocateAudioQueueMemory,
	createMainController,
	type MainAudioController,
} from "./queue";
import {
	type EngineConfig,
	type EngineError,
	EngineErrorCode,
	type EngineErrorCodeValue,
	type EngineEventMap,
	type EngineState,
	type PlayerCover,
	type QueueConfig,
} from "./types";
import { TypedEventTarget } from "./utils";

const TIMEUPDATE_INTERVAL_MS = 250;

const DEFAULT_QUEUE_CONFIG: Required<QueueConfig> = {
	capacitySeconds: 4.0,
	notifyWatermarkSeconds: 1.0,
	emergencyWatermarkSeconds: 0.4,
};

export class FFmpegAudioEngine extends TypedEventTarget<EngineEventMap> {
	private config: EngineConfig;
	private queueConfig: Required<QueueConfig>;
	private loadSessionId = 0;
	private renderer: AudioRenderer;
	private workerClient: DecoderWorkerClient;
	private audioController: MainAudioController | null = null;
	private sharedBuffer: SharedArrayBuffer | null = null;

	private _state: EngineState = "idle";
	private _duration = 0;
	private _metadata: Record<string, string> = {};
	private _cover: PlayerCover | null = null;
	private _error: EngineError | null = null;
	private _volume = 1.0;
	private _pauseAt: number | null = null;
	private baseTime = 0;

	private _tempo = 1.0;
	private _pitch = 1.0;
	private _rate = 1.0;

	private timeupdateTimer: ReturnType<typeof setInterval> | null = null;
	private loadResolve: (() => void) | null = null;
	private loadReject:
		| ((err: { code: EngineErrorCodeValue; message: string }) => void)
		| null = null;

	constructor(config: EngineConfig) {
		super();
		this.config = config;

		this.queueConfig = {
			...DEFAULT_QUEUE_CONFIG,
			...config.queueConfig,
		};

		this.renderer = new AudioRenderer(
			config.audioContext,
			config.assets.workletUrl,
			config.assets.soundtouchWasmUrl,
			config.gainNode,
		);

		this.renderer.onMessage = (event) => {
			if (event.type === "AUTO_PAUSED") {
				this.handleAutoPaused();
			}
		};

		this.workerClient = new DecoderWorkerClient(config.assets.workerUrl, {
			onInitDone: (payload) => this.handleWorkerInitDone(payload),
			onEnded: () => this.handleWorkerEnded(),
			onError: (code, message) => this.handleError(code, message),
		});
	}

	//#region Public API
	public get state(): EngineState {
		return this._state;
	}
	public get duration(): number {
		return this._duration;
	}
	public get metadata(): Record<string, string> {
		return this._metadata;
	}
	public get cover(): PlayerCover | null {
		return this._cover;
	}
	public get error(): EngineError | null {
		return this._error;
	}

	public get volume(): number {
		return this._volume;
	}
	public set volume(val: number) {
		if (!Number.isFinite(val)) {
			console.warn("Invalid volume value ignored", val);
			return;
		}

		this._volume = Math.max(0, Math.min(1, val));

		if (this.config.gainNode) {
			const ctx = this.config.audioContext;
			this.config.gainNode.gain.setTargetAtTime(
				this._volume,
				ctx.currentTime,
				0.1,
			);
		}
	}

	public get currentTime(): number {
		if (!this.audioController) return 0;
		return (
			this.baseTime +
			this.audioController.getPlaybackIndex() / this.renderer.sampleRate
		);
	}
	public set currentTime(seconds: number) {
		if (!Number.isFinite(seconds) || seconds < 0) {
			console.warn("AudioEngine: Invalid currentTime value ignored", seconds);
			return;
		}

		if (
			this._state !== "ready" &&
			this._state !== "playing" &&
			this._state !== "paused"
		) {
			return;
		}

		this.audioController?.setSeeking(true);

		this.baseTime = seconds;

		this.syncPauseAtToAudioController();

		this.workerClient.seek(seconds);
	}

	/**
	 * Sets the target time for auto-pause.
	 *
	 * The engine will automatically pause upon reaching this time. If a seek
	 * operation occurs before this time is reached, the target remains set.
	 * @param targetSeconds The target absolute timestamp (in seconds).
	 */
	public set pauseAt(second: number | null) {
		if (second !== null && (!Number.isFinite(second) || second < 0)) {
			console.warn("Invalid pauseAt value ignored", second);
			return;
		}

		this._pauseAt = second;
		this.syncPauseAtToAudioController();
	}
	/**
	 * Gets the target time for auto-pause.
	 */
	public get pauseAt(): number | null {
		return this._pauseAt;
	}

	public get tempo(): number {
		return this._tempo;
	}
	public set tempo(val: number) {
		if (!Number.isFinite(val)) {
			console.warn("Invalid tempo value ignored", val);
			return;
		}

		this._tempo = Math.max(0.1, val);
		this.renderer.setTempo(this._tempo);
	}

	public get pitch(): number {
		return this._pitch;
	}
	public set pitch(val: number) {
		if (!Number.isFinite(val)) {
			console.warn("Invalid pitch value ignored", val);
			return;
		}

		this._pitch = Math.max(0.1, val);
		this.renderer.setPitch(this._pitch);
	}

	public get rate(): number {
		return this._rate;
	}
	public set rate(val: number) {
		if (!Number.isFinite(val)) {
			console.warn("Invalid rate value ignored", val);
			return;
		}

		this._rate = Math.max(0.1, val);
		this.renderer.setRate(this._rate);
	}

	/**
	 * Loads a file, prepares the multithreading environment, and extracts metadata.
	 */
	public async loadFile(file: File): Promise<void> {
		const currentSessionId = ++this.loadSessionId;
		this.reset();

		this._state = "loading";

		const channels = this.renderer.maxChannels;
		const sampleRate = this.renderer.sampleRate;

		await this.renderer.initialize(channels);

		if (this.loadSessionId !== currentSessionId) {
			return;
		}

		this.sharedBuffer = allocateAudioQueueMemory(
			sampleRate,
			channels,
			this.queueConfig,
		);

		this.audioController = createMainController(this.sharedBuffer);

		await this.renderer.bindQueue(
			this.sharedBuffer,
			channels,
			this._tempo,
			this._pitch,
			this._rate,
		);

		if (this.loadSessionId !== currentSessionId) {
			return;
		}

		const loadPromise = new Promise<void>((resolve, reject) => {
			this.loadResolve = resolve;
			this.loadReject = reject;
		});

		this.workerClient.init(
			file,
			sampleRate,
			channels,
			this.sharedBuffer,
			this.config.assets.ffmpegWasmUrl,
		);

		await loadPromise;
	}

	public async play(): Promise<void> {
		if (this._state !== "ready" && this._state !== "paused") {
			return;
		}
		if (!this.audioController) {
			return;
		}

		await this.renderer.resumeContext();

		this._state = "playing";
		this.audioController.play();
		this.workerClient.play();
		this.startTimeupdate();
		this.dispatch("play");
	}

	/**
	 * Pauses the playback.
	 */
	public pause(): void {
		if (this._state !== "playing") {
			return;
		}
		if (!this.audioController) {
			return;
		}

		this._state = "paused";
		this.audioController.pause();
		this.workerClient.pause();
		this.stopTimeupdate();
		this.dispatch("pause");
	}

	public destroy(): void {
		this.stopTimeupdate();
		this.workerClient.destroy();
		this.renderer.destroyNode();
		this.sharedBuffer = null;
		this.audioController = null;
		this.resetState();
		this._state = "idle";
	}

	private reset(): void {
		this.stopTimeupdate();
		this.workerClient.pause();
		this.audioController?.pause();
		this.sharedBuffer = null;
		this.audioController = null;
		this.resetState();
		this._state = "idle";
	}
	//#endregion

	//#region Internal Callbacks & Utils
	private handleWorkerInitDone(payload: {
		duration: number;
		metadata: Record<string, string>;
		coverBytes: ArrayBuffer | null;
		coverMime: string | null;
	}): void {
		this._duration = payload.duration;
		this._metadata = payload.metadata ?? {};

		if (payload.coverBytes && payload.coverBytes.byteLength > 0) {
			this._cover = { bytes: payload.coverBytes, mime: payload.coverMime };
		} else {
			this._cover = null;
		}

		this._state = "ready";

		if (this.loadResolve) {
			this.loadResolve();
			this.loadResolve = null;
			this.loadReject = null;
		}
		this.dispatch("loadedmetadata");
	}

	private handleWorkerEnded(): void {
		this.stopTimeupdate();
		this.audioController?.pause();
		this._state = "ready";
		this.dispatch("ended");
	}

	private handleError(code: EngineErrorCodeValue, message: string): void {
		this._error = { code, message };
		this.stopTimeupdate();
		this._state = "idle";

		if (this.loadReject) {
			this.loadReject({ code, message });
			this.loadResolve = null;
			this.loadReject = null;
		}

		this.dispatch("error", { code, message });
	}

	private handleAutoPaused(): void {
		this._pauseAt = null;

		if (this._state !== "playing") return;

		this._state = "paused";
		this.audioController?.pause();
		this.workerClient.pause();

		this.stopTimeupdate();
		this.dispatch("pause");
	}

	private syncPauseAtToAudioController(): void {
		if (!this.audioController) return;

		if (this._pauseAt === null) {
			this.audioController.clearPauseAtIndex();
		} else {
			let relativeTargetFrames = Math.floor(
				(this._pauseAt - this.baseTime) * this.renderer.sampleRate,
			);
			relativeTargetFrames = Math.max(0, relativeTargetFrames);

			this.audioController.setPauseAtIndex(relativeTargetFrames);
		}
	}

	private startTimeupdate(): void {
		this.stopTimeupdate();
		this.timeupdateTimer = setInterval(() => {
			this.dispatch("timeupdate");
		}, TIMEUPDATE_INTERVAL_MS);
	}

	private stopTimeupdate(): void {
		if (this.timeupdateTimer !== null) {
			clearInterval(this.timeupdateTimer);
			this.timeupdateTimer = null;
		}
	}

	private resetState(): void {
		this._error = null;
		this._metadata = {};
		this._cover = null;
		this._duration = 0;
		this.baseTime = 0;

		if (this.loadReject) {
			this.loadReject({
				code: EngineErrorCode.Aborted,
				message: "Loading aborted by subsequent operation",
			});
			this.loadResolve = null;
			this.loadReject = null;
		}
	}
	//#endregion
}
