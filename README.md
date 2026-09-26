# Meeting Notes

Meeting Notes records your microphone and Windows system audio, saves both tracks on your computer, and creates a transcript and meeting notes through your OpenAI API key. You can browse meetings, search their contents, mark action items complete, and open meetings from the calendar.

Download the Windows installer or portable package from [Releases](https://github.com/abdulloev-n/meeting-notes/releases). To build the app yourself, follow [BUILD.md](BUILD.md).

## Install

Run `MeetingNotes-Setup-x64.exe` on Windows 10 or 11 (x64). The installer adds Meeting Notes to the Start Menu and Windows Installed Apps. It can create a desktop shortcut. You do not need Node.js or Rust to use the installed app.

Open **Settings → AI → Add API key**, enter your OpenAI API key, then choose **Save & test**. The app tests a replacement key before it replaces the current one. Windows Credential Manager stores the key.

Use **Start recording** on Home or press **Ctrl+Shift+R**. The same shortcut stops recording. You can change it in Settings. When you close the window during recording, the app stays in the tray. Use the tray icon to reopen it or stop recording.

Settings offers English and Russian for the interface. Set the summary language separately under **Settings → AI → Advanced AI settings**.

## Your files

The app stores its SQLite database and logs in the Windows app data directory for `com.meetingnotes.desktop`. It stores recordings in `Documents\Meeting Notes\Recordings` by default. You can change the recordings folder in Settings. Each meeting folder contains the mixed `recording.m4a` and the original `microphone.wav` and `system.wav` tracks. The app keeps both original tracks after conversion.

Meeting data stays on this computer. The app sends audio and transcript text to OpenAI only when it processes a meeting. A network or API error leaves the recording available; open the meeting and retry processing.

Uninstall through **Windows Settings → Installed Apps**. Uninstall leaves your recordings and database in place.

The optional `MeetingNotes-portable-x64.zip` runs without installation. Extract the whole archive before opening `meeting-notes.exe`; keep `bin\ffmpeg.exe` beside it. The portable package still stores meeting data in the standard Windows folders above.

## Defaults

| Setting | Value |
| --- | --- |
| Transcription model | `gpt-4o-mini-transcribe` |
| Summary model | `gpt-4o-mini` |
| Summary language | Russian |
| Global shortcut | Ctrl+Shift+R |

## Known limits

Recording and audio capture have been tested on Windows. OpenAI API connection, transcription, and summarization still need an end-to-end test with a valid API key. This first release is marked as a pre-release for that reason.

The app does not label speakers. The app transcribes 15-minute audio segments, so a long meeting can take time to process. OpenAI processing requires internet access and a valid API key. A failed audio conversion leaves the original tracks in the meeting folder.

If recording cannot start, check Windows microphone access for desktop apps and select a working microphone and output device in Settings. The app reports the device error without starting an empty meeting.
