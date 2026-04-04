import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { nestGet, nestPost, nestPatch, nestDelete } from "../lib/api";
import DataTable, { type Column } from "../components/DataTable";
import Modal from "../components/Modal";
import ConfirmDialog from "../components/ConfirmDialog";
import { Plus } from "lucide-react";

interface OAuthApp {
  id: string;
  name: string;
  clientId: string;
  clientSecret: string;
  redirectUri: string;
  active?: boolean;
  createdAt?: string;
}

type FormData = Omit<OAuthApp, "id" | "active" | "createdAt">;

const emptyForm: FormData = { name: "", clientId: "", clientSecret: "", redirectUri: "" };

export default function OAuthApps() {
  const qc = useQueryClient();
  const [modal, setModal] = useState<{ mode: "create" | "edit"; item?: OAuthApp } | null>(null);
  const [deleteItem, setDeleteItem] = useState<OAuthApp | null>(null);
  const [form, setForm] = useState<FormData>(emptyForm);

  const { data = [], isLoading } = useQuery({
    queryKey: ["oauth-apps"],
    queryFn: () => nestGet<OAuthApp[]>("/oauth-apps"),
  });

  const create = useMutation({
    mutationFn: (body: FormData) => nestPost("/oauth-apps", body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth-apps"] });
      setModal(null);
    },
  });

  const update = useMutation({
    mutationFn: ({ id, ...body }: FormData & { id: string }) =>
      nestPatch(`/oauth-apps/${id}`, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth-apps"] });
      setModal(null);
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => nestDelete(`/oauth-apps/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth-apps"] });
      setDeleteItem(null);
    },
  });

  function openCreate() {
    setForm(emptyForm);
    setModal({ mode: "create" });
  }

  function openEdit(item: OAuthApp) {
    setForm({
      name: item.name,
      clientId: item.clientId,
      clientSecret: item.clientSecret,
      redirectUri: item.redirectUri,
    });
    setModal({ mode: "edit", item });
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (modal?.mode === "edit" && modal.item) {
      update.mutate({ ...form, id: modal.item.id });
    } else {
      create.mutate(form);
    }
  }

  const columns: Column<OAuthApp>[] = [
    { key: "name", label: "Name" },
    { key: "clientId", label: "Client ID" },
    {
      key: "active",
      label: "Active",
      render: (a) =>
        a.active !== false ? (
          <span className="px-2 py-1 rounded-lg text-xs bg-emerald-500/20 text-emerald-300">
            Yes
          </span>
        ) : (
          <span className="px-2 py-1 rounded-lg text-xs bg-white/10 text-white/40">
            No
          </span>
        ),
    },
    {
      key: "createdAt",
      label: "Created",
      render: (a) =>
        a.createdAt ? new Date(a.createdAt).toLocaleDateString() : "—",
    },
  ];

  const inputClass =
    "w-full px-4 py-3 rounded-xl bg-white/5 border border-white/10 text-white placeholder-white/30 outline-none focus:border-white/25 transition-colors";

  const isPending = create.isPending || update.isPending;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-white/90">OAuth Apps</h1>
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
          keyExtractor={(a) => a.id}
          onEdit={openEdit}
          onDelete={setDeleteItem}
        />
      )}

      <Modal
        open={!!modal}
        onClose={() => setModal(null)}
        title={modal?.mode === "edit" ? "Edit OAuth App" : "Add OAuth App"}
      >
        <form onSubmit={handleSubmit} className="space-y-4">
          <input
            className={inputClass}
            placeholder="App Name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            required
          />
          <input
            className={inputClass}
            placeholder="Client ID"
            value={form.clientId}
            onChange={(e) => setForm({ ...form, clientId: e.target.value })}
            required
          />
          <input
            className={inputClass}
            placeholder="Client Secret"
            type="password"
            value={form.clientSecret}
            onChange={(e) => setForm({ ...form, clientSecret: e.target.value })}
            required
          />
          <input
            className={inputClass}
            placeholder="Redirect URI"
            value={form.redirectUri}
            onChange={(e) => setForm({ ...form, redirectUri: e.target.value })}
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
        title="Delete OAuth App"
        message={`Delete "${deleteItem?.name}"?`}
        loading={remove.isPending}
      />
    </div>
  );
}
