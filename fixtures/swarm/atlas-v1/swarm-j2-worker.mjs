// Fixture-only worker: run the versioned J2 database probe and report its evidence.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const required = (name) => {
  const value = process.env[name];
  if (!value) throw new Error(`missing ${name}`);
  return value;
};
const runId = required('OVERSEER_SWARM_RUN_ID');
const jobId = required('OVERSEER_SWARM_JOB_ID');
const attemptId = required('OVERSEER_SWARM_ATTEMPT_ID');
const token = required('OVERSEER_SWARM_TOKEN');
const revision = Number(required('OVERSEER_SWARM_REVISION'));
const binary = required('OVERSEER_BIN');
if (jobId !== 'j2' || !Number.isInteger(revision)) throw new Error('unexpected assignment');
const databaseUrlFile = process.argv[2];
if (!databaseUrlFile) throw new Error('missing disposable database URL file');

const probe = JSON.parse(execFileSync(process.execPath,
  [fileURLToPath(new URL('./probe.mjs', import.meta.url)), 'j2'],
  { encoding: 'utf8', env: { ...process.env,
    ATLAS_DATABASE_URL: readFileSync(databaseUrlFile, 'utf8') } }));
if (probe.fixtureVersion !== 1 || probe.job !== 'j2'
  || probe.evidence.foreignPatchStatus !== 200
  || probe.evidence.taskBefore !== 'Bob task'
  || probe.evidence.taskAfter !== 'changed-by-alice') {
  throw new Error('Atlas J2 did not reproduce the seeded task defect');
}

const call = (method, params) => {
  const reply = JSON.parse(execFileSync(binary,
    ['ctl', method, JSON.stringify(params)], { encoding: 'utf8' }));
  if (reply.error) throw new Error(`${method}: ${reply.error.message}`);
  return reply.result;
};
const artifact = `atlas-s5-recovered-${attemptId}`;
call('swarm.artifact.put', {
  run_id: runId, job_id: jobId, attempt_id: attemptId, token,
  artifact_id: artifact, source_revision: revision, kind: 'reproduction',
  content: JSON.stringify(probe.evidence),
});
call('swarm.report', {
  run_id: runId, job_id: jobId, attempt_id: attemptId, token,
  message_id: `atlas-s5-result-${attemptId}`, type: 'result', revision,
  payload: { audit_outcome: 'confirmed_defect', artifact_ids: [artifact] },
});
