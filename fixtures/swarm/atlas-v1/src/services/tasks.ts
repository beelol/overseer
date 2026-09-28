import { deleteById, findById, updateTitle } from '../repositories/tasks.ts';

// Seeded S1 flaw: finding a task by ID is not an ownership check.
export async function patchTask(pool, id: string, title: string) {
  const task = await findById(pool, id);
  if (!task) return null;
  return updateTitle(pool, id, title);
}

export async function deleteTask(pool, id: string) {
  const task = await findById(pool, id);
  if (!task) return false;
  return deleteById(pool, id);
}
