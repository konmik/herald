import { strict as assert } from "node:assert"
import { writeFile } from "node:fs/promises"
import { join } from "node:path"
import { Service } from "@opencode/client/service"
import { OpenCode } from "@opencode/client"

const [scratch, app, evidence] = process.argv.slice(2)
if (!scratch || !app || !evidence) throw new Error("Missing bootstrap verification arguments")
const file = join(scratch, "state/opencode/service.json")
const server = Bun.spawn(["pwsh", "-NoProfile", "-Command", "opencode serve --service --port 0 --log-level debug"], {
  cwd: scratch,
  stdout: Bun.file(join(evidence, "server-output.txt")),
  stderr: Bun.file(join(evidence, "server-errors.txt")),
})
try {
  const deadline = Date.now() + 30_000
  let endpoint: Awaited<ReturnType<typeof Service.discover>>
  while (!endpoint && Date.now() < deadline) {
    if (server.exitCode !== null) throw new Error("Private bootstrap server exited")
    endpoint = await Service.discover({ file })
    if (!endpoint) await Bun.sleep(100)
  }
  if (!endpoint) throw new Error("Private bootstrap server did not start")
  const client = OpenCode.make({ baseUrl: endpoint.url, headers: { ...Service.headers(endpoint), "x-opencode-directory": scratch } })
  const session = await client.session.create({ location: { directory: scratch }, title: "Installed package bootstrap" }, { signal: AbortSignal.timeout(30_000) })
  await client.session.remove({ sessionID: session.id }, { signal: AbortSignal.timeout(30_000) })
  const configuration = await fetch(new URL("/api/config", endpoint.url), { headers: { ...Service.headers(endpoint), "x-opencode-directory": scratch }, signal: AbortSignal.timeout(30_000) })
  await writeFile(join(evidence, "config.json"), await configuration.text())
  let installed: { id: string; source: { type: string; path?: string } } | undefined
  const loadedBy = Date.now() + 15_000
  while (!installed && Date.now() < loadedBy) {
    const response: Response = await fetch(new URL("/api/plugin", endpoint.url), {
      headers: { ...Service.headers(endpoint), "x-opencode-directory": scratch },
      signal: AbortSignal.timeout(30_000),
    })
    assert.equal(response.status, 200)
    const result = await response.json() as { data: { id: string; source: { type: string; path?: string } }[] }
    await writeFile(join(evidence, "plugins.json"), JSON.stringify(result, null, 2))
    installed = result.data.find(plugin => plugin.id === "civilized-agent")
    if (!installed) await Bun.sleep(100)
  }
  assert.equal(installed?.source.type, "local")
  assert.ok(installed?.source.path?.replaceAll("\\", "/").startsWith(app.replaceAll("\\", "/") + "/"))
  await writeFile(join(evidence, "proof.json"), JSON.stringify({ passed: true, scope: "real OpenCode package bootstrap without external-companion or executable overrides", app }, null, 2))
} finally {
  await Service.stop({ file })
  if (server.exitCode === null) {
    Bun.spawnSync(["taskkill", "/PID", String(server.pid), "/T", "/F"])
    await server.exited
  }
}
