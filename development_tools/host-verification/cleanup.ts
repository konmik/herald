import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { OpenCode } from "@opencode/client"
import { Service } from "@opencode/client/service"

const scratch = process.argv[2]
if (!scratch) throw new Error("Missing owned scratch directory")
const file = join(scratch, "state/opencode/service.json")
const endpoint = await Service.discover({ file })
if (endpoint) {
  const manifest = await readFile(join(scratch, "session.json"), "utf8").catch(() => undefined)
  if (manifest) {
    const { sessionID } = JSON.parse(manifest)
    const client = OpenCode.make({ baseUrl: endpoint.url, headers: Service.headers(endpoint) })
    const session = await client.session.get({ sessionID }).catch(() => undefined)
    if (session) {
      if (session.location.directory !== scratch) throw new Error("Refusing to remove a session outside the owned scratch directory")
      await client.session.remove({ sessionID })
    }
  }
}
await Service.stop({ file })
