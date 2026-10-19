# Request examples

CLI prefix (replace the example path): `"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli`.

## Snake case and numbering

Write this as a JSON file, substituting actual absolute local paths:

```json
{
  "paths": ["/absolute/path/MyPhoto.jpg", "/absolute/path/OtherPhoto.jpg"],
  "order": "Name",
  "rules": [
    {"kind": "Remove", "target": "Name", "options": {"mode": "Accents"}},
    {"kind": "Word format", "target": "Name", "options": {"mode": "snake_case"}},
    {"kind": "Numbering", "target": "Name", "options": {
      "mode": "Prefix", "start": 1, "step": 1, "digits": 3, "separator": "_"
    }}
  ]
}
```

For folder contents set `"paths": ["/absolute/folder"]` and `"contents": true`. Add `"recursive": true` only if subfolders are in scope. `contents: false` (default) selects the folder itself. `kind` is `Files`, `Folders` or `Both`. `extensions` is a comma-separated string such as `jpg,png`. `name_filter` uses a glob such as `IMG*`.

## Regex cleanup

```json
{
  "paths": ["/absolute/path/001 - Photo (copy).jpg"],
  "rules": [
    {"kind": "Regular expression", "options": {"pattern": "^\\d+[\\s._-]*", "replacement": ""}},
    {"kind": "Regular expression", "options": {"pattern": "\\s*\\([^)]*\\)", "replacement": ""}}
  ]
}
```

Rules default to enabled and target `Name` (preserves extension). Other targets are `Extension` and `Full name`. Compound extensions are preserved by default. Python-style replacement backreferences in JSON use `"\\g<1>"`; JavaScript `$1` is not a backreference here. Run `catalog` for current supported modes and examples.

## Commands

```sh
"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli preview --request /tmp/rename-job.json
"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli inspect --plan PLAN_ID --limit 100
"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli apply --plan PLAN_ID
"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli history
"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli undo --batch BATCH_ID
```

Use actual returned IDs, not the placeholders. Send SIGINT/SIGTERM to request cancellation; apply checks between rename operations and rolls completed operations back. Avoid SIGKILL; it requires subsequent journal recovery.

Rust migration: common Python regex patterns are supported through fancy-regex, but it is not the Python `re` runtime. Check previews for advanced patterns. The app reads existing version-1 plans and undo journals.
