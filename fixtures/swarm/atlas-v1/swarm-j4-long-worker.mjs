// Fixture-only worker: keep polling the durable broker while a real Atlas
// attachment probe occupies the work command. Receipt is not application.
import { execFileSync, spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
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
if (jobId !== 'j4' || !Number.isInteger(revision)) throw new Error('unexpected assignment');
const [databaseUrlFile, marker, mode] = process.argv.slice(2);
if (!databaseUrlFile || !marker) throw new Error('missing disposable fixture inputs');
if (mode && mode !== 'heartbeat') throw new Error('unknown fixture worker mode');

const call = (method, params) => {
  const reply = JSON.parse(execFileSync(binary,
    ['ctl', method, JSON.stringify(params)], { encoding: 'utf8' }));
  if (reply.error) throw new Error(`${method}: ${reply.error.message}`);
  return reply.result;
};

const probe = spawn(process.execPath,
  [fileURLToPath(new URL('./probe.mjs', import.meta.url)), 'j4'],
  { env: { ...process.env,
    ATLAS_DATABASE_URL: readFileSync(databaseUrlFile, 'utf8'),
    ATLAS_PROBE_STARTED_FILE: marker,
    ATLAS_PROBE_DELAY_MS: '15000',
  } });
let output = '';
let error = '';
let exitCode;
probe.stdout.on('data', chunk => { output += chunk; });
probe.stderr.on('data', chunk => { error += chunk; });
probe.on('close', code => { exitCode = code; });
const interrupt = () => { probe.kill('SIGTERM'); process.exit(143); };
process.on('SIGTERM', interrupt);
process.on('SIGINT', interrupt);

let cursor = 0;
const pending = [];
let heartbeats = 0;
let lastHeartbeat = 0;
while (exitCode === undefined) {
  if (mode === 'heartbeat' && Date.now() - lastHeartbeat >= 250) {
    heartbeats += 1;
    call('swarm.report', { run_id: runId, job_id: jobId,
      attempt_id: attemptId, token, revision,
      message_id: `atlas-j4-heartbeat-${attemptId}-${heartbeats}`,
      type: 'progress', payload: { state: 'working' } });
    lastHeartbeat = Date.now();
  }
  const page = call('swarm.messages', { run_id: runId, recipient: attemptId, token,
    cursor, limit: 20 });
  for (const message of page.messages) {
    cursor = Math.max(cursor, message.seq);
    if (message.type !== 'redirect' || message.phase !== 'queued') continue;
    call('swarm.ack', { run_id: runId, message_id: message.message_id,
      recipient: attemptId, token, revision: message.revision,
      phase: 'delivered' });
    pending.push(message);
  }
  await delay(100);
}
if (exitCode !== 0) throw new Error(`Atlas J4 probe failed: ${error}`);
const result = JSON.parse(output);
if (result.fixtureVersion !== 1 || result.job !== 'j4'
  || result.evidence.objectStatus !== 200) {
  throw new Error('Atlas J4 did not reproduce the seeded attachment defect');
}
for (const message of pending) {
  call('swarm.ack', { run_id: runId, message_id: message.message_id,
    recipient: attemptId, token, revision: message.revision,
    phase: 'applied' });
}
const artifact = `atlas-j4-${attemptId}`;
call('swarm.artifact.put', { run_id: runId, job_id: jobId,
  attempt_id: attemptId, token, artifact_id: artifact,
  source_revision: revision, kind: 'reproduction',
  content: JSON.stringify(result.evidence) });
call('swarm.report', { run_id: runId, job_id: jobId, attempt_id: attemptId,
  token, message_id: `atlas-j4-result-${attemptId}`, type: 'result',
  revision, payload: { audit_outcome: 'confirmed_defect', artifact_ids: [artifact] } });
