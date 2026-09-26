import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

const source = resolve('node_modules/@ffmpeg-installer/win32-x64/ffmpeg.exe');
const target = resolve('src-tauri/bin/ffmpeg.exe');

if (!existsSync(source)) {
  throw new Error('FFmpeg is missing. Run npm install on Windows x64 first.');
}

mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
console.log('Prepared FFmpeg for the Windows build.');
