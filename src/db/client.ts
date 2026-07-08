import Database from "@tauri-apps/plugin-sql";
import { join } from "@tauri-apps/api/path";
import { DB_FILE_NAME } from "../config/constants";
import { runMigrations } from "./migrations";

let db: Database | null = null;

/** Opens (or returns) the app database inside the user-chosen folder and applies migrations. */
export async function openDatabase(folderPath: string): Promise<Database> {
  if (db) return db;
  const fullPath = await join(folderPath, DB_FILE_NAME);
  const instance = await Database.load(`sqlite:${fullPath}`);
  await runMigrations(instance);
  db = instance;
  return instance;
}

/** The already-opened database. Throws if called before openDatabase — a programming error. */
export function getDb(): Database {
  if (!db) throw new Error("Database has not been opened yet");
  return db;
}

export function isDbOpen(): boolean {
  return db !== null;
}
