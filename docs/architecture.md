# Architecture and testing

Renamer separates the UI, transformations, filesystem execution, and agent protocol.

| Source | Responsibility |
| --- | --- |
| `src/gui.rs` | Desktop interface, selection, rule editor, background jobs |
| `src/rules.rs` | Unicode name transformations, compiled rules, replacement expansion |
| `src/engine.rs` | Scanning, validation, exclusive moves, persistent journal, rollback and undo |
| `src/protocol.rs` | Saved plans, JSON request handling, isolated evaluation workers |
| `src/main.rs` | CLI routing and application startup |
| `assets/catalog.json` | Operation catalog and regex examples |
| `presets/` | Importable rule examples |
| `tests/rust_core.rs` | Transformation and filesystem regression tests |
| `tests/rust_cli.rs` | Process-level CLI and large-preview tests |
| `examples/round_icon.rs` | Deterministic rounded alpha mask for the existing artwork |

## Execution model

The GUI sends scanning and execution work to background threads. Preview and regex evaluation run in a separate process with a deadline. A preview always begins with the selected original names and applies enabled rules in order.

Execution validates the reviewed metadata, then stages sibling moves through unique temporary names. Descendant groups run before ancestor groups. The journal records pending and completed steps; rollback walks those steps in reverse using file identities and exclusive rename operations.

The desktop UI and CLI share their default history directory. A custom `--data-dir` is useful for isolated tests, but separate state directories do not coordinate access to the same selected files.

## Verified behavior and limits

The automated suite exercises temporary files only, including case-only changes, swaps, nested folders, occupied destinations, cancellation, simulated destination races, and pending-journal recovery. CLI tests invoke a new process for preview, apply, and undo and create 10,000 temporary files for the large-preview case.

The application has also been manually checked on an Apple Silicon Mac for launching the packaged app, preview/rename/undo, built-in presets, regex example results, and the parent-folder inclusion toggle. The large-preview test is not an instrumented UI frame-time benchmark. Other operating systems and all macOS versions have not been validated.

The app is locally ad-hoc signed. Notarization and a public automatic-update service are not implemented.
