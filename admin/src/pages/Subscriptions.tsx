import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { streamGet, streamPost, streamDelete } from "../lib/api";
import DataTable, { type Column } from "../components/DataTable";
import Modal from "../components/Modal";
import ConfirmDialog from "../components/ConfirmDialog";
import { Plus } from "lucide-react";

interface Subscription {
  user_urn: string;
  exp_date: string;
}

export default function Subscriptions() {
  const qc = useQueryClient();
  const [modal, setModal] = useState<{ mode: "create" | "edit"; item?: Subscription } | null>(null);
  const [deleteItem, setDeleteItem] = useState<Subscription | null>(null);
  const [form, setForm] = useState({ user_urn: "", exp_date: "" });

  const { data = [], isLoading } = useQuery({
    queryKey: ["subscriptions"],
    queryFn: () => streamGet<Subscription[]>("/admin/subscriptions"),
  });

  const upsert = useMutation({
    mutationFn: (body: { user_urn: string; exp_date: string }) =>
      streamPost("/admin/subscriptions", body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["subscriptions"] });
      setModal(null);
    },
  });

  const remove = useMutation({
    mutationFn: (urn: string) => streamDelete(`/admin/subscriptions/${urn}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["subscriptions"] });
      setDeleteItem(null);
    },
  });

  function openCreate() {
    setForm({ user_urn: "", exp_date: "" });
    setModal({ mode: "create" });
  }

  function openEdit(item: Subscription) {
    setForm({ user_urn: item.user_urn, exp_date: item.exp_date.split("T")[0] });
    setModal({ mode: "edit", item });
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    upsert.mutate(form);
  }

  const isExpired = (d: string) => new Date(d) < new Date();

  const columns: Column<Subscription>[] = [
    { key: "user_urn", label: "User URN" },
    {
      key: "exp_date",
      label: "Expiry Date",
      render: (s) => new Date(s.exp_date).toLocaleDateString(),
    },
    {
      key: "status",
      label: "Status",
      render: (s) =>
        isExpired(s.exp_date) ? (
          <span className="px-2 py-1 rounded-lg text-xs bg-red-500/20 text-red-300">
            Expired
          </span>
        ) : (
          <span className="px-2 py-1 rounded-lg text-xs bg-emerald-500/20 text-emerald-300">
            Active
          </span>
        ),
    },
  ];

  const inputClass =
    "w-full px-4 py-3 rounded-xl bg-white/5 border border-white/10 text-white placeholder-white/30 outline-none focus:border-white/25 transition-colors";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-white/90">Subscriptions</h1>
        <button
          onClick={openCreate}
          className="flex items-center gap-2 px-4 py-2 rounded-xl bg-white/10 hover:bg-white/20 border border-white/10 text-sm text-white font-medium transition-all"
        >
          <Plus size={16} />
          Add
        </button>
      </div>

      {isLoading ? (
        <div className="animate-pulse space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-12 bg-white/5 rounded-xl" />
          ))}
        </div>
      ) : (
        <DataTable
          columns={columns}
          data={data}
          keyExtractor={(s) => s.user_urn}
          onEdit={openEdit}
          onDelete={setDeleteItem}
        />
      )}

      {/* Create/Edit Modal */}
      <Modal
        open={!!modal}
        onClose={() => setModal(null)}
        title={modal?.mode === "edit" ? "Edit Subscription" : "Add Subscription"}
      >
        <form onSubmit={handleSubmit} className="space-y-4">
          <input
            className={inputClass}
            placeholder="User URN (e.g. soundcloud:users:123)"
            value={form.user_urn}
            onChange={(e) => setForm({ ...form, user_urn: e.target.value })}
            disabled={modal?.mode === "edit"}
            required
          />
          <input
            className={inputClass}
            type="date"
            value={form.exp_date}
            onChange={(e) => setForm({ ...form, exp_date: e.target.value })}
            required
          />
          <div className="flex gap-3 justify-end pt-2">
            <button
              type="button"
              onClick={() => setModal(null)}
              className="px-4 py-2 rounded-xl text-sm text-white/60 hover:text-white/90 hover:bg-white/5 transition-all"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={upsert.isPending}
              className="px-4 py-2 rounded-xl text-sm bg-indigo-500/20 text-indigo-300 hover:bg-indigo-500/30 border border-indigo-500/20 transition-all disabled:opacity-50"
            >
              {upsert.isPending ? "Saving..." : "Save"}
            </button>
          </div>
        </form>
      </Modal>

      {/* Delete Confirm */}
      <ConfirmDialog
        open={!!deleteItem}
        onClose={() => setDeleteItem(null)}
        onConfirm={() => deleteItem && remove.mutate(deleteItem.user_urn)}
        title="Delete Subscription"
        message={`Remove subscription for ${deleteItem?.user_urn}?`}
        loading={remove.isPending}
      />
    </div>
  );
}
