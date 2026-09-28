import { Router } from 'express';
import { deleteTask, patchTask } from '../services/tasks.ts';

export function taskRoutes(pool, { taskOwnershipGuard = false } = {}) {
  const router = Router();
  const canAccessTask = async (id, userId) => (await pool.query(
    `SELECT 1 FROM tasks t JOIN projects p ON p.id=t.project_id
       JOIN memberships m ON m.workspace_id=p.workspace_id
       WHERE t.id=$1 AND m.user_id=$2`, [id, userId],
  )).rowCount > 0;
  router.patch('/:id', async (req, res) => {
    if (req.auth.scope === 'read') return res.sendStatus(403);
    if (typeof req.body?.title !== 'string' || !req.body.title) return res.sendStatus(400);
    if (taskOwnershipGuard && !await canAccessTask(req.params.id, req.auth.userId)) return res.sendStatus(403);
    const task = await patchTask(pool, req.params.id, req.body.title);
    if (!task) return res.sendStatus(404);
    res.json(task);
  });
  router.delete('/:id', async (req, res) => {
    if (req.auth.scope === 'read') return res.sendStatus(403);
    if (taskOwnershipGuard && !await canAccessTask(req.params.id, req.auth.userId)) return res.sendStatus(403);
    if (!await deleteTask(pool, req.params.id)) return res.sendStatus(404);
    res.sendStatus(204);
  });
  return router;
}
