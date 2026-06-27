import { execSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, "../../");
const webRoot = resolve(__dirname, "../");

try {
	execSync(
		"cargo build --package ffmpeg_wasm --target wasm32-unknown-emscripten --release",
		{
			cwd: projectRoot,
			stdio: "inherit",
		},
	);
} catch {
	process.exit(1);
}

const outDir = resolve(webRoot, "src/audio-core/worker/wasm");
if (!existsSync(outDir)) {
	mkdirSync(outDir, { recursive: true });
}

const filesToCopy = ["ffmpeg_wasm.js", "ffmpeg_wasm.wasm"];
const releaseDir = resolve(
	projectRoot,
	"target/wasm32-unknown-emscripten/release",
);

for (const file of filesToCopy) {
	const src = resolve(releaseDir, file);
	const dest = resolve(outDir, file);

	if (existsSync(src)) {
		copyFileSync(src, dest);
		console.log(`  ✅ Copied: ${file}`);
	} else {
		console.error(`  ⚠️ File not found: ${src}`);
		process.exit(1);
	}
}
