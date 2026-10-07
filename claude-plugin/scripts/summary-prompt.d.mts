export const DEFAULT_SUMMARY_PROMPT: string
export const SUMMARY_PROMPT_MAX_LENGTH: number
export function isValidSummaryPrompt(value: unknown): boolean
export function readSummaryPrompt(directory?: string): string
export function renderSummaryPrompt(template: string, status: string, report: string): string
export function createSummaryPrompt(status: string, report: string, directory?: string): string
