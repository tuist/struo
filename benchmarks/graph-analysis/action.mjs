import { createHash } from 'node:crypto';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

const [output, label, symbols, ...inputs] = process.argv.slice(2);
const hash = createHash('sha256').update(`${label}\n${symbols}\n`);
for (const input of inputs) hash.update(readFileSync(input));
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, `${label}:${hash.digest('hex')}\n`);
