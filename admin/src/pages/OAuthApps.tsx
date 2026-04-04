import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { nestGet, nestPost, nestPatch } from "../lib/api";
import DataTable, { type Column } from "../components/DataTable";
import Modal from "../components/Modal";
import { Plus, Power } from "lucide-react";

interface OAuthApp {
  id: string;
  name: string;
  clientId: string;
  redirectUri: string;
  active?: boolean;
  createdAt?: string;
}

interface FormData {
  name: string;
  clientId: string;
  clientSecret: string;
  redirectUri: string;
  active: boolean;
}

type UpdatePayload = Partial<FormData> & { id: string };

const emptyForm: FormData = {
  name: "",
  clientId: "",
  clientSecret: "",
  redirectUri: "",
  active: true,
};

export default function OAuthApps() {
  const qc = useQueryClient();
  const [modal, setModal] = useState<{ mode: "create" | "edit"; item?: OAuthApp } | null>(null);
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
    mutationFn: ({ id, ...body }: UpdatePayload) =>
      nestPatch(`/oauth-apps/${id}`, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth-apps"] });
      setModal(null);
    },
  });

  const toggleActive = useMutation({
    mutationFn: ({ id, active }: { id: string; active: boolean }) =>
      nestPatch(`/oauth-apps/${id}`, { active }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth-apps"] });
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
      clientSecret: "",
      redirectUri: item.redirectUri,
      active: item.active !== false,
    });
    setModal({ mode: "edit", item });
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (modal?.mode === "edit" && modal.item) {
      update.mutate({
        id: modal.item.id,
        name: form.name,
        clientId: form.clientId,
        redirectUri: form.redirectUri,
        active: form.active,
        ...(form.clientSecret ? { clientSecret: form.clientSecret } : {}),
      });
    } else {
      create.mutate({
        name: form.name,
        clientId: form.clientId,
        clientSecret: form.clientSecret,
        redirectUri: form.redirectUri,
        active: form.active,
      });
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
    {
      key: "toggle",
      label: "Toggle",
      render: (a) => (
        <button
          type="button"
          onClick={() => toggleActive.mutate({ id: a.id, active: a.active === false })}
          disabled={toggleActive.isPending}
          className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg border border-white/10 bg-white/5 text-xs text-white/70 hover:bg-white/10 disabled:opacity-50"
        >
          <Power size={14} />
          {a.active === false ? "Enable" : "Disable"}
        </button>
      ),
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
            required={modal?.mode !== "edit"}
          />
          <input
            className={inputClass}
            placeholder="Redirect URI"
            value={form.redirectUri}
            onChange={(e) => setForm({ ...form, redirectUri: e.target.value })}
            required
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
    </div>
  );
}
