import * as menuRepo from "../../db/repositories/menuRepo";
import * as customersRepo from "../../db/repositories/customersRepo";
import { listTables } from "../../db/repositories/tablesRepo";

/**
 * Builds the catalog payload the phone renders (menu + tables + credit
 * customers) and a stable hash of it, so the bridge pushes only when
 * something actually changed (pos-orders "push_catalog").
 */

export interface CatalogPayload {
  categories: { localId: number; name: string; sortOrder: number }[];
  items: {
    localId: number;
    categoryLocalId: number | null;
    name: string;
    price: number;
    isAvailable: boolean;
  }[];
  tables: {
    localId: number;
    section: string;
    label: string;
    sortOrder: number;
    isActive: boolean;
  }[];
  customers: { localId: number; name: string; phone: string; creditBalance: number }[];
  catalogHash: string;
}

/** FNV-1a over the serialized payload — cheap and stable across restarts. */
function fnv1a(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

export async function buildCatalogPayload(): Promise<CatalogPayload> {
  const [categories, items, tables, customers] = await Promise.all([
    menuRepo.listCategories(),
    menuRepo.listAllItems(),
    listTables(),
    customersRepo.listCustomers(),
  ]);

  const payload = {
    categories: categories
      .map((c, i) => ({ localId: c.id, name: c.name, sortOrder: i }))
      .sort((a, b) => a.localId - b.localId),
    items: items
      .map((it) => ({
        localId: it.id,
        categoryLocalId: it.category_id ?? null,
        name: it.name,
        price: it.price,
        isAvailable: it.is_available !== 0,
      }))
      .sort((a, b) => a.localId - b.localId),
    tables: tables
      .map((t) => ({
        localId: t.id,
        section: t.section ?? "",
        label: t.label,
        sortOrder: t.sort_order ?? 0,
        isActive: t.is_active === 1,
      }))
      .sort((a, b) => a.localId - b.localId),
    customers: customers
      .map((c) => ({
        localId: c.id,
        name: c.name,
        phone: c.phone ?? "",
        creditBalance: c.credit_balance ?? 0,
      }))
      .sort((a, b) => a.localId - b.localId),
  };

  return { ...payload, catalogHash: fnv1a(JSON.stringify(payload)) };
}
