import { Completions } from "../opencode-plugin/completions"

const baseline = process.memoryUsage().rss
const completions = new Completions(async () => ({ text: "The task completed." }), async () => {}, async () => true)
for (let index = 0; index < 10000; index++) {
  completions.start(`session-${index}`, 0)
  await completions.finish(`event-${index}`, `session-${index}`, 60001)
}
const resident = process.memoryUsage().rss
console.log(JSON.stringify({ tasks: 10000, runtimeResidentMB: resident / 1048576, addedResidentMB: (resident - baseline) / 1048576 }))
if (resident >= 100 * 1048576) process.exitCode = 1
