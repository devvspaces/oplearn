# Lectern

Lectern is a desktop app that turns open courses into study courses you own: YouTube playlists, MIT OpenCourseWare, NPTEL, OpenStax, LibreTexts and Open Yale Courses. Everything stays on your computer.

Built with Tauri v2 (Rust core, SQLite) and React 19 with TypeScript and Tailwind. See [SPEC.md](SPEC.md) for the full design.

## Features

- **Courses from open sources:** YouTube playlists (Data API or yt-dlp), MIT OCW (videos, notes, problem sets with solutions, exams), NPTEL (weekly lectures with transcripts), OpenStax (textbook sections with figures and math), LibreTexts (a PDF per page), Open Yale Courses (lectures with chapters, transcripts and handouts). Every course shows its license and links back to the source.
- **Study player:** the official YouTube player with our own controls and hotkeys, resume, speed, chapters, and offline downloads per course.
- **Notes:** timestamped notes, bookmarks, questions, to-dos and references; notes anchored to PDF pages; lesson and course notebooks; transcripts with search.
- **Progress and plans:** watch tracking, daily goals and streaks, study plans with rebalancing, reminders.
- **Review:** flashcards (SM-2), quizzes, and ⌘K search across transcripts, notes, cards and PDFs.
- **AI (optional, your Anthropic key):** restructure a course, study plans, flashcards, quizzes, Explain this moment or page, Ask AI, lesson summaries, reference suggestions. Every request shows its cost first and nothing is sent without a click.
- **Backups:** daily snapshots, restore, and a JSON export of the whole library.

No telemetry. API keys live only in your OS keychain.

## Install

Download the installer for your system from [Releases](https://github.com/devvspaces/oplearn/releases):

| System | File |
| --- | --- |
| macOS, Apple silicon | `Lectern_*_aarch64.dmg` |
| macOS, Intel | `Lectern_*_x64.dmg` |
| Windows | `Lectern_*_x64-setup.exe` or `.msi` |
| Linux | `Lectern_*_amd64.AppImage` or `.deb` |

The builds aren't signed by Apple or Microsoft, so the first launch needs one extra step:

- **macOS:** drag Lectern to Applications, then right-click it → **Open** → **Open**. If macOS says the app is damaged or won't open it, run `xattr -dr com.apple.quarantine /Applications/Lectern.app`, or use System Settings → Privacy & Security → **Open Anyway**.
- **Windows:** on the SmartScreen warning, click **More info** → **Run anyway**.
- **Linux:** `chmod +x Lectern_*.AppImage` and run it, or `sudo apt install ./Lectern_*.deb`.

On first launch Lectern asks where to keep your library (default `~/Lectern`), lets you add optional keys, and helps you add a first course.

### Optional keys

- **YouTube Data API key:** faster playlist imports, Creative Commons badges and search filters. Create one in the Google Cloud console with "YouTube Data API v3" enabled.
- **Anthropic API key:** the AI features. You can set a monthly spending limit in Settings → AI.

## Your data

Everything lives in one folder, `~/Lectern` by default (Settings → Data folder to move it):

- `library.db`: the SQLite database
- `media/`: downloaded videos, PDFs and imported pages
- `backups/`: daily and manual snapshots

### Start fresh or uninstall

Export a backup first if you want one (Settings → Backups → Export library…). Then quit Lectern and remove its data, settings, UI storage and keychain items.

macOS:

```sh
cat ~/Library/Application\ Support/com.lectern.app/bootstrap.json 2>/dev/null   # shows a custom library folder, if you set one
rm -rf ~/Lectern
rm -rf ~/Library/Application\ Support/com.lectern.app
rm -rf ~/Library/WebKit/com.lectern.app ~/Library/Caches/com.lectern.app
for k in youtube_api_key anthropic_api_key google_refresh_token google_oauth_client_id google_oauth_client_secret google_account_channel; do
  security delete-generic-password -s com.lectern.app -a "$k" >/dev/null 2>&1
done
rm -rf /Applications/Lectern.app   # to uninstall, or before installing a new copy
```

Windows: delete `%USERPROFILE%\Lectern`, `%APPDATA%\com.lectern.app` and `%LOCALAPPDATA%\com.lectern.app`, and remove the `com.lectern.app` entries in Credential Manager.

Linux: delete `~/Lectern`, `~/.config/com.lectern.app` and `~/.local/share/com.lectern.app`, and remove the `com.lectern.app` items from your keyring.

If you chose another library folder, delete that folder instead of `~/Lectern`.

## Development

Prerequisites: Node 24+, pnpm, Rust (stable) and the [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm fetch-binaries     # yt-dlp, ffmpeg and ffprobe for your platform (sha256-verified) into src-tauri/binaries
pnpm app                # run in dev mode (Vite on :1430)
```

| Command | What it does |
| --- | --- |
| `pnpm app` | Run the app (Tauri dev) |
| `pnpm app:build` | Build installers for the current OS (`src-tauri/target/release/bundle/`) |
| `pnpm test` | Unit tests (Vitest), Rust tests, then Playwright smoke tests |
| `pnpm test:unit` / `test:rust` / `test:e2e` | Each suite on its own |
| `pnpm lint` | ESLint + `tsc` (also run `cargo clippy --manifest-path src-tauri/Cargo.toml`) |
| `pnpm format` | Prettier + `cargo fmt` |
| `pnpm bindings` | Regenerate `src/bindings.ts` from the Rust commands |
| `pnpm seed` | Import a demo course into `~/Lectern` |
| `pnpm version:set X.Y.Z` | Set the app version everywhere before a release |

[CLAUDE.md](CLAUDE.md) has working notes on the architecture and its gotchas.

### Releases

CI (`.github/workflows/build.yml`) tests every push to `master` and builds installers as run artifacts. To publish a release:

```sh
pnpm version:set 0.2.0
git commit -am "Release v0.2.0"
git tag v0.2.0
git push origin master v0.2.0
```

The `release` workflow checks that the tag matches the version, runs the tests, builds macOS (Apple silicon and Intel), Windows and Linux installers, and attaches them to a **draft** release. Review it on GitHub and click Publish.

macOS builds are ad-hoc signed but not notarised (there's no Apple Developer account), and Windows builds are unsigned.

## Responsible use

Lectern streams through YouTube's official embedded player, and downloading is opt-in per course. Download only content you have the right to, such as Creative Commons lectures or your own videos. Imported provider content keeps its license and attribution; it's for your personal study.
