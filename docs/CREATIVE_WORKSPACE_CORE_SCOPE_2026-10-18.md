# Creative Workspace Core — scope and acceptance

This is the first product slice for Atlas-owned content creation. It covers
photo, vector, and video work in one local-first project model rather than
three disconnected apps.

## Shared project contract

- A project has an Atlas-owned manifest, asset registry, edit history, and
  recoverable snapshots.
- Source assets are immutable by default; edits create named revisions.
- Every render/export records inputs, settings, tool versions, output path,
  and warnings.
- Offline operation is the default. Network-dependent features must be
  explicitly optional and must never masquerade as completed work.

## Photo lane

- Import common raster assets.
- Non-destructive crop, resize, rotate, exposure/color adjustments, masks,
  layers, text, and simple compositing.
- Preview and deterministic render.
- Export PNG/JPEG/WebP plus a project-native reopenable form.

## Vector lane

- Import SVG and raster references.
- Create and edit paths, fills, strokes, transforms, groups, text, and simple
  layout frames.
- Produce named variants without overwriting the base composition.
- Export SVG and rasterized previews.

## Video lane

- Import local video, image, and audio assets.
- Timeline with clips, trims, splits, transitions, text overlays, captions,
  and basic audio gain/mute.
- Render a bounded preview and export a standard video format.
- Caption import, editing, timing validation, and burned-in or sidecar export.

## Recovery and quality gates

- Close and reopen a project with its history intact.
- Recover from an interrupted render without corrupting the project.
- Re-run a prior render from its recorded manifest.
- Refuse unsupported or oversized inputs with a clear reason.
- Test photo, vector, video, caption, render, export, interruption, and
  recovery paths offline before adding provider-backed assistance.

## Gate

Implementation begins after Goal 2's all-target campaign is green. Until then
this document is the accepted scope, not a claim that the capability exists.

