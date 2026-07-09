export type WorkletCommand =
	| {
			type: "INIT";
			payload: {
				sharedBuffer: SharedArrayBuffer;
				channels: number;
				wasmBytes: ArrayBuffer;
				initId: number;
			};
	  }
	| { type: "SET_TEMPO"; payload: { tempo: number } }
	| { type: "SET_PITCH"; payload: { pitch: number } }
	| { type: "SET_RATE"; payload: { rate: number } }
	| { type: "DESTROY" };

export type WorkletEvent =
	| { type: "INIT_DONE"; payload: { initId: number } }
	| { type: "AUTO_PAUSED" };
