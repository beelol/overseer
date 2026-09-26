import assert from 'node:assert/strict';
import { test } from 'node:test';
import pg from 'pg';
import { probeAtlas, probeJob } from './src/probe.ts';

test('Atlas job probes produce distinct, database-backed evidence for the S1 director', async () => {
  assert.ok(process.env.ATLAS_DATABASE_URL);
  const root = new pg.Pool({ connectionString: process.env.ATLAS_DATABASE_URL });
  try {
    const trace = await probeAtlas(root);
    assert.equal(trace.fixtureVersion, 1);
    assert.deepEqual(Object.keys(trace.jobs), ['j1', 'j2', 'j3', 'j4', 'j5', 'j6', 'j7']);
    assert.equal(trace.jobs.j1.foreignStatus, 403);
    assert.equal(trace.jobs.j2.foreignPatchStatus, 200);
    assert.equal(trace.jobs.j2.taskBefore, 'Bob task');
    assert.equal(trace.jobs.j2.taskAfter, 'changed-by-alice');
    assert.equal(trace.jobs.j3.memberElevationStatus, 403);
    assert.equal(trace.jobs.j4.objectStatus, 200);
    assert.equal(trace.jobs.j4.objectBody, 'Bob private attachment');
    assert.equal(trace.jobs.j5.foreignExportStatus, 403);
    assert.deepEqual(trace.jobs.j5.queuedTaskIds, ['task-a-1']);
    assert.equal(trace.jobs.j6.readOnlyMutationStatus, 403);
    assert.equal(trace.jobs.j6.revokedMutationStatus, 401);
    assert.equal(trace.jobs.j7.taskBefore, 'Bob task');
    assert.equal(trace.jobs.j7.taskAfter, 'changed-by-alice');
    assert.notEqual(trace.jobs.j2.namespace, trace.jobs.j7.namespace);
  } finally {
    await root.end();
  }
});

test('a job probe runs independently at the moment its Swarm job is admitted', async () => {
  const root = new pg.Pool({ connectionString: process.env.ATLAS_DATABASE_URL });
  try {
    const j2 = await probeJob(root, 'j2');
    const j7 = await probeJob(root, 'j7');
    assert.equal(j2.taskAfter, 'changed-by-alice');
    assert.equal(j7.taskBefore, 'Bob task');
    assert.notEqual(j2.namespace, j7.namespace);
  } finally {
    await root.end();
  }
});
