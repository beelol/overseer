import { randomUUID } from 'node:crypto';
import { Pool } from 'pg';
import { createAtlasApp, createWorkerNamespace } from './server.ts';

async function withWorker(root, databaseUrl: string, name: string, run, options = {}) {
  const namespace = `atlas_${name}_${randomUUID().replaceAll('-', '')}`;
  await createWorkerNamespace(root, namespace);
  const pool = new Pool({ connectionString: databaseUrl, options: `-c search_path=${namespace}` });
  let server;
  try {
    if (options.missingExportQueue) await pool.query('DROP TABLE exports');
    server = createAtlasApp(pool, options).listen(0, '127.0.0.1');
    await new Promise(resolve => server.once('listening', resolve));
    const base = `http://127.0.0.1:${server.address().port}`;
    const request = (path, options = {}) => fetch(`${base}${path}`, {
      ...options,
      headers: { authorization: 'Bearer alice-test', 'content-type': 'application/json', ...options.headers },
    });
    return { namespace, ...await run({ pool, base, request }) };
  } finally {
    try {
      if (server) await new Promise(resolve => server.close(resolve));
    } finally {
      try { await pool.end(); }
      finally { await root.query(`DROP SCHEMA ${namespace} CASCADE`); }
    }
  }
}

const probes = {
  j1: async ({ request }) => ({
    ownStatus: (await request('/projects/project-a-1')).status,
    foreignStatus: (await request('/projects/project-b-1')).status,
    sourcePath: 'src/routes/projects.ts',
  }),
  j2: async ({ pool, request }) => {
    const taskBefore = (await pool.query("SELECT title FROM tasks WHERE id='task-b-7'")).rows[0].title;
    const foreignPatchStatus = (await request('/tasks/task-b-7', {
      method: 'PATCH', body: JSON.stringify({ title: 'changed-by-alice' }),
    })).status;
    const taskAfter = (await pool.query("SELECT title FROM tasks WHERE id='task-b-7'")).rows[0].title;
    const foreignDeleteStatus = (await request('/tasks/task-b-7', { method: 'DELETE' })).status;
    return { taskBefore, taskAfter, foreignPatchStatus, foreignDeleteStatus,
      sourcePath: 'src/repositories/tasks.ts::findById' };
  },
  j3: async ({ pool, request }) => {
    const change = (workspace, user, token) => request(`/workspaces/${workspace}/members/${user}`, {
      method: 'PATCH', headers: { authorization: `Bearer ${token}` },
      body: JSON.stringify({ role: 'admin' }),
    });
    const memberElevationStatus = (await change('workspace-a', 'alice', 'alice-test')).status;
    const foreignStatus = (await change('workspace-b', 'bob', 'owner-a-test')).status;
    const adminStatus = (await change('workspace-a', 'alice', 'owner-a-test')).status;
    const finalRole = (await pool.query("SELECT role FROM memberships WHERE user_id='alice' AND workspace_id='workspace-a'")).rows[0].role;
    return { memberElevationStatus, foreignStatus, adminStatus, finalRole };
  },
  j4: async ({ base, request }) => {
    const download = await request('/attachments/attachment-b-1/download');
    const { objectKey, url } = await download.json();
    const object = await fetch(`${base}${url}`);
    return { downloadStatus: download.status, objectKey, objectStatus: object.status,
      objectBody: await object.text(), sourcePath: 'src/routes/attachments.ts' };
  },
  j5: async ({ pool, request }) => {
    const foreignExportStatus = (await request('/exports', {
      method: 'POST', body: JSON.stringify({ workspaceId: 'workspace-b' }),
    })).status;
    const accepted = await request('/exports', {
      method: 'POST', body: JSON.stringify({ workspaceId: 'workspace-a' }),
    });
    if (accepted.status === 503) {
      const failure = await accepted.json();
      return { foreignExportStatus, ownExportStatus: 503, queueAvailable: false,
        unavailableResource: failure.unavailableResource };
    }
    const { id } = await accepted.json();
    const queued = (await pool.query('SELECT workspace_id,actor_id FROM exports WHERE id=$1', [id])).rows[0];
    const processed = await request(`/__fixture/exports/${id}/run`, { method: 'POST' });
    return { foreignExportStatus, ownExportStatus: accepted.status, queueAvailable: true,
      queuedWorkspace: queued.workspace_id, queuedActor: queued.actor_id,
      queuedTaskIds: (await processed.json()).taskIds };
  },
  j6: async ({ pool, request }) => {
    const patch = token => request('/tasks/task-a-1', {
      method: 'PATCH', headers: { authorization: `Bearer ${token}` },
      body: JSON.stringify({ title: 'unauthorized-change' }),
    });
    const readOnlyMutationStatus = (await patch('alice-readonly')).status;
    const revokedMutationStatus = (await patch('alice-revoked')).status;
    await pool.query("DELETE FROM memberships WHERE user_id='alice' AND workspace_id='workspace-a'");
    const formerMemberMutationStatus = (await patch('alice-test')).status;
    return { readOnlyMutationStatus, revokedMutationStatus, formerMemberMutationStatus };
  },
  j7: async ({ pool, request }) => {
    const taskBefore = (await pool.query("SELECT title FROM tasks WHERE id='task-b-7'")).rows[0].title;
    const foreignPatchStatus = (await request('/tasks/task-b-7', {
      method: 'PATCH', body: JSON.stringify({ title: 'changed-by-alice' }),
    })).status;
    const taskAfter = (await pool.query("SELECT title FROM tasks WHERE id='task-b-7'")).rows[0].title;
    return { taskBefore, taskAfter, foreignPatchStatus, independentOf: 'j2' };
  },
};

export async function probeJob(root, job: string, { variant } = {}) {
  const databaseUrl = process.env.ATLAS_DATABASE_URL;
  if (!databaseUrl) throw new Error('ATLAS_DATABASE_URL is required');
  if (!Object.hasOwn(probes, job)) throw new Error(`unknown Atlas job ${job}`);
  const variants = { j7: 'task-guarded', j5: 'export-queue-missing' };
  if (variant && variants[job] !== variant) {
    throw new Error(`unknown Atlas variant ${variant} for ${job}`);
  }
  const evidence = await withWorker(root, databaseUrl, job, probes[job], {
    taskOwnershipGuard: variant === 'task-guarded',
    missingExportQueue: variant === 'export-queue-missing',
  });
  return variant ? { variant, ...evidence } : evidence;
}

export async function probeAtlas(root) {
  const jobs = {};
  for (const job of ['j1', 'j2', 'j3', 'j4', 'j5', 'j6', 'j7']) {
    jobs[job] = await probeJob(root, job);
  }
  return { fixtureVersion: 1, jobs };
}
