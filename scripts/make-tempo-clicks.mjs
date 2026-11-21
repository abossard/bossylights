// Ten seconds of mono PCM. Sixteen clicks, 640 ms apart, set the tempo to 93.75 BPM.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const fixture = new URL("../crates/audio-in/tests/data/click-93_75.wav", import.meta.url);

function clickTrack() {
  const sampleRate = 8_000;
  const frameBytes = 2;
  const headerBytes = 44;
  const dataBytes = 10 * sampleRate * frameBytes;
  const wav = Buffer.alloc(headerBytes + dataBytes);

  wav.write("RIFF", 0);
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20); // PCM
  wav.writeUInt16LE(1, 22); // mono
  wav.writeUInt32LE(sampleRate, 24);
  wav.writeUInt32LE(sampleRate * frameBytes, 28);
  wav.writeUInt16LE(frameBytes, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(dataBytes, 40);

  for (let click = 0; click < 16; click++) {
    const start = 1_600 + 5_120 * click;
    for (let frame = 0; frame < 40; frame++) {
      const sample = frame % 2 === 0 ? 24_576 : -24_576;
      wav.writeInt16LE(sample, headerBytes + (start + frame) * frameBytes);
    }
  }
  return wav;
}

function check(expected) {
  let actual;
  try {
    actual = readFileSync(fixture);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    console.error(`Missing ${fixture.pathname}`);
    return false;
  }
  if (actual.length !== expected.length) {
    console.error(`Expected ${expected.length} bytes, found ${actual.length}`);
    return false;
  }
  const offset = expected.findIndex((byte, index) => byte !== actual[index]);
  if (offset === -1) return true;
  console.error(`Byte ${offset}: expected ${expected[offset]}, found ${actual[offset]}`);
  return false;
}

const wav = clickTrack();
if (process.argv.includes("--check")) {
  process.exitCode = check(wav) ? 0 : 1;
} else {
  mkdirSync(new URL(".", fixture), { recursive: true });
  writeFileSync(fixture, wav);
}
