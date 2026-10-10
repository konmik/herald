# Task-completion announcements

Completed work produces a short animated, spoken summary without stealing focus, while the relevant host session remains open. Users can dismiss it.

## Sub-features

- Claude and OpenCode main-task completions after at least one minute.
- Subagents stay silent; the final main-task summary forks the existing conversation.
- Background work delays completion; new messages reset the timer.
- Presence gates display; rendered messages enter history and leave the queue.
- Animation plays from the shared library, stays topmost, and preserves focus.
- Left-click dismisses the current notification; right-click exits the companion.
- Claude `/voice-dismiss [agent-id]` dismisses queued messages.

## How to get to it (user POV)

- Finish a sufficiently long task in an open Claude session.
- Finish a sufficiently long task in an open OpenCode session/tab.
- Complete delegated/background work in either host.
- Click the displayed companion, or run the Claude dismissal/status commands.

## Driving it with Win32 and the production bridge

Preconditions: baseline; shared portrait/video assets present; fresh evidence directory; no concurrent focus check.

- **Native delivery boundary.** Run `pwsh -NoProfile -File .claude/skills/verify-herald/scripts/announce.ps1 -Evidence temp/verification/announcement-proof`. The helper submits presence and notify messages through the native announcer `--bridge` mode, the actual file transport. Require a rendered title `Verification session`, decoded animation frames, one completed message, preserved focus/passive window, exact text in `history.jsonl`, and an empty final queue.
- **Speech.** With permission to make sound, append `-Speech` and use a new evidence directory. Require one speech start and capture actual audio separately when testing sound quality.
- **Real host generation.** Run `npm run test:hosts -- -HostName OpenCode -Model provider/model -Scenario Background` or `npm run test:hosts -- -HostName Claude -Scenario Background`. These make real model requests using existing credentials. Run `Subagent` and `CancelRestart` separately for either host. The runner owns a silent isolated companion and scratch host state, records original-context generation/fork calls, and requires one rendered main-session announcement after background work and the final report. OpenCode uses a private server registration and scratch configuration, with the existing credential database; only its newly created sessions are removed. Its minimum duration is zero for these timing tests; unit tests cover the one-minute threshold. Claude tests keep the real one-minute threshold. Keep transcripts local. A missing credential, denied tool, or model that does not follow the recipe is a failed/unverified entry, not a pass.
- **Other host entries.** Repeat separately for each affected main/subagent/background path, tab visibility changes, and timer reset. For Claude dismissal, issue `/voice-dismiss` in that disposable session and retain the command response plus queue/visible state. Transport injection cannot prove these paths.
- **Mouse entry points.** In owned isolated playback, click the actual companion with the left or right mouse button and record disappearance plus queue state or owned process exit. Target the owned window, not a guessed screen coordinate. The helper's automatic completion does not prove either click.

## Gotchas

- Native bridge proof is downstream integration evidence, not a proof of model generation, timing, forking or host presence hooks.
- Keep presence alive; the native consumer skips sessions without recent visibility updates.
- The generated render PNG is not a desktop screenshot. Passive/focus report assertions and actual audio evidence have different scopes.
- Normal playback shares port 47863. Use `--isolated` and unique data; never send fixtures to the default inbox.
- History records shown messages, not merely submitted messages. Compare notification ID, text and title, not just file existence.
