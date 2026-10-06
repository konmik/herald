# Audio preview

Users try draft volume, output and CPU/GPU settings without applying them, then stop the example or let it finish.

## Sub-features

- Zero volume reports a silent preview.
- Play example becomes Stop example; Stop cancels playback.
- Preview uses draft audio settings without saving them.
- CPU and optional GPU examples finish and release resources.

## How to get to it (user POV)

Open settings, adjust audio preferences and choose `Play example`. Choose `Stop example` to cancel. Apply is separate.

## Driving it with Win32

Preconditions: baseline; permission to play sound for the audible branch.

- **Silent preview.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/verify.ps1 -Feature Preview -Evidence temp/verification/preview-silent`. Status becomes `Preview is silent at 0% volume.` Saved settings bytes remain unchanged.
- **Start and stop.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/verify.ps1 -Feature Preview -Audible -Evidence temp/verification/preview-audible`. At draft 35% on System default, the button changes to `Stop example`; a second click reports `Preview stopped.` Controls and unchanged persisted bytes are retained as evidence.
- **Finish and GPU.** The existing `development_tools/verify-settings.ps1` shows the concrete completion recipe: click control 112, poll status control 109 for `Preview finished.`, then select GPU control 113 and repeat. It can play sound and needs the prepared GPU runtime. Add evidence-preserving cleanup before using it for proof; this shipped helper does not claim those branches.

## Gotchas

- Immediate cancellation proves start/stop controls, not complete synthesized speech or audible quality.
- Zero-volume proof must observe unchanged saved bytes, not assume preview never saves.
- GPU runtime preparation is separate and can download large dependencies; do not install it just for silent verification.
