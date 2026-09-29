import pg from 'pg';
import { writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { probeAtlas, probeJob } from './src/probe.ts';

const root = new pg.Pool({ connectionString: process.env.ATLAS_DATABASE_URL });
try {
  const job = process.argv[2];
  const variant = process.argv[3];
  const output = job ? { fixtureVersion: 1, job, evidence: await probeJob(root, job, { variant }) }
    : await probeAtlas(root);
  if (process.env.ATLAS_PROBE_STARTED_FILE) {
    writeFileSync(process.env.ATLAS_PROBE_STARTED_FILE, JSON.stringify({
      fixtureVersion: output.fixtureVersion, job: output.job,
      attachmentStatus: output.evidence?.objectStatus,
    }));
  }
  const holdMs = Number(process.env.ATLAS_PROBE_DELAY_MS ?? 0);
  if (!Number.isInteger(holdMs) || holdMs < 0 || holdMs > 30_000) {
    throw new Error('invalid fixture probe delay');
  }
  if (holdMs) await delay(holdMs);
  process.stdout.write(`${JSON.stringify(output)}\n`);
} finally {
  await root.end();
}
