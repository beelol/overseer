import { Router } from 'express';
import { randomUUID } from 'node:crypto';

export function exportRoutes(pool) {
  const router = Router();
  router.post('/', async (req, res) => {
    if (req.auth.scope === 'read') return res.sendStatus(403);
    const workspaceId = req.body?.workspaceId;
    if (typeof workspaceId !== 'string') return res.sendStatus(400);
    const allowed = await pool.query('SELECT 1 FROM memberships WHERE user_id=$1 AND workspace_id=$2',
      [req.auth.userId, workspaceId]);
    if (!allowed.rowCount) return res.sendStatus(403);
    const id = randomUUID();
    try {
      await pool.query('INSERT INTO exports(id,workspace_id,actor_id) VALUES($1,$2,$3)',
        [id, workspaceId, req.auth.userId]);
    } catch (error) {
      if (error.code === '42P01') return res.status(503).json({ unavailableResource: 'exports_queue' });
      throw error;
    }
    res.status(202).json({ id });
  });
  return router;
}

export function exportFixtureRoutes(pool) {
  const router = Router();
  router.post('/:id/run', async (req, res) => {
    const job = (await pool.query('SELECT workspace_id,actor_id FROM exports WHERE id=$1', [req.params.id])).rows[0];
    if (!job) return res.sendStatus(404);
    const rows = await pool.query('SELECT t.id FROM tasks t JOIN projects p ON p.id=t.project_id WHERE p.workspace_id=$1 ORDER BY t.id',
      [job.workspace_id]);
    res.json({ workspaceId: job.workspace_id, actorId: job.actor_id, taskIds: rows.rows.map(row => row.id) });
  });
  return router;
}
