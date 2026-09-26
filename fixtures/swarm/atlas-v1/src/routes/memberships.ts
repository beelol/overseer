import { Router } from 'express';

export function membershipRoutes(pool) {
  const router = Router();
  router.patch('/:workspaceId/members/:userId', async (req, res) => {
    if (!['admin', 'member'].includes(req.body?.role)) return res.sendStatus(400);
    const caller = (await pool.query('SELECT role FROM memberships WHERE user_id=$1 AND workspace_id=$2',
      [req.auth.userId, req.params.workspaceId])).rows[0];
    if (caller?.role !== 'admin') return res.sendStatus(403);
    const changed = (await pool.query('UPDATE memberships SET role=$3 WHERE user_id=$1 AND workspace_id=$2 RETURNING user_id,workspace_id,role',
      [req.params.userId, req.params.workspaceId, req.body.role])).rows[0];
    if (!changed) return res.sendStatus(404);
    res.json(changed);
  });
  return router;
}
