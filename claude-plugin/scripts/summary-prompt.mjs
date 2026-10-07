import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

export const DEFAULT_SUMMARY_PROMPT = 'Report the outcome of the task you just finished in one explicit, concise spoken sentence. State what was done and any important failure or remaining blocker. Use plain English, no Markdown. Do not run tools. Output only that sentence.'
export const SUMMARY_PROMPT_MAX_LENGTH = 16384

function dataDirectory() {
  if (process.env.CIVILIZED_AGENT_DATA) return process.env.CIVILIZED_AGENT_DATA
  if (process.platform === 'win32') return join(process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'), 'CivilizedAgent')
  if (process.platform === 'darwin') return join(homedir(), 'Library', 'Application Support', 'CivilizedAgent')
  return join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'CivilizedAgent')
}

export function isValidSummaryPrompt(value) {
  return typeof value === 'string'
    && value.length > 0
    && Array.from(value).length <= SUMMARY_PROMPT_MAX_LENGTH
    && !value.includes('\0')
}

export function readSummaryPrompt(directory = dataDirectory()) {
  try {
    const settings = JSON.parse(readFileSync(join(directory, 'settings.json'), 'utf8'))
    const characterPrompt = typeof settings?.selectedCharacter === 'string'
      ? settings.characters?.[settings.selectedCharacter]?.summaryPrompt
      : undefined
    if (isValidSummaryPrompt(characterPrompt)) return characterPrompt
    return isValidSummaryPrompt(settings?.summaryPrompt) ? settings.summaryPrompt : DEFAULT_SUMMARY_PROMPT
  } catch {
    return DEFAULT_SUMMARY_PROMPT
  }
}
