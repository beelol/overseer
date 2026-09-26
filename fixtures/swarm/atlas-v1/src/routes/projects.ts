import { Router } from 'express';

export function projectRoutes(pool) {
  const router = Router();
  router.get('/:id', async (req, res) => {
    const project = (await pool.query('SELECT id, workspace_id FROM projects WHERE id=$1', [req.params.id])).rows[0];
    if (!project) return res.sendStatus(404);
    const member = await pool.query('SELECT 1 FROM memberships WHERE user_id=$1 AND workspace_id=$2',
      [req.auth.userId, project.workspace_id]);
    if (!member.rowCount) return res.sendStatus(403);
    res.json(project);
  });
  return router;
}
