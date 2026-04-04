import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { nestGet, nestPost, nestPatch, nestDelete } from "../lib/api";
import DataTable, { type Column } from "../components/DataTable";
import Modal from "../components/Modal";
import ConfirmDialog from "../components/ConfirmDialog";
import { Plus } from "lucide-react";

interface FeaturedItem {
  id: string;
  type: string;
  scUrn: string;
  weight?: number;
  active?: boolean;
}

interface FormData {
  type: string;
  scUrn: string;
  weight: string;
  active: boolean;
}

const emptyForm: FormData = { type: "track", scUrn: "", weight: "0", active: true };

export default function Featured() {
  const qc = useQueryClient();
  const [modal, setModal] = useState<{ mode: "create" | "edit"; item?: FeaturedItem } | null>(null);
  const [deleteItem, setDeleteItem] = useState<FeaturedItem | null>(null);
  const [form, setForm] = useState<FormData>(emptyForm);

  const { data = [], isLoading } = useQuery({
    queryKey: ["featured"],
    queryFn: () => nestGet<FeaturedItem[]>("/admin/featured"),
  });

  const create = useMutation({
    mutationFn: (body: { type: string; scUrn: string; weight?: number; active?: boolean }) =>
      nestPost("/admin/featured", body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["featured"] });
      setModal(null);
    },
  });

  const update = useMutation({
    mutationFn: ({ id, ...body }: { id: string; type: string; scUrn: string; weight?: number; active?: boolean }) =>
      nestPatch(`/admin/featured/${id}`, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["featured"] });
      setModal(null);
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => nestDelete(`/admin/featured/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["featured"] });
      setDeleteItem(null);
    },
  });

  function openCreate() {
    setForm(emptyForm);
    setModal({ mode: "create" });
  }

  function openEdit(item: FeaturedItem) {
    setForm({
      type: item.type,
      scUrn: item.scUrn,
      weight: String(item.weight ?? 0),
      active: item.active !== false,
    });
    setModal({ mode: "edit", item });
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const body = {
      type: form.type,
      scUrn: form.scUrn,
      weight: Number(form.weight) || 0,
      active: form.active,
    };
    if (modal?.mode === "edit" && modal.item) {
      update.mutate({ ...body, id: modal.item.id });
    } else {
      create.mutate(body);
    }
  }

  const columns: Column<FeaturedItem>[] = [
    {
      key: "type",
      label: "Type",
      render: (f) => (
        <span className="px-2 py-1 rounded-lg text-xs bg-indigo-500/20 text-indigo-300">
          {f.type}
        </span>
      ),
    },
    { key: "scUrn", label: "SC URN" },
    { key: "weight", label: "Weight", render: (f) => String(f.weight ?? 0) },
    {
      key: "active",
      label: "Active",
      render: (f) =>
        f.active !== false ? (
          <span className="px-2 py-1 rounded-lg text-xs bg-emerald-500/20 text-emerald-300">
            Yes
          </span>
        ) : (
          <span className="px-2 py-1 rounded-lg text-xs bg-white/10 text-white/40">
            No
          </span>
        ),
    },
  ];

  const inputClass =
    "w-full px-4 py-3 rounded-xl bg-white/5 border border-white/10 text-white placeholder-white/30 outline-none focus:border-white/25 transition-colors";

  const isPending = create.isPending || update.isPending;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-white/90">Featured</h1>
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
          keyExtractor={(f) => f.id}
          onEdit={openEdit}
          onDelete={setDeleteItem}
        />
      )}

      <Modal
        open={!!modal}
        onClose={() => setModal(null)}
        title={modal?.mode === "edit" ? "Edit Featured" : "Add Featured"}
      >
        <form onSubmit={handleSubmit} className="space-y-4">
          <select
            className={inputClass}
            value={form.type}
            onChange={(e) => setForm({ ...form, type: e.target.value })}
          >
            <option value="track">Track</option>
            <option value="playlist">Playlist</option>
            <option value="user">User</option>
          </select>
          <input
            className={inputClass}
            placeholder="SC URN (e.g. soundcloud:tracks:123)"
            value={form.scUrn}
            onChange={(e) => setForm({ ...form, scUrn: e.target.value })}
            required
          />
          <input
            className={inputClass}
            type="number"
            placeholder="Weight"
            value={form.weight}
            onChange={(e) => setForm({ ...form, weight: e.target.value })}
          />
          <label className="flex items-center gap-3 text-sm text-white/70 cursor-pointer">
            <input
              type="checkbox"
              checked={form.active}
              onChange={(e) => setForm({ ...form, active: e.target.checked })}
              className="rounded"
            />
            Active
          </label>
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
              disabled={isPending}
              className="px-4 py-2 rounded-xl text-sm bg-indigo-500/20 text-indigo-300 hover:bg-indigo-500/30 border border-indigo-500/20 transition-all disabled:opacity-50"
            >
              {isPending ? "Saving..." : "Save"}
            </button>
          </div>
        </form>
      </Modal>

      <ConfirmDialog
        open={!!deleteItem}
        onClose={() => setDeleteItem(null)}
        onConfirm={() => deleteItem && remove.mutate(deleteItem.id)}
        title="Delete Featured"
        message={`Remove featured item "${deleteItem?.scUrn}"?`}
        loading={remove.isPending}
      />
    </div>
  );
}
