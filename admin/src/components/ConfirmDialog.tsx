import Modal from "./Modal";

interface ConfirmDialogProps {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  loading?: boolean;
}

export default function ConfirmDialog({
  open,
  onClose,
  onConfirm,
  title,
  message,
  loading,
}: ConfirmDialogProps) {
  return (
    <Modal open={open} onClose={onClose} title={title}>
      <p className="text-white/60 text-sm mb-6">{message}</p>
      <div className="flex gap-3 justify-end">
        <button
          onClick={onClose}
          className="px-4 py-2 rounded-xl text-sm text-white/60 hover:text-white/90 hover:bg-white/5 transition-all"
        >
          Cancel
        </button>
        <button
          onClick={onConfirm}
          disabled={loading}
          className="px-4 py-2 rounded-xl text-sm bg-red-500/20 text-red-300 hover:bg-red-500/30 border border-red-500/20 transition-all disabled:opacity-50"
        >
          Delete
        </button>
      </div>
    </Modal>
  );
}
