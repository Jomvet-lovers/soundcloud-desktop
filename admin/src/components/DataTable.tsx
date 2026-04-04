import { Pencil, Trash2 } from "lucide-react";

export interface Column<T> {
  key: string;
  label: string;
  render?: (item: T) => React.ReactNode;
}

interface DataTableProps<T> {
  columns: Column<T>[];
  data: T[];
  onEdit?: (item: T) => void;
  onDelete?: (item: T) => void;
  keyExtractor: (item: T) => string;
}

export default function DataTable<T>({
  columns,
  data,
  onEdit,
  onDelete,
  keyExtractor,
}: DataTableProps<T>) {
  return (
    <div className="backdrop-blur-xl bg-white/5 border border-white/10 rounded-2xl shadow-2xl overflow-hidden">
      <table className="w-full">
        <thead>
          <tr className="border-b border-white/10">
            {columns.map((col) => (
              <th
                key={col.key}
                className="text-left px-5 py-3 text-sm font-medium text-white/50"
              >
                {col.label}
              </th>
            ))}
            {(onEdit || onDelete) && (
              <th className="px-5 py-3 text-right text-sm font-medium text-white/50 w-24">
                Actions
              </th>
            )}
          </tr>
        </thead>
        <tbody>
          {data.length === 0 ? (
            <tr>
              <td
                colSpan={columns.length + (onEdit || onDelete ? 1 : 0)}
                className="text-center py-8 text-white/30"
              >
                No data
              </td>
            </tr>
          ) : (
            data.map((item) => (
              <tr
                key={keyExtractor(item)}
                className="border-b border-white/5 hover:bg-white/5 transition-colors"
              >
                {columns.map((col) => (
                  <td key={col.key} className="px-5 py-3 text-sm text-white/80">
                    {col.render
                      ? col.render(item)
                      : String((item as Record<string, unknown>)[col.key] ?? "")}
                  </td>
                ))}
                {(onEdit || onDelete) && (
                  <td className="px-5 py-3 text-right">
                    <div className="flex items-center justify-end gap-2">
                      {onEdit && (
                        <button
                          onClick={() => onEdit(item)}
                          className="p-1.5 rounded-lg text-white/40 hover:text-white/80 hover:bg-white/10 transition-all"
                        >
                          <Pencil size={15} />
                        </button>
                      )}
                      {onDelete && (
                        <button
                          onClick={() => onDelete(item)}
                          className="p-1.5 rounded-lg text-white/40 hover:text-red-400 hover:bg-white/10 transition-all"
                        >
                          <Trash2 size={15} />
                        </button>
                      )}
                    </div>
                  </td>
                )}
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
