import { Router } from 'express';
import { deleteTask, patchTask } from '../services/tasks.ts';

export function taskRoutes(pool) {
  const router = Router();
  router.patch('/:id', async (req, res) => {
    if (req.auth.scope === 'read') return res.sendStatus(403);
    if (typeof req.body?.title !== 'string' || !req.body.title) return res.sendStatus(400);
    const task = await patchTask(pool, req.params.id, req.body.title);
    if (!task) return res.sendStatus(404);
    res.json(task);
  });
  router.delete('/:id', async (req, res) => {
    if (req.auth.scope === 'read') return res.sendStatus(403);
    if (!await deleteTask(pool, req.params.id)) return res.sendStatus(404);
    res.sendStatus(204);
  });
  return router;
}
