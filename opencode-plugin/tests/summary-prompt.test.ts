import { expect, test } from "bun:test"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { DEFAULT_SUMMARY_PROMPT, SUMMARY_PROMPT_MAX_LENGTH, isValidSummaryPrompt, readSummaryPrompt } from "../../claude-plugin/scripts/summary-prompt.mjs"

const custom = "Línea Ω 😀\nReport the task outcome clearly and briefly."

test("keeps the shared default prompt exact", () => {
  expect(DEFAULT_SUMMARY_PROMPT).toBe('Report the outcome of the task you just finished in one explicit, concise spoken sentence. State what was done and any important failure or remaining blocker. Use plain English, no Markdown. Do not run tools. Output only that sentence.')
})

test("reads valid custom prompts and rereads saved changes", () => {
  const data = mkdtempSync(join(tmpdir(), "civilized-summary-prompt-"))
  try {
    writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: custom }))
    expect(readSummaryPrompt(data)).toBe(custom)
    writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: "Next prompt." }))
    expect(readSummaryPrompt(data)).toBe('Next prompt.')
  } finally { rmSync(data, { recursive: true, force: true }) }
})

test("falls back for missing or invalid saved prompts", () => {
  const data = mkdtempSync(join(tmpdir(), "civilized-summary-prompt-"))
  try {
    expect(readSummaryPrompt(data)).toBe(DEFAULT_SUMMARY_PROMPT)
    for (const summaryPrompt of ["", "Bad\0prompt", "😀".repeat(SUMMARY_PROMPT_MAX_LENGTH + 1)] as const) {
      writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt }))
      expect(readSummaryPrompt(data)).toBe(DEFAULT_SUMMARY_PROMPT)
    }
  } finally { rmSync(data, { recursive: true, force: true }) }
})

test("uses the selected character prompt and falls back for blank or unavailable profiles", () => {
  const data = mkdtempSync(join(tmpdir(), "civilized-character-prompts-"))
  try {
    const settings = {
      summaryPrompt: "Default voice.",
      selectedCharacter: "herald",
      characters: {
        herald: { summaryPrompt: "Speak as a herald.\nReport the outcome." },
        robot: { summaryPrompt: "Speak as a robot." },
        inherited: { summaryPrompt: "" },
        invalid: { summaryPrompt: "Bad\0prompt" },
      },
    }
    for (const [character, expected] of [
      ["herald", "Speak as a herald.\nReport the outcome."],
      ["robot", "Speak as a robot."],
      ["inherited", "Default voice."],
      ["invalid", "Default voice."],
      ["missing", "Default voice."],
    ]) {
      settings.selectedCharacter = character
      writeFileSync(join(data, "settings.json"), JSON.stringify(settings))
      expect(readSummaryPrompt(data)).toBe(expected)
      const bridge = Bun.spawnSync(["node", fileURLToPath(new URL("../../claude-plugin/scripts/bridge.mjs", import.meta.url))], {
        env: { ...process.env, CIVILIZED_AGENT_DATA: data },
        stdin: new TextEncoder().encode(JSON.stringify({ type: "read-summary-prompt" })),
      })
      expect(bridge.exitCode).toBe(0)
      expect(JSON.parse(bridge.stdout.toString())).toBe(expected)
    }
    writeFileSync(join(data, "settings.json"), JSON.stringify({ ...settings, selectedCharacter: null }))
    expect(readSummaryPrompt(data)).toBe("Default voice.")
  } finally { rmSync(data, { recursive: true, force: true }) }
})

test("counts Unicode characters and accepts plain prompts", () => {
  expect(isValidSummaryPrompt("😀".repeat(SUMMARY_PROMPT_MAX_LENGTH))).toBe(true)
  expect(isValidSummaryPrompt("😀".repeat(SUMMARY_PROMPT_MAX_LENGTH + 1))).toBe(false)
  expect(isValidSummaryPrompt("Report the task outcome.")).toBe(true)
})
