import { appendFileSync, readFileSync } from 'node:fs'

if (!process.env.HERALD_HOST_PROOF) throw new Error('Missing host evidence path')
const value = JSON.parse(readFileSync(0, 'utf8'))
appendFileSync(process.env.HERALD_HOST_PROOF, JSON.stringify(value) + '\n')
