import express from 'express';
import { randomUUID } from 'node:crypto';
import { taskRoutes } from './routes/tasks.ts';
import { projectRoutes } from './routes/projects.ts';
import { attachmentRoutes } from './routes/attachments.ts';
import { membershipRoutes } from './routes/memberships.ts';
import { exportFixtureRoutes, exportRoutes } from './routes/exports.ts';

const objects = new Map([['workspace-b/bob.txt', 'Bob private attachment']]);

export async function createWorkerNamespace(root, namespace: string) {
  if (!/^atlas_[a-z0-9_]+$/.test(namespace)) throw new Error('invalid worker namespace');
  const client = await root.connect();
  try {
    await client.query('BEGIN');
    await client.query(`CREATE SCHEMA ${namespace}`);
    await client.query(`SET LOCAL search_path TO ${namespace}`);
    await client.query(`
      CREATE TABLE workspaces(id text PRIMARY KEY);
      CREATE TABLE memberships(user_id text, workspace_id text REFERENCES workspaces(id), role text,
        PRIMARY KEY(user_id,workspace_id));
      CREATE TABLE projects(id text PRIMARY KEY, workspace_id text REFERENCES workspaces(id));
      CREATE TABLE tasks(id text PRIMARY KEY, project_id text REFERENCES projects(id), title text);
      CREATE TABLE attachments(id text PRIMARY KEY, task_id text REFERENCES tasks(id) ON DELETE CASCADE, object_key text);
      CREATE TABLE api_tokens(token text PRIMARY KEY, user_id text, scope text, revoked boolean DEFAULT false);
      CREATE TABLE exports(id text PRIMARY KEY, workspace_id text REFERENCES workspaces(id), actor_id text);
    `);
    await client.query(`
      INSERT INTO workspaces VALUES ('workspace-a'),('workspace-b');
      INSERT INTO memberships VALUES ('alice','workspace-a','member'),('owner-a','workspace-a','admin'),('bob','workspace-b','admin');
      INSERT INTO projects VALUES ('project-a-1','workspace-a'),('project-b-1','workspace-b');
      INSERT INTO tasks VALUES ('task-a-1','project-a-1','Alice task'),('task-b-7','project-b-1','Bob task');
      INSERT INTO attachments VALUES ('attachment-b-1','task-b-7','workspace-b/bob.txt');
      INSERT INTO api_tokens(token,user_id,scope,revoked) VALUES
        ('alice-test','alice','write',false),('alice-readonly','alice','read',false),
        ('alice-revoked','alice','write',true),('owner-a-test','owner-a','write',false);
    `);
    await client.query('COMMIT');
  } catch (error) {
    await client.query('ROLLBACK');
    throw error;
  } finally {
    client.release();
  }
}

export function createAtlasApp(pool) {
  const app = express();
  const signed = new Map();
  app.use(express.json());
  app.get('/objects/:signature', (req, res) => {
    const key = signed.get(req.params.signature);
    if (!key || !objects.has(key)) return res.sendStatus(404);
    res.type('text/plain').send(objects.get(key));
  });
  app.use(async (req, res, next) => {
    try {
      const token = /^Bearer (.+)$/.exec(req.get('authorization') ?? '')?.[1];
      if (!token) return res.sendStatus(401);
      const identity = (await pool.query('SELECT user_id,scope FROM api_tokens WHERE token=$1 AND NOT revoked',
        [token])).rows[0];
      if (!identity) return res.sendStatus(401);
      const memberships = await pool.query('SELECT 1 FROM memberships WHERE user_id=$1 LIMIT 1',
        [identity.user_id]);
      if (!memberships.rowCount) return res.sendStatus(401);
      req.auth = { userId: identity.user_id, scope: identity.scope };
      next();
    } catch (error) { next(error); }
  });
  app.use('/projects', projectRoutes(pool));
  app.use('/tasks', taskRoutes(pool));
  app.use('/attachments', attachmentRoutes(pool, key => {
    const signature = randomUUID();
    signed.set(signature, key);
    return `/objects/${signature}`;
  }));
  app.use('/workspaces', membershipRoutes(pool));
  app.use('/exports', exportRoutes(pool));
  app.use('/__fixture/exports', exportFixtureRoutes(pool));
  return app;
}
