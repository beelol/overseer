import pg from 'pg';
import { probeAtlas, probeJob } from './src/probe.ts';

const root = new pg.Pool({ connectionString: process.env.ATLAS_DATABASE_URL });
try {
  const job = process.argv[2];
  const variant = process.argv[3];
  const output = job ? { fixtureVersion: 1, job, evidence: await probeJob(root, job, { variant }) }
    : await probeAtlas(root);
  process.stdout.write(`${JSON.stringify(output)}\n`);
} finally {
  await root.end();
}
