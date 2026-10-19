<p align="center">
  <img src="assets/renamer-icon.png" width="128" alt="Renamer app icon">
</p>

<h1 align="center">Renamer</h1>
<p align="center">A local batch file renamer for macOS, built in Rust.</p>
<p align="center">Combine rules. Preview every change. Rename safely. Undo after restarting.</p>

Renamer helps turn a folder of inconsistent filenames into an organized collection. Use the desktop app for visual review, or the JSON command-line interface for scripts and local AI agents. Files stay on your computer—there is no account, server, or upload step.

```text
MySummerPhoto.jpg         → my-summer-photo.jpg
HTTPServerNotes.txt       → http_server_notes.txt
Photo (copy).png          → Photo.png
Holiday Photo.jpg         → 001_holiday_photo.jpg
```

## What it can do

- **Change case:** lowercase, UPPERCASE, Title Case, sentence case, and inverted case.
- **Format words:** camelCase, PascalCase, snake_case, SCREAMING_SNAKE_CASE, kebab-case, Train-Case, separated words, and joined words. Split camel case and acronyms, with optional letter/number boundaries.
- **Clean names:** replace literal text or regex matches; remove text, characters by position, digits, punctuation, or accents; trim, collapse, remove, or replace whitespace.
- **Add text and numbering:** prefixes, suffixes, insertion at a position, whole-name replacement, and configurable numbering with per-folder sequences.
- **Remember your rules:** the active rules and extension setting save automatically and return when you reopen the app. Rename a batch or clear the file list, then add another folder to reuse the same setup. Inputs remain editable while previews update in the background.
- **Combine rules:** reorder, duplicate, or disable rules; import/export JSON presets; start from the included presets.
- **Control scope:** select files, folders, or both; filter by extension or name; preserve extensions, including compound extensions such as `.tar.gz`.
- **Review before changing files:** live preview, conflict detection, natural sorting, manual ordering, and a checkbox for each item.
- **Undo:** persistent history, cancellation rollback, and recovery after interrupted operations.

## Requirements

The supported and tested target is **macOS on Apple Silicon**. The application uses Rust with egui/eframe and native file dialogs. It does not require Python, Qt, Electron, or a web server.

To build from source, install [Rust](https://www.rust-lang.org/tools/install) and Apple's Command Line Tools:

```sh
xcode-select --install
```

Other platforms are not currently supported or tested as complete desktop applications.

## Build and open

```sh
git clone https://github.com/icaroboaventura/renamer.git
cd renamer
./build_app.sh
open dist/Renamer.app
```

The build script creates:

| File | Purpose |
| --- | --- |
| `dist/Renamer.app` | Desktop application; can be copied into Applications |
| `dist/Renamer.zip` | Packaged application archive |

The script uses Apple's Command Line Tools and stages the signed bundle in a temporary directory to avoid file-provider metadata problems. The application is **ad-hoc signed, not notarized**. A source build is the recommended installation route; there is no notarized public installer in this repository.

For development, run the desktop app directly:

```sh
cargo run --release
```

If your selected Xcode installation requires setup, use Command Line Tools for that command:

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo run --release
```

## Desktop workflow

1. **Add files or a folder**, or drag them into the window. Adding a folder lists its immediate children by default.
2. Enable **Also rename selected folders** to include the parent folders themselves. This updates the current list immediately. Enable **Include subfolder contents** before adding a folder to scan deeper.
3. Choose filters before adding items. Hidden items are excluded by default. Symbolic-link targets and `.app` interiors are not traversed.
4. Add and configure rules on the left. Rules run from top to bottom, starting from each original name. The default target is **Name**, which preserves the extension.
5. Review the proposed names. Hover over a proposed name to see the final path, including any ancestor-folder changes. Exclude items using their checkboxes.
6. Click **Rename** when the preview is correct. Conflicts block execution. **Undo last batch / Recover** restores the latest batch, including after restarting the app.

Use **Refresh** or **Cmd+R** after external changes. Scanning and execution run in background workers, and the table renders only visible rows for large selections.

## Regex, with examples

The **Regular expression** rule includes examples, a compact syntax guide, editable pattern/replacement fields, and a test area with highlighted matches.

| Goal | Pattern | Replacement |
| --- | --- | --- |
| Remove leading numbers and separators | `^\d+[\s._-]*` | Empty |
| Normalize whitespace | `\s+` | One space |
| Remove parenthesized text | `\s*\([^)]*\)` | Empty |
| Swap “Surname, First name” | `^(.+),\s*(.+)$` | `\g<2> \g<1>` |
| Convert DD-MM-YYYY to YYYY-MM-DD | `(\d{2})-(\d{2})-(\d{4})` | `\g<3>-\g<2>-\g<1>` |

Regex uses Rust's **fancy-regex**, with Python-style replacement references such as `\g<1>`, `\1`, and `\g<name>`. `$1` is literal text. Common Python patterns work, but this is not Python's `re` runtime: advanced syntax and some Unicode behavior may differ. Always review imported presets.

Regex tests run in an isolated process with a three-second timeout. Batch previews have a 30-second limit, and the regex engine also limits backtracking.

## Command-line interface

The desktop executable also exposes a JSON CLI:

```sh
./dist/Renamer.app/Contents/MacOS/Renamer --cli catalog
```

During development, use `cargo run --release -- --cli` as the command prefix instead.

Save a request as `job.json`, replacing the example folder with an actual absolute path:

```json
{
  "paths": ["/absolute/path/to/photos"],
  "contents": true,
  "kind": "Files",
  "order": "Name",
  "rules": [
    {"kind": "Word format", "options": {"mode": "kebab-case"}},
    {"kind": "Numbering", "options": {
      "mode": "Prefix", "start": 1, "digits": 3, "separator": "-"
    }}
  ]
}
```

Preview, inspect, and apply the exact saved plan:

```sh
./dist/Renamer.app/Contents/MacOS/Renamer --cli preview --request job.json
./dist/Renamer.app/Contents/MacOS/Renamer --cli inspect --plan PLAN_ID --limit 100
./dist/Renamer.app/Contents/MacOS/Renamer --cli apply --plan PLAN_ID
./dist/Renamer.app/Contents/MacOS/Renamer --cli history
./dist/Renamer.app/Contents/MacOS/Renamer --cli undo --batch BATCH_ID
```

Use the actual IDs returned by the commands. Inspect `conflicts` and `can_apply` before applying: a successful preview response may still contain conflicts. Output is JSON; execution/validation failures return a nonzero exit code. `preview --request -` reads JSON from stdin.

Other useful commands:

```sh
# Test an expression without renaming anything
./dist/Renamer.app/Contents/MacOS/Renamer --cli regex \
  --pattern '^(\d+)-(.*)$' --replacement '\g<2>_\g<1>' --text '12-Photo'

# Open a request in the desktop UI without applying it
./dist/Renamer.app/Contents/MacOS/Renamer --cli open --request /absolute/path/job.json
```

CLI scope remains explicit: `contents: false` selects a folder itself; `contents: true` expands its contents. `recursive: true` includes deeper descendants. For isolated tests, put `--data-dir /absolute/test-state` before the subcommand.

### Local AI agents

[`integrations/renamer-local`](integrations/renamer-local) contains an optional skill for agents with local command access. Configure its executable path for your installation. The integration follows the same preview → inspect → apply workflow and uses the same undo history as the desktop app.

It does not give cloud-only chats access to local files. Renamer does not download courses, authenticate to websites, or automate browsers; downloading must finish before files are handed to it.

## Safety and storage

- Existing destinations are never overwritten. macOS moves use `renamex_np(RENAME_EXCL)`.
- Names that collide by case or Unicode normalization are conservatively blocked, even on case-sensitive volumes.
- Unique temporary names support swaps and case-only changes. Descendants move before their ancestors.
- Saved plans bind reviewed identities and metadata. Applying a plan does not rescan a folder or silently include new files.
- Each filesystem step is journaled before and after execution. Failures or cancellation trigger rollback; unresolved recovery remains in the journal.
- Undo validates identities and destination availability. It restores names, not edits to file contents.
- A lock prevents simultaneous execution against the same history directory.

History and saved plans are stored locally at `~/Library/Application Support/Renamer`. Version-1 plans and journals from the earlier Python app remain readable. Do not delete an unfinished journal before recovering it. Try unfamiliar rules on a small sample and keep backups of important files.

## Development and validation

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --release
```

Tests cover transformations, regex examples, extension handling, numbering, folder selection, collisions, swaps, nested folders, stale previews, rollback, crash recovery, persistent undo, CLI workflows, and a 10,000-file preview.

See [architecture and testing](docs/architecture.md) for the source layout and validation boundaries. The current bundled app is approximately 9–10 MiB on the development Mac; size and speed vary by build and machine.

## Contributing

Bug reports and focused pull requests are welcome. Include your macOS version, the rule sequence, and a minimal example of original and expected filenames. Use synthetic names in public reports; do not attach private files, download links with tokens, or your history journal.

For changes to filesystem behavior, include a temporary-directory regression test. File safety and recoverable undo take priority over additional rename options.

## License

[MIT](LICENSE). Dependency licenses remain those of their respective authors.
