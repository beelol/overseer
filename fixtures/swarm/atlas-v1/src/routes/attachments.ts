import { Router } from 'express';

export function attachmentRoutes(pool, sign) {
  const router = Router();
  router.get('/:id/download', async (req, res) => {
    const attachment = (await pool.query('SELECT id, task_id, object_key FROM attachments WHERE id=$1',
      [req.params.id])).rows[0];
    if (!attachment) return res.sendStatus(404);
    // Seeded S1 flaw: no authorization through the attachment's task/project.
    res.json({ objectKey: attachment.object_key, url: sign(attachment.object_key) });
  });
  return router;
}
