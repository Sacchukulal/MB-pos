import { useEffect, useRef, useState } from "react";
import { Check, Edit2, Plus, Tag, Trash2, X } from "lucide-react";
import { formatCurrency } from "../../utils/format";
import { useToast } from "../../hooks/useToast";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import EmptyState from "../../components/ui/EmptyState";
import * as menuRepo from "../../db/repositories/menuRepo";
import type { Category, MenuItem } from "../../types";

interface MenuManagementProps {
  dbReady: boolean;
}

export default function MenuManagement({ dbReady }: MenuManagementProps) {
  const { toast } = useToast();
  const [categories, setCategories] = useState<Category[]>([]);
  const [items, setItems] = useState<MenuItem[]>([]);
  const [selectedCategoryId, setSelectedCategoryId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);

  const [newCategoryName, setNewCategoryName] = useState("");
  const [newItemName, setNewItemName] = useState("");
  const [newItemPrice, setNewItemPrice] = useState("");

  const [editingCategoryId, setEditingCategoryId] = useState<number | null>(null);
  const [editCategoryName, setEditCategoryName] = useState("");

  const [editingItemId, setEditingItemId] = useState<number | null>(null);
  const [editItemName, setEditItemName] = useState("");
  const [editItemPrice, setEditItemPrice] = useState("");
  const [editItemCategoryId, setEditItemCategoryId] = useState<number | null>(null);

  const [deleteConfirmation, setDeleteConfirmation] = useState<{ type: "category" | "item"; id: number; name: string } | null>(null);

  const itemNameRef = useRef<HTMLInputElement>(null);
  const itemPriceRef = useRef<HTMLInputElement>(null);

  const fetchCategories = async (selectFirst = false) => {
    try {
      setLoading(true);
      const result = await menuRepo.listCategories();
      setCategories(result);
      if (result.length > 0 && (selectFirst || selectedCategoryId === null)) {
        setSelectedCategoryId((prev) => (prev === null ? result[0].id : prev));
      }
    } catch (error) {
      console.error("Failed to fetch categories:", error);
    } finally {
      setLoading(false);
    }
  };

  const fetchItems = async (categoryId: number) => {
    try {
      setItems(await menuRepo.listItems(categoryId));
    } catch (error) {
      console.error("Failed to fetch items:", error);
    }
  };

  useEffect(() => {
    if (dbReady) fetchCategories(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dbReady]);

  useEffect(() => {
    if (dbReady && selectedCategoryId !== null) {
      fetchItems(selectedCategoryId);
      cancelItemEdit();
    } else {
      setItems([]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dbReady, selectedCategoryId]);

  /* --------------------------- categories --------------------------- */

  const addCategory = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = newCategoryName.trim();
    if (!name) return;
    if (categories.some((c) => c.name.toLowerCase() === name.toLowerCase())) {
      toast("A category with this name already exists.", "warning");
      return;
    }
    setNewCategoryName("");
    try {
      await menuRepo.addCategory(name);
      await fetchCategories();
      toast("Category added.", "success");
    } catch (error) {
      console.error("Failed to add category:", error);
      toast("Failed to add category.", "danger");
      await fetchCategories();
    }
  };

  const saveCategoryEdit = async (id: number) => {
    const name = editCategoryName.trim();
    if (!name) return;
    if (categories.some((c) => c.id !== id && c.name.toLowerCase() === name.toLowerCase())) {
      toast("A category with this name already exists.", "warning");
      return;
    }
    setCategories((prev) => prev.map((c) => (c.id === id ? { ...c, name } : c)).sort((a, b) => a.name.localeCompare(b.name)));
    setEditingCategoryId(null);
    try {
      await menuRepo.renameCategory(id, name);
      toast("Category updated.", "success");
    } catch (error) {
      console.error("Failed to update category:", error);
      toast("Failed to update category.", "danger");
      fetchCategories();
    }
  };

  /* ------------------------------ items ----------------------------- */

  const addItem = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = newItemName.trim();
    if (!name || !newItemPrice || selectedCategoryId === null) return;
    if (items.some((i) => i.name.toLowerCase() === name.toLowerCase())) {
      toast("An item with this name already exists in this category.", "warning");
      return;
    }
    const price = parseFloat(newItemPrice);
    setNewItemName("");
    setNewItemPrice("");
    setTimeout(() => itemNameRef.current?.focus(), 0);
    try {
      await menuRepo.addItem(selectedCategoryId, name, price);
      await fetchItems(selectedCategoryId);
      toast("Item added.", "success");
    } catch (error) {
      console.error("Failed to add item:", error);
      toast("Failed to add item.", "danger");
      await fetchItems(selectedCategoryId);
    }
  };

  const startItemEdit = (item: MenuItem) => {
    setEditingItemId(item.id);
    setEditItemName(item.name);
    setEditItemPrice(item.price.toString());
    setEditItemCategoryId(item.category_id);
  };

  const cancelItemEdit = () => {
    setEditingItemId(null);
    setEditItemName("");
    setEditItemPrice("");
    setEditItemCategoryId(null);
  };

  const saveItemEdit = async (id: number) => {
    const name = editItemName.trim();
    if (!name || !editItemPrice || !editItemCategoryId) return;
    const price = parseFloat(editItemPrice);
    const categoryId = editItemCategoryId;

    if (categoryId === selectedCategoryId) {
      if (items.some((i) => i.id !== id && i.name.toLowerCase() === name.toLowerCase())) {
        toast("An item with this name already exists in this category.", "warning");
        return;
      }
    } else {
      try {
        if (await menuRepo.itemNameExistsInCategory(categoryId, name, id)) {
          toast("An item with this name already exists in the target category.", "warning");
          return;
        }
      } catch (error) {
        console.error(error);
      }
    }

    cancelItemEdit();
    try {
      await menuRepo.updateItem(id, name, price, categoryId);
      if (selectedCategoryId) await fetchItems(selectedCategoryId);
      toast("Item updated.", "success");
    } catch (error) {
      console.error("Failed to update item:", error);
      toast("Failed to update item.", "danger");
      if (selectedCategoryId) await fetchItems(selectedCategoryId);
    }
  };

  /* ----------------------------- delete ----------------------------- */

  const executeDelete = async () => {
    if (!deleteConfirmation) return;
    const { type, id } = deleteConfirmation;
    setDeleteConfirmation(null);

    if (type === "category") {
      if (selectedCategoryId === id) setSelectedCategoryId(null);
      try {
        await menuRepo.deleteCategory(id);
        await fetchCategories();
        toast("Category deleted.");
      } catch (error) {
        console.error("Failed to delete category:", error);
        toast("Failed to delete category.", "danger");
        await fetchCategories();
      }
    } else {
      try {
        await menuRepo.deleteItem(id);
        if (selectedCategoryId) await fetchItems(selectedCategoryId);
        toast("Item deleted.");
      } catch (error) {
        console.error("Failed to delete item:", error);
        toast("Failed to delete item.", "danger");
        if (selectedCategoryId) await fetchItems(selectedCategoryId);
      }
    }
  };

  const activeCategory = categories.find((c) => c.id === selectedCategoryId);

  return (
    <div className="split-page">
      {/* Left: categories */}
      <aside className="menu-aside">
        <div className="page-head">
          <h1 style={{ fontSize: "var(--text-xl)" }}>Categories</h1>
          <p>Organize your offerings</p>
        </div>

        <form onSubmit={addCategory} style={{ display: "flex", gap: "var(--space-2)" }}>
          <input
            type="text"
            className="input"
            placeholder="New category…"
            value={newCategoryName}
            onChange={(e) => setNewCategoryName(e.target.value)}
          />
          <button type="submit" className="btn btn--primary" style={{ padding: "0.5rem 0.65rem" }} disabled={!newCategoryName.trim()}>
            <Plus size={18} />
          </button>
        </form>

        <div className="menu-cat-list">
          {loading && categories.length === 0 ? (
            <p className="field-hint" style={{ padding: "var(--space-2)" }}>Loading…</p>
          ) : categories.length === 0 ? (
            <p className="field-hint" style={{ padding: "var(--space-2)" }}>No categories yet.</p>
          ) : (
            categories.map((cat) => {
              const isActive = selectedCategoryId === cat.id;
              return (
                <div
                  key={cat.id}
                  className={`menu-cat ${isActive ? "active" : ""}`}
                  onClick={() => {
                    if (editingCategoryId !== cat.id) setSelectedCategoryId(cat.id);
                  }}
                >
                  {editingCategoryId === cat.id ? (
                    <div style={{ display: "flex", width: "100%", gap: "var(--space-1)" }} onClick={(e) => e.stopPropagation()}>
                      <input
                        autoFocus
                        className="input"
                        style={{ flex: 1, padding: "0.3rem 0.5rem" }}
                        value={editCategoryName}
                        onChange={(e) => setEditCategoryName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") saveCategoryEdit(cat.id);
                          if (e.key === "Escape") setEditingCategoryId(null);
                        }}
                      />
                      <button className="row-action-btn" onClick={() => saveCategoryEdit(cat.id)} title="Save">
                        <Check size={16} />
                      </button>
                      <button className="row-action-btn danger" onClick={() => setEditingCategoryId(null)} title="Cancel">
                        <X size={16} />
                      </button>
                    </div>
                  ) : (
                    <>
                      <span className="menu-cat-name">{cat.name}</span>
                      <div className="menu-cat-actions">
                        <button
                          className="row-action-btn"
                          onClick={(e) => {
                            e.stopPropagation();
                            setEditingCategoryId(cat.id);
                            setEditCategoryName(cat.name);
                          }}
                          title="Edit category"
                        >
                          <Edit2 size={15} />
                        </button>
                        <button
                          className="row-action-btn danger"
                          onClick={(e) => {
                            e.stopPropagation();
                            setDeleteConfirmation({ type: "category", id: cat.id, name: cat.name });
                          }}
                          title="Delete category"
                        >
                          <Trash2 size={15} />
                        </button>
                      </div>
                    </>
                  )}
                </div>
              );
            })
          )}
        </div>
      </aside>

      {/* Right: items */}
      <section className="split-main">
        {selectedCategoryId ? (
          <>
            <div className="page-head">
              <h1 style={{ textTransform: "capitalize" }}>{activeCategory?.name}</h1>
              <p>
                {items.length} {items.length === 1 ? "item" : "items"} in this category
              </p>
            </div>

            <form onSubmit={addItem} className="inline-add-bar">
              <input
                ref={itemNameRef}
                className="input"
                style={{ flex: 2, minWidth: 200 }}
                placeholder="Item name (e.g. Masala Dosa)"
                value={newItemName}
                onChange={(e) => setNewItemName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    itemPriceRef.current?.focus();
                  }
                }}
              />
              <input
                ref={itemPriceRef}
                type="number"
                className="input"
                style={{ flex: 1, minWidth: 120 }}
                placeholder="Price (₹)"
                value={newItemPrice}
                onChange={(e) => setNewItemPrice(e.target.value)}
                step="0.01"
                min="0"
              />
              <button type="submit" className="btn btn--primary" disabled={!newItemName.trim() || !newItemPrice}>
                <Plus size={16} /> Add Item
              </button>
            </form>

            <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
              {items.length === 0 ? (
                <EmptyState icon={<Tag size={24} />} title="No items yet" message="Use the form above to add your first item." />
              ) : (
                <div className="data-list">
                  <div className="data-list-head" style={{ gridTemplateColumns: "1fr 140px 96px" }}>
                    <div>Name</div>
                    <div style={{ textAlign: "right" }}>Price</div>
                    <div style={{ textAlign: "right" }}>Actions</div>
                  </div>
                  {items.map((item) =>
                    editingItemId === item.id ? (
                      <div key={item.id} className="data-row" style={{ display: "flex", gap: "var(--space-2)", alignItems: "center" }}>
                        <input
                          autoFocus
                          className="input"
                          style={{ flex: 2, minWidth: 120 }}
                          value={editItemName}
                          onChange={(e) => setEditItemName(e.target.value)}
                        />
                        <select
                          className="select"
                          style={{ width: 150, flexShrink: 0 }}
                          value={editItemCategoryId ?? ""}
                          onChange={(e) => setEditItemCategoryId(Number(e.target.value))}
                        >
                          {categories.map((c) => (
                            <option key={c.id} value={c.id}>
                              {c.name}
                            </option>
                          ))}
                        </select>
                        <input
                          type="number"
                          className="input"
                          style={{ width: 100, flexShrink: 0, textAlign: "right" }}
                          value={editItemPrice}
                          onChange={(e) => setEditItemPrice(e.target.value)}
                          step="0.01"
                          min="0"
                        />
                        <button className="btn btn--primary btn--sm" onClick={() => saveItemEdit(item.id)} title="Save">
                          <Check size={16} />
                        </button>
                        <button className="btn btn--ghost btn--sm" onClick={cancelItemEdit} title="Cancel">
                          <X size={16} />
                        </button>
                      </div>
                    ) : (
                      <div key={item.id} className="data-row" style={{ gridTemplateColumns: "1fr 140px 96px" }}>
                        <div style={{ fontWeight: "var(--font-medium)" }}>{item.name}</div>
                        <div className="strong" style={{ textAlign: "right" }}>{formatCurrency(item.price)}</div>
                        <div className="data-row-actions">
                          <button className="row-action-btn" onClick={() => startItemEdit(item)} title="Edit item">
                            <Edit2 size={17} />
                          </button>
                          <button
                            className="row-action-btn danger"
                            onClick={() => setDeleteConfirmation({ type: "item", id: item.id, name: item.name })}
                            title="Delete item"
                          >
                            <Trash2 size={17} />
                          </button>
                        </div>
                      </div>
                    )
                  )}
                </div>
              )}
            </div>
          </>
        ) : (
          <EmptyState icon={<Tag size={24} />} title="No category selected" message="Select a category from the left to manage its items." />
        )}
      </section>

      {deleteConfirmation && (
        <ConfirmDialog
          title="Confirm Deletion"
          danger
          message={
            <>
              Are you sure you want to delete the {deleteConfirmation.type}{" "}
              <strong>"{deleteConfirmation.name}"</strong>?
              {deleteConfirmation.type === "category" && (
                <>
                  <br />
                  <br />
                  <span className="text-danger">Warning: All items in this category will also be permanently deleted.</span>
                </>
              )}
            </>
          }
          confirmLabel="Delete"
          onConfirm={executeDelete}
          onCancel={() => setDeleteConfirmation(null)}
        />
      )}
    </div>
  );
}
