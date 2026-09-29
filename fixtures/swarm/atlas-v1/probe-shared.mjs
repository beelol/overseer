import pg from 'pg';
import { randomUUID } from 'node:crypto';
import { createAtlasApp, createWorkerNamespace } from './src/server.ts';

const databaseUrl = process.env.ATLAS_DATABASE_URL;
if (!databaseUrl) throw new Error('ATLAS_DATABASE_URL is required');
const namespace = `atlas_shared_${randomUUID().replaceAll('-', '')}`;
const root = new pg.Pool({ connectionString: databaseUrl });
let first;
let second;
let firstServer;
let secondServer;
let schemaCreated = false;
try {
  await createWorkerNamespace(root, namespace);
  schemaCreated = true;
  const connection = { connectionString: databaseUrl, options: `-c search_path=${namespace}` };
  first = new pg.Pool(connection);
  second = new pg.Pool(connection);
  firstServer = createAtlasApp(first).listen(0, '127.0.0.1');
  secondServer = createAtlasApp(second).listen(0, '127.0.0.1');
  await Promise.all([firstServer, secondServer].map(server =>
    new Promise(resolve => server.once('listening', resolve))));
  const base = server => `http://127.0.0.1:${server.address().port}`;
  const title = async pool => (await pool.query(
    "SELECT title FROM tasks WHERE id='task-b-7'")).rows[0].title;
  const patch = async (server, value) => (await fetch(`${base(server)}/tasks/task-b-7`, {
    method: 'PATCH',
    headers: { authorization: 'Bearer alice-test', 'content-type': 'application/json' },
    body: JSON.stringify({ title: value }),
  })).status;

  const j2Before = await title(first);
  const j7Initial = await title(second);
  const j2Status = await patch(firstServer, 'changed-by-alice');
  const j2After = await title(first);
  const j7Before = await title(second);
  const j7Status = await patch(secondServer, 'changed-by-j7');
  const j7After = await title(second);
  const j2Final = await title(first);
  process.stdout.write(`${JSON.stringify({
    fixtureVersion: 1,
    fault: 'shared-schema',
    resource: `db:atlas:${namespace}`,
    j2: { namespace, taskBefore: j2Before, taskAfter: j2After,
      taskAtEnd: j2Final, foreignPatchStatus: j2Status },
    j7: { namespace, taskInitially: j7Initial, taskBefore: j7Before,
      taskAfter: j7After, foreignPatchStatus: j7Status },
  })}\n`);
} finally {
  if (firstServer) await new Promise(resolve => firstServer.close(resolve));
  if (secondServer) await new Promise(resolve => secondServer.close(resolve));
  if (first) await first.end();
  if (second) await second.end();
  if (schemaCreated) await root.query(`DROP SCHEMA ${namespace} CASCADE`);
  await root.end();
}
