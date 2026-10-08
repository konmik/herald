import { expect, test } from "bun:test"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { DEFAULT_SUMMARY_PROMPT, SUMMARY_PROMPT_MAX_LENGTH, isValidSummaryPrompt, readAnnouncementProfile } from "../../claude-plugin/scripts/summary-prompt.mjs"

const custom = "Línea Ω 😀\nReport the task outcome clearly and briefly."

test("keeps the shared default prompt exact", () => {
  expect(DEFAULT_SUMMARY_PROMPT).toBe('Report the outcome of the task you just finished in one explicit, concise spoken sentence. State what was done and any important failure or remaining blocker. Use plain English, no Markdown. Do not run tools. Output only that sentence.')
})

test("reads valid custom prompts and rereads saved changes", () => {
  const data = mkdtempSync(join(tmpdir(), "herald-summary-prompt-"))
  try {
    writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: custom }))
    expect(readAnnouncementProfile(data).prompt).toBe(custom)
    writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: "Next prompt." }))
    expect(readAnnouncementProfile(data).prompt).toBe('Next prompt.')
  } finally { rmSync(data, { recursive: true, force: true }) }
})

test("falls back for missing or invalid saved prompts", () => {
  const data = mkdtempSync(join(tmpdir(), "herald-summary-prompt-"))
  try {
    expect(readAnnouncementProfile(data)).toEqual({ prompt: DEFAULT_SUMMARY_PROMPT, characterID: undefined })
    for (const summaryPrompt of ["", "Bad\0prompt", "😀".repeat(SUMMARY_PROMPT_MAX_LENGTH + 1)] as const) {
      writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt }))
      expect(readAnnouncementProfile(data).prompt).toBe(DEFAULT_SUMMARY_PROMPT)
    }
  } finally { rmSync(data, { recursive: true, force: true }) }
})

test("uses the chosen character prompt and falls back for blank or invalid prompts", () => {
  const data = mkdtempSync(join(tmpdir(), "herald-character-prompts-"))
  try {
    const settings = {
      summaryPrompt: "Default voice.",
      selectedCharacter: "herald",
      characters: {
        herald: { selected: false, summaryPrompt: "Speak as a herald.\nReport the outcome." },
        robot: { selected: false, summaryPrompt: "Speak as a robot." },
        inherited: { selected: false, summaryPrompt: "" },
        invalid: { selected: false, summaryPrompt: "Bad\0prompt" },
      },
    }
    for (const [character, expected] of [
      ["herald", "Speak as a herald.\nReport the outcome."],
      ["robot", "Speak as a robot."],
      ["inherited", "Default voice."],
      ["invalid", "Default voice."],
    ]) {
      for (const [id, profile] of Object.entries(settings.characters)) profile.selected = id === character
      writeFileSync(join(data, "settings.json"), JSON.stringify(settings))
      expect(readAnnouncementProfile(data)).toEqual({ characterID: character, prompt: expected })
    }
    writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: settings.summaryPrompt, characters: {} }))
    expect(readAnnouncementProfile(data)).toEqual({ characterID: undefined, prompt: "Default voice." })
  } finally { rmSync(data, { recursive: true, force: true }) }
})

test("counts Unicode characters and accepts plain prompts", () => {
  expect(isValidSummaryPrompt("😀".repeat(SUMMARY_PROMPT_MAX_LENGTH))).toBe(true)
  expect(isValidSummaryPrompt("😀".repeat(SUMMARY_PROMPT_MAX_LENGTH + 1))).toBe(false)
  expect(isValidSummaryPrompt("Report the task outcome.")).toBe(true)
})

test("chooses from all characters when none are checked and restricts choices when some are checked", () => {
  const data = mkdtempSync(join(tmpdir(), "herald-character-choice-"))
  try {
    const settings = {
      selectedCharacter: "editor-only",
      characters: {
        herald: { selected: false, summaryPrompt: "Herald prompt." },
        robot: { selected: false, summaryPrompt: "Robot prompt." },
        "editor-only": { selected: false, summaryPrompt: "Editor prompt." },
      },
    }
    for (const checked of [[], ["herald"], ["herald", "robot"]]) {
      for (const [id, character] of Object.entries(settings.characters)) character.selected = checked.includes(id)
      writeFileSync(join(data, "settings.json"), JSON.stringify(settings))
      const pool = checked.length ? checked : Object.keys(settings.characters)
      for (let index = 0; index < 20; index++) {
        const profile = readAnnouncementProfile(data)
        expect(pool).toContain(profile.characterID as string)
        expect(profile.prompt).toBe(settings.characters[profile.characterID as keyof typeof settings.characters].summaryPrompt)
      }
    }
  } finally { rmSync(data, { recursive: true, force: true }) }
})
