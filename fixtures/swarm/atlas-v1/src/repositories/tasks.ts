export async function findById(pool, id: string) {
  const result = await pool.query('SELECT id, project_id, title FROM tasks WHERE id=$1', [id]);
  return result.rows[0] ?? null;
}

export async function updateTitle(pool, id: string, title: string) {
  const result = await pool.query('UPDATE tasks SET title=$2 WHERE id=$1 RETURNING id, project_id, title', [id, title]);
  return result.rows[0] ?? null;
}

export async function deleteById(pool, id: string) {
  const result = await pool.query('DELETE FROM tasks WHERE id=$1 RETURNING id', [id]);
  return result.rowCount > 0;
}
