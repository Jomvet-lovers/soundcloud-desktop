import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
}

export default function Modal({ open, onClose, title, children }: ModalProps) {
  return (
    <Dialog.Root open={open} onOpenChange={(v) => !v && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50" />
        <Dialog.Content className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-full max-w-md backdrop-blur-xl bg-slate-900/95 border border-white/10 rounded-2xl shadow-2xl p-6">
          <div className="flex items-center justify-between mb-6">
            <Dialog.Title className="text-lg font-semibold text-white/90">
              {title}
            </Dialog.Title>
            <Dialog.Close className="text-white/40 hover:text-white/80 transition-colors">
              <X size={20} />
            </Dialog.Close>
          </div>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
