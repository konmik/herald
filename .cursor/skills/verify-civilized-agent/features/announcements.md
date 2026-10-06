# Task-completion announcements

Completed work produces a short animated, spoken summary without stealing focus, while the relevant host session remains open. Users can dismiss it.

## Sub-features

- Claude and OpenCode main-task completions after at least one minute.
- Subagent completion summaries fork the existing main conversation with the completed report appended.
- Background work delays completion; new messages reset the timer.
- Presence gates display; rendered messages enter history and leave the queue.
- Animation plays from the shared library, stays topmost, and preserves focus.
- Left-click dismisses the current notification; right-click exits the companion.
- Claude `/voice-dismiss [agent-id]` dismisses queued messages; `/civilized-status` reports plugin availability.

## How to get to it (user POV)

- Finish a sufficiently long task in an open Claude session.
- Finish a sufficiently long task in an open OpenCode session/tab.
- Complete delegated/background work in either host.
- Click the displayed companion, or run the Claude dismissal/status commands.

## Driving it with Win32 and the production bridge

Preconditions: baseline; shared portrait/video assets present; fresh evidence directory; no concurrent focus check.

- **Native delivery boundary.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/announce.ps1 -Evidence temp/verification/announcement-proof`. The helper submits presence and notify messages through `node claude-plugin/scripts/bridge.mjs`, the actual file transport. Require a rendered title `Verification session`, decoded animation frames, one completed message, preserved focus/passive window, exact text in `history.jsonl`, and an empty final queue.
- **Speech.** With permission to make sound, append `-Speech` and use a new evidence directory. Require one speech start and capture actual audio separately when testing sound quality.
- **Real host generation.** In a disposable real Claude or OpenCode session, perform a task lasting at least one minute, then wait for completion without submitting a new user message. Record the initiating user action, task completion, native notification and matching isolated history. Verify the announcement was appended as a fork of that session, not a standalone request. This repository has no safe scripted host-session launcher; record the host path as unverified if a disposable session cannot be established. Do not drive the user's current conversation as a fixture.
- **Other host entries.** Repeat separately for each affected main/subagent/background path, tab visibility changes, and timer reset. For Claude status/dismissal, issue `/civilized-status` and `/voice-dismiss` in that disposable session and retain the command response plus queue/visible state. Transport injection cannot prove these paths.
- **Mouse entry points.** In owned isolated playback, click the actual companion with the left or right mouse button and record disappearance plus queue state or owned process exit. Target the owned window, not a guessed screen coordinate. The helper's automatic completion does not prove either click.

## Gotchas

- Native bridge proof is downstream integration evidence, not a proof of model generation, timing, forking or host presence hooks.
- Keep presence alive; the native consumer skips sessions without recent visibility updates.
- The generated render PNG is not a desktop screenshot. Passive/focus report assertions and actual audio evidence have different scopes.
- Normal playback shares port 47863. Use `--isolated` and unique data; never send fixtures to the default inbox.
- History records shown messages, not merely submitted messages. Compare notification ID, text and title, not just file existence.
