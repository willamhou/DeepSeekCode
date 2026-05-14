# DeepSeek-TUI Parity: TUI Attach Command

## Context

DeepSeek-TUI exposes `/attach <path>` with `/image` and `/media` aliases for
local image/video attachments. DeepSeekCode already has CLI image input support
and agent-visible `image_analyze` / `image_ocr` tools, but the TUI lacked the
slash-command entry point that lets a user attach media while composing a turn.

## Goals

- Add `attach <path>` / `/attach <path>` command palette and composer support.
- Add `image <path>` / `/image <path>` and `media <path>` / `/media <path>`
  aliases.
- Add `attach help` / `/attach help` detail rendering.
- Resolve relative paths under the selected workspace; allow absolute and
  `~/...` paths.
- Reject missing paths, non-files, and unsupported extensions.
- Insert an editable attachment reference into the composer instead of
  immediately submitting a model turn.
- For images inside the workspace, insert the workspace-relative path so the
  active agent can call `image_analyze image_path=<path>` if visual inspection
  is needed.

## Design

The command is handled entirely in `TuiApp` because its observable effect is a
local composer mutation, matching DeepSeek-TUI's "attach into input" behavior.
It does not create a `TuiAction` and does not touch the runtime store by itself.
The next normal composer submit records the attachment reference as part of the
user turn.

Supported image extensions mirror DeepSeek-TUI's local media filter: `png`,
`jpg`, `jpeg`, `gif`, `webp`, `bmp`, `tif`, `tiff`, and `ppm`. Supported video
extensions are `mp4`, `mov`, `m4v`, `webm`, `avi`, and `mkv`.

## Acceptance

- `/attach <path>` validates an existing media file and inserts an attachment
  block into the composer.
- `/image <path>` and `/media <path>` behave as aliases.
- Existing composer text is preserved and the attachment block is appended;
  when the composer itself contains the slash command, the command text is
  replaced by the attachment block.
- `/attach help` renders aliases, supported media types, and selected workspace
  context.
- Unsupported extensions are rejected without mutating the composer.
- Tests cover command-palette insertion, composer alias insertion, help detail
  rendering, and unsupported-extension rejection.
