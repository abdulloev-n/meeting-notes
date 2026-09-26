# Build Meeting Notes

## Requirements

- Windows 10 or 11, x64
- Node.js 20 or later
- Rust stable MSVC toolchain
- Visual Studio C++ Build Tools and Windows SDK
- WebView2 runtime for development and app use
- Network access for the first dependency install and installer tool download

The build copies FFmpeg from the `@ffmpeg-installer/win32-x64` dependency to `src-tauri/bin/ffmpeg.exe`. Git does not track that executable. See `THIRD_PARTY_NOTICES.md`.

## Commands

```powershell
npm install
npm run dev
```

Build the Windows installer:

```powershell
npm run build
npx tauri build --bundles nsis
```

Tauri writes the installer under `src-tauri/target/release/bundle/nsis/`. The compiled application is `src-tauri/target/release/meeting-notes.exe`.

The application database uses SQLite migrations on launch. The Rust service layer owns audio capture, database access, API requests, credential storage, tray behavior, and the global shortcut. React calls Tauri commands and never reads the API key.
