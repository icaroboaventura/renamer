---
name: renamer-local
description: Rename local files and folders on this Mac using the installed Renamer app, including batch case conversion, snake/camel case, regex, numbering, previews, and undo. Use for requests to rename downloaded files after they have been saved locally.
---

# Renamer local

Use the existing Renamer engine through its JSON CLI. It uses the same transformations, collision checks and persistent undo history as the desktop window. It does not upload files or require UI clicking.

Configure the absolute path to the Rust executable for this installation:

```sh
"/absolute/path/to/Renamer.app/Contents/MacOS/Renamer" --cli catalog
```

The app and CLI are compiled Rust and require no Python runtime. Use the same executable for every command.

## Workflow

1. Use the user's selected paths and requested transformations. If downloads are part of the request, use the available download/browser/connector tools first and wait for completed local files. Pass those exact paths rather than scanning unrelated Downloads contents. Renamer does not itself download URLs, and a cloud attachment is not a local file path.
2. Call `catalog` when you need operation names/options. Write a JSON request to an absolute temporary file using a file tool or properly quoted heredoc. See [request examples](references.md). Names and paths are data, not shell commands.
3. Call `preview --request /absolute/job.json`. It changes no selected file names and returns a saved `plan_id`, change/conflict counts, and up to 50 mappings. Use `inspect --plan ID --offset 50 --limit 50` to page through more mappings if needed. A successful JSON envelope can contain conflicts; inspect `conflicts` and `can_apply`.
4. Check the preview against the requested scope and outcome. When the user asked to perform the rename, that instruction authorizes `apply --plan ID`; do not add another confirmation by default. A preview-only request does not authorize applying it. Respect the current host's filesystem permissions.
5. Report the actual result and `batch_id` when useful. `history` reports the latest shared GUI/CLI batch. To undo the requested batch, use `undo --batch ID`; this refuses to undo a different newer batch. For an interrupted batch, that same command attempts recovery.

Saved plans bind the reviewed file identities, names and metadata. Apply never re-scans a folder. If files changed or an apply response was lost, inspect `history` before retrying. Do not blindly repeat an applied plan. Resolve conflicts by adjusting the requested rules or selection and preview again; never bypass them with ad-hoc overwriting scripts.

To show the user the configured window, run `open --request /absolute/job.json` through the Rust executable. This populates rules and selected files but does not click Rename. Downloads, uploads, and accessing external accounts remain the responsibility of the appropriate tools and the user's request.

All commands return one JSON object to stdout (exit 0 on command success, 1 on execution/validation failure). Shared state defaults to `~/Library/Application Support/Renamer`. For isolated testing, put `--data-dir /tmp/unique-state` before the subcommand and use it consistently.

This integration requires a local execution tool on the Mac where Renamer is installed. Do not claim it is connected in cloud-only chats or hosts where this skill and local command access are unavailable.

Rust migration: common Python regex patterns are supported through fancy-regex, but it is not the Python `re` runtime. Check previews for advanced patterns. The app reads existing version-1 plans and undo journals.
