import { expect, test } from "bun:test"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

const { sessionTitle } = await import("../../claude-plugin/scripts/session-title.mjs")

test("reads the latest renamed Claude session title", async () => {
  const directory = await mkdtemp(join(tmpdir(), "civilized-title-"))
  try {
    const path = join(directory, "session.jsonl")
    await writeFile(path, '{"type":"custom-title","customTitle":"Old title"}\n{"type":"custom-title","customTitle":"Native adviser"}\n')
    expect(sessionTitle(path, "Default title")).toBe("Native adviser")
  } finally {
    await rm(directory, { recursive: true })
  }
})

test("missing transcripts retain the hook title", () => {
  expect(sessionTitle("missing-transcript", "Known title")).toBe("Known title")
  expect(sessionTitle("")).toBe("Untitled session")
})
