import { getDb } from "../client";
import type { Category, MenuItem } from "../../types";

export async function listCategories(): Promise<Category[]> {
  return getDb().select<Category[]>("SELECT * FROM categories ORDER BY name");
}

export async function addCategory(name: string): Promise<void> {
  await getDb().execute("INSERT INTO categories (name) VALUES ($1)", [name]);
}

export async function renameCategory(id: number, name: string): Promise<void> {
  await getDb().execute("UPDATE categories SET name = $1 WHERE id = $2", [name, id]);
}

/** Deletes a category and all of its items (matches existing cascade behavior). */
export async function deleteCategory(id: number): Promise<void> {
  await getDb().execute("DELETE FROM items WHERE category_id = $1", [id]);
  await getDb().execute("DELETE FROM categories WHERE id = $1", [id]);
}

export async function listAllItems(): Promise<MenuItem[]> {
  return getDb().select<MenuItem[]>("SELECT * FROM items ORDER BY name");
}

export async function listItems(categoryId: number): Promise<MenuItem[]> {
  return getDb().select<MenuItem[]>("SELECT * FROM items WHERE category_id = $1 ORDER BY name", [categoryId]);
}

export async function addItem(categoryId: number, name: string, price: number): Promise<void> {
  await getDb().execute("INSERT INTO items (category_id, name, price) VALUES ($1, $2, $3)", [categoryId, name, price]);
}

export async function updateItem(id: number, name: string, price: number, categoryId: number): Promise<void> {
  await getDb().execute("UPDATE items SET name = $1, price = $2, category_id = $3 WHERE id = $4", [name, price, categoryId, id]);
}

export async function deleteItem(id: number): Promise<void> {
  await getDb().execute("DELETE FROM items WHERE id = $1", [id]);
}

export async function itemNameExistsInCategory(categoryId: number, name: string, excludeId?: number): Promise<boolean> {
  const rows = await getDb().select<{ id: number }[]>(
    "SELECT id FROM items WHERE category_id = $1 AND LOWER(name) = LOWER($2)",
    [categoryId, name]
  );
  return rows.some((r) => r.id !== excludeId);
}
