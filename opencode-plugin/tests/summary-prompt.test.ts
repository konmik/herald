import { expect, test } from "bun:test"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
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

test("counts Unicode characters and accepts plain prompts", () => {
  expect(isValidSummaryPrompt("😀".repeat(SUMMARY_PROMPT_MAX_LENGTH))).toBe(true)
  expect(isValidSummaryPrompt("😀".repeat(SUMMARY_PROMPT_MAX_LENGTH + 1))).toBe(false)
  expect(isValidSummaryPrompt("Report the task outcome.")).toBe(true)
})
