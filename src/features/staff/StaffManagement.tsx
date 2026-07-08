import { useEffect, useState } from "react";
import { Trash2, UserPlus } from "lucide-react";
import { STAFF_ROLES } from "../../config/constants";
import { useToast } from "../../hooks/useToast";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import EmptyState from "../../components/ui/EmptyState";
import * as staffRepo from "../../db/repositories/staffRepo";
import type { StaffMember } from "../../types";

interface StaffManagementProps {
  dbReady: boolean;
}

export default function StaffManagement({ dbReady }: StaffManagementProps) {
  const { toast } = useToast();
  const [staff, setStaff] = useState<StaffMember[]>([]);
  const [name, setName] = useState("");
  const [role, setRole] = useState("Waiter");
  const [phone, setPhone] = useState("");
  const [saving, setSaving] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<StaffMember | null>(null);

  const fetchStaff = async () => {
    try {
      setStaff(await staffRepo.listStaff());
    } catch (error) {
      console.error("Failed to fetch staff:", error);
    }
  };

  useEffect(() => {
    if (dbReady) fetchStaff();
  }, [dbReady]);

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    try {
      setSaving(true);
      await staffRepo.addStaff(name.trim(), role, phone);
      setName("");
      setPhone("");
      await fetchStaff();
      toast("Staff member added.", "success");
    } catch (error) {
      console.error("Failed to add staff:", error);
      toast("Failed to add staff member.", "danger");
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    try {
      await staffRepo.deleteStaff(target.id);
      await fetchStaff();
      toast("Staff member removed.");
    } catch (error) {
      console.error("Failed to delete staff:", error);
      toast("Failed to remove staff member.", "danger");
    }
  };

  return (
    <div className="page settings-page">
      <div className="page-head">
        <h1>Staff Management</h1>
        <p>Manage your team members and roles</p>
      </div>

      <div className="section">
        <div className="section-head">
          <UserPlus size={14} /> Add Staff Member
        </div>
        <form onSubmit={handleAdd} className="inline-add-bar">
          <input
            type="text"
            className="input"
            style={{ flex: 2, minWidth: 200 }}
            placeholder="Full Name (e.g. John Doe)"
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={saving}
          />
          <select className="select" style={{ flex: 1, minWidth: 130 }} value={role} onChange={(e) => setRole(e.target.value)} disabled={saving}>
            {STAFF_ROLES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
          <input
            type="text"
            className="input"
            style={{ flex: 1.5, minWidth: 150 }}
            placeholder="Phone (e.g. 9876543210)"
            value={phone}
            onChange={(e) => setPhone(e.target.value.replace(/\D/g, "").slice(0, 10))}
            disabled={saving}
          />
          <button type="submit" className="btn btn--primary" disabled={saving || !name.trim()}>
            <UserPlus size={16} /> Add Staff
          </button>
        </form>
      </div>

      <div className="section" style={{ flex: 1, minHeight: 0 }}>
        <div className="section-head">
          Team Members
          <span className="flex-spacer" />
          <span style={{ textTransform: "none", letterSpacing: 0 }}>{staff.length} total</span>
        </div>
        {staff.length === 0 ? (
          <EmptyState
            icon={<UserPlus size={24} />}
            title="No staff members yet"
            message="Use the form above to add your first team member."
          />
        ) : (
          <div className="data-list">
            <div className="data-list-head" style={{ gridTemplateColumns: "2fr 1fr 1.5fr 70px" }}>
              <div>Name</div>
              <div>Role</div>
              <div>Phone</div>
              <div style={{ textAlign: "right" }}>Actions</div>
            </div>
            {staff.map((member) => (
              <div key={member.id} className="data-row" style={{ gridTemplateColumns: "2fr 1fr 1.5fr 70px" }}>
                <div className="strong">{member.name}</div>
                <div>
                  <span className="role-pill">{member.role}</span>
                </div>
                <div className="text-secondary">{member.phone || "-"}</div>
                <div className="data-row-actions">
                  <button className="row-action-btn danger" onClick={() => setDeleteTarget(member)} title="Remove staff">
                    <Trash2 size={18} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {deleteTarget && (
        <ConfirmDialog
          title="Remove Staff Member"
          message={
            <>
              Remove <strong>{deleteTarget.name}</strong> from the team?
            </>
          }
          confirmLabel="Remove"
          danger
          onConfirm={handleDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  );
}
