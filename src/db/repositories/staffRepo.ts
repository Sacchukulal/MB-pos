import { getDb } from "../client";
import type { StaffMember } from "../../types";

export async function listStaff(): Promise<StaffMember[]> {
  return getDb().select<StaffMember[]>("SELECT * FROM staff ORDER BY name");
}

export async function addStaff(name: string, role: string, phone: string): Promise<void> {
  await getDb().execute("INSERT INTO staff (name, role, phone) VALUES ($1, $2, $3)", [name, role, phone]);
}

export async function deleteStaff(id: number): Promise<void> {
  await getDb().execute("DELETE FROM staff WHERE id = $1", [id]);
}
