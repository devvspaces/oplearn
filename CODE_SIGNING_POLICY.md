# Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## What is signed

- `lectern.exe` and the Windows installers (`Lectern_*_x64-setup.exe` and `Lectern_*_x64_en-US.msi`).
- They are built from this repository by GitHub Actions ([`release.yml`](.github/workflows/release.yml)) when a `v*` tag is pushed. Nothing built on a personal machine is signed.
- The third-party tools bundled with Lectern ([yt-dlp](https://github.com/yt-dlp/yt-dlp), [FFmpeg](https://ffmpeg.org) and ffprobe) are not built by this project and are **not** signed with this certificate.

## Team roles

- **Committers and reviewers:** [devvspaces](https://github.com/devvspaces). Changes from anyone else come in through pull requests and are reviewed before merging.
- **Approvers:** [devvspaces](https://github.com/devvspaces). Every signing request is approved by hand.

All team members use multi-factor authentication on GitHub and SignPath.

## Privacy policy

Lectern has no telemetry, accounts or analytics. Your library, notes and API keys stay on your computer (keys are stored in your operating system's keychain).

Lectern connects to other systems only for the following:

- **When you ask it to:**
  - YouTube, to search, import playlists, stream or download videos and fetch transcripts.
  - Google sign-in, if you choose to connect a YouTube account.
  - The course providers you import from: MIT OpenCourseWare, NPTEL, OpenStax, LibreTexts and Open Yale Courses.
  - Anthropic's API, only if you add your own API key and click an AI action. Each request shows its estimated cost first and sends only the course material it lists.
- **Automatically:** once a day, Lectern checks GitHub for a newer release of yt-dlp and downloads it if there is one. No personal data is sent.

The services above have their own privacy policies: [Google/YouTube](https://policies.google.com/privacy), [Anthropic](https://www.anthropic.com/legal/privacy), [GitHub](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement), and those of each course provider.
