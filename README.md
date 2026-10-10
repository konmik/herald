# herald

A desktop herald that announces completed AI tasks with animated characters and spoken summaries.

## Demo

<img src=".github/assets/mad-hatter-resource-announcement-v1.png" alt="Mad Hatter announcing a task result" width="320">

https://github.com/user-attachments/assets/e20a64b4-677e-4c94-b022-3cb32f88c377

Recorded replay with Mad Hatter and his configured voice.

## How it works

Works with OpenCode, Claude and Codex. Tasks lasting at least one minute trigger an announcement when they finish. It waits for background jobs and stays above other windows without taking focus.

Speech is muted during meetings. Quiet hours default to 22:00–08:00 and can be changed or disabled.

See [installation instructions](INSTALLATION.md) for the Windows bundle.

## Plugins

Plugins for **OpenCode**, **Claude** and **Codex** share the announcer and settings. They summarize the existing conversation after background work finishes; subagents stay silent. The Windows installer and `pnpm run deploy:macos` register all three; Codex is registered when `codex` is on PATH.

Codex runs the plugin through lifecycle hooks and skips new or changed plugin hooks until you trust them. After installing, open `/hooks` in Codex once and trust the Herald hooks. Codex hooks cannot see background terminals, prompt typing or subagent expansion, so a Codex announcement does not wait for background jobs and is dismissed only by your next prompt. Codex has no plugin slash commands, so `/herald-status` and `/voice-dismiss` exist only in Claude.

## ElevenLabs voices

ElevenLabs gives characters their own expressive voices.

## Summary prompts

The summary prompt is editable, with optional character-specific overrides. It shapes the announcement's wording, length and personality.

Each announcement makes an extra model request using the existing conversation as context. The conversation, summary prompt and generated summary consume tokens; cost depends on the model and caching. The request uses a fork, so the prompt and summary do not add messages to the original conversation or grow its context.

With compatible ElevenLabs models, prompts can also request emotion tags for more expressive delivery. The prompt changes what the character says and how it is delivered, not the selected voice's identity.

## Offline voice selection

Optional offline speech uses Kitten CPU voices instead of ElevenLabs. It works without an internet connection and also provides a fallback when ElevenLabs is unavailable.

Local voices can vary by character or plugin; the defaults are Jasper for OpenCode and Bruno for Claude. They do not imitate the character's ElevenLabs voice.
