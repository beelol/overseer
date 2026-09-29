import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import pg from 'pg';
import { createAtlasApp, createWorkerNamespace } from './src/server.ts';

const { Pool } = pg;
const databaseUrl = process.env.ATLAS_DATABASE_URL;
assert.ok(databaseUrl, 'ATLAS_DATABASE_URL must point to the local fixture database');
const root = new Pool({ connectionString: databaseUrl });
const workerNamespaces = [];

async function worker(name) {
  const namespace = `atlas_${name}_${process.pid}`;
  await createWorkerNamespace(root, namespace);
  workerNamespaces.push(namespace);
  const pool = new Pool({ connectionString: databaseUrl, options: `-c search_path=${namespace}` });
  const app = createAtlasApp(pool);
  const server = app.listen(0, '127.0.0.1');
  await new Promise(resolve => server.once('listening', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  return {
    pool,
    base,
    request: (path, options = {}) => fetch(`${base}${path}`, {
      ...options,
      headers: { authorization: 'Bearer alice-test', 'content-type': 'application/json', ...options.headers },
    }),
    close: async () => { await new Promise(resolve => server.close(resolve)); await pool.end(); },
  };
}

after(async () => {
  for (const namespace of workerNamespaces) {
    await root.query(`DROP SCHEMA IF EXISTS ${namespace} CASCADE`);
  }
  await root.end();
});

test('J2 reproduces foreign task mutation with a response and changed database row', async () => {
  const j2 = await worker('j2');
  try {
    const before = await j2.pool.query("SELECT title FROM tasks WHERE id='task-b-7'");
    assert.equal(before.rows[0].title, 'Bob task');
    const response = await j2.request('/tasks/task-b-7', {
      method: 'PATCH', body: JSON.stringify({ title: 'changed-by-alice' }),
    });
    assert.equal(response.status, 200);
    assert.equal((await response.json()).title, 'changed-by-alice');
    const afterRow = await j2.pool.query("SELECT title FROM tasks WHERE id='task-b-7'");
    assert.equal(afterRow.rows[0].title, 'changed-by-alice');
    const owner = await j2.request('/tasks/task-a-1', {
      method: 'PATCH', body: JSON.stringify({ title: 'owner-change' }),
    });
    assert.equal(owner.status, 200);
  } finally {
    await j2.close();
  }
});

test('J1 protected project lookup and J4 foreign attachment remain separate findings', async () => {
  const j1 = await worker('j1');
  const j4 = await worker('j4');
  try {
    assert.equal((await j1.request('/projects/project-a-1')).status, 200);
    assert.equal((await j1.request('/projects/project-b-1')).status, 403);
    const download = await j4.request('/attachments/attachment-b-1/download');
    assert.equal(download.status, 200);
    const signed = await download.json();
    assert.equal(signed.objectKey, 'workspace-b/bob.txt');
    const fetched = await fetch(`${j4.base}${signed.url}`);
    assert.equal(fetched.status, 200);
    assert.equal(await fetched.text(), 'Bob private attachment');
  } finally {
    await j1.close();
    await j4.close();
  }
});

test('worker database namespaces isolate mutation evidence', async () => {
  const j2 = await worker('j2_isolation');
  const j7 = await worker('j7_repro');
  try {
    await j2.request('/tasks/task-b-7', {
      method: 'PATCH', body: JSON.stringify({ title: 'changed-by-alice' }),
    });
    const independent = await j7.pool.query("SELECT title FROM tasks WHERE id='task-b-7'");
    assert.equal(independent.rows[0].title, 'Bob task');
    const replay = await j7.request('/tasks/task-b-7', {
      method: 'PATCH', body: JSON.stringify({ title: 'changed-by-alice' }),
    });
    assert.equal(replay.status, 200);
    assert.equal((await j7.pool.query("SELECT title FROM tasks WHERE id='task-b-7'")).rows[0].title, 'changed-by-alice');
  } finally {
    await j2.close();
    await j7.close();
  }
});

test('J2 foreign delete is an independent mutation path', async () => {
  const j2 = await worker('j2_delete');
  try {
    const response = await j2.request('/tasks/task-b-7', { method: 'DELETE' });
    assert.equal(response.status, 204);
    assert.equal((await j2.pool.query("SELECT count(*)::int AS count FROM tasks WHERE id='task-b-7'")).rows[0].count, 0);
  } finally {
    await j2.close();
  }
});

test('J3 ordinary member cannot elevate role while local admin can', async () => {
  const j3 = await worker('j3');
  const change = (path, token) => j3.request(path, {
    method: 'PATCH', headers: { authorization: `Bearer ${token}` },
    body: JSON.stringify({ role: 'admin' }),
  });
  try {
    assert.equal((await change('/workspaces/workspace-a/members/alice', 'alice-test')).status, 403);
    assert.equal((await change('/workspaces/workspace-b/members/bob', 'alice-test')).status, 403);
    assert.equal((await change('/workspaces/workspace-b/members/bob', 'owner-a-test')).status, 403);
    assert.equal((await change('/workspaces/workspace-a/members/alice', 'owner-a-test')).status, 200);
    assert.equal((await j3.pool.query("SELECT role FROM memberships WHERE user_id='alice' AND workspace_id='workspace-a'")).rows[0].role, 'admin');
  } finally {
    await j3.close();
  }
});

test('J5 export queue and worker query stay inside the authorized workspace', async () => {
  const j5 = await worker('j5');
  try {
    const foreign = await j5.request('/exports', {
      method: 'POST', body: JSON.stringify({ workspaceId: 'workspace-b' }),
    });
    assert.equal(foreign.status, 403);
    const allowed = await j5.request('/exports', {
      method: 'POST', body: JSON.stringify({ workspaceId: 'workspace-a' }),
    });
    assert.equal(allowed.status, 202);
    const job = await allowed.json();
    const queued = await j5.pool.query('SELECT workspace_id,actor_id FROM exports WHERE id=$1', [job.id]);
    assert.deepEqual(queued.rows[0], { workspace_id: 'workspace-a', actor_id: 'alice' });
    const processed = await j5.request(`/__fixture/exports/${job.id}/run`, { method: 'POST' });
    assert.equal(processed.status, 200);
    assert.deepEqual((await processed.json()).taskIds, ['task-a-1']);
  } finally {
    await j5.close();
  }
});

test('J6 read-only and revoked tokens cannot mutate and removal disables a former member', async () => {
  const j6 = await worker('j6');
  const patch = token => j6.request('/tasks/task-a-1', {
    method: 'PATCH', headers: { authorization: `Bearer ${token}` },
    body: JSON.stringify({ title: 'unauthorized-change' }),
  });
  try {
    assert.equal((await patch('alice-readonly')).status, 403);
    assert.equal((await patch('alice-revoked')).status, 401);
    await j6.pool.query("DELETE FROM memberships WHERE user_id='alice' AND workspace_id='workspace-a'");
    assert.equal((await patch('alice-test')).status, 401);
    assert.equal((await j6.pool.query("SELECT title FROM tasks WHERE id='task-a-1'")).rows[0].title, 'Alice task');
  } finally {
    await j6.close();
  }
});
