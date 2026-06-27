export class AudioRenderer {
	private workletNode: AudioWorkletNode | null = null;
	private _isWorkletLoaded = false;
	private initPromise: Promise<void> | null = null;

	constructor(
		private audioCtx: AudioContext,
		private workletUrl: string,
	) {}

	/**
	 * Returns true if the AudioWorklet has been completely loaded.
	 */
	public get isWorkletLoaded(): boolean {
		return this._isWorkletLoaded;
	}

	/**
	 * Ensures the AudioWorklet module is added and the node is connected.
	 */
	public async initialize(channels: number): Promise<void> {
		if (!this.initPromise) {
			this.initPromise = this.audioCtx.audioWorklet.addModule(this.workletUrl);
		}
		await this.initPromise;
		this._isWorkletLoaded = true;

		this.destroyNode();

		this.workletNode = new AudioWorkletNode(this.audioCtx, "ffmpeg-audio", {
			outputChannelCount: [channels],
		});

		this.workletNode.connect(this.audioCtx.destination);
	}

	/**
	 * Sends the SharedArrayBuffer to the Worklet for memory sharing.
	 */
	public bindQueue(sharedBuffer: SharedArrayBuffer, channels: number): void {
		if (!this._isWorkletLoaded || !this.workletNode) {
			console.warn(
				"AudioRenderer: Cannot bind queue. Worklet is not loaded yet.",
			);
			return;
		}

		this.workletNode.port.postMessage({
			type: "INIT_SAB",
			payload: { sharedBuffer, channels },
		});
	}

	/**
	 * Resumes the AudioContext (Required by browser autoplay policies).
	 */
	public async resumeContext(): Promise<void> {
		if (!this._isWorkletLoaded) {
			console.warn(
				"AudioRenderer: Context resumed before Worklet initialization.",
			);
		}
		if (this.audioCtx.state === "suspended") {
			await this.audioCtx.resume();
		}
	}

	/**
	 * Gets properties from the AudioContext.
	 */
	public get sampleRate(): number {
		return this.audioCtx.sampleRate;
	}

	public get maxChannels(): number {
		return this.audioCtx.destination.maxChannelCount;
	}

	/**
	 * Cleans up the audio node graph.
	 */
	public destroyNode(): void {
		if (this.workletNode) {
			this.workletNode.disconnect();
			this.workletNode = null;
		}
	}
}
