import { useQuery } from "@tanstack/react-query";
import { nestGet, streamGet } from "../lib/api";
import { Activity, Users, Clock, Database, Wifi, WifiOff } from "lucide-react";
import GlassCard from "../components/GlassCard";

interface Stats {
  active_24h: number;
  active_7d: number;
  active_30d: number;
  total_sessions: number;
}

export default function Dashboard() {
  const stats = useQuery({
    queryKey: ["admin-stats"],
    queryFn: () => nestGet<Stats>("/admin/stats"),
    refetchInterval: 30_000,
  });

  const health = useQuery({
    queryKey: ["streaming-health"],
    queryFn: () => streamGet<string>("/health"),
    refetchInterval: 30_000,
    retry: false,
  });

  const cards = stats.data
    ? [
        { label: "Active 24h", value: stats.data.active_24h, icon: Activity },
        { label: "Active 7d", value: stats.data.active_7d, icon: Clock },
        { label: "Active 30d", value: stats.data.active_30d, icon: Users },
        { label: "Total Sessions", value: stats.data.total_sessions, icon: Database },
      ]
    : [];

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-white/90">Dashboard</h1>
        <div className="flex items-center gap-2">
          {health.isSuccess ? (
            <span className="flex items-center gap-2 text-sm text-emerald-400">
              <Wifi size={16} />
              Streaming Online
            </span>
          ) : health.isError ? (
            <span className="flex items-center gap-2 text-sm text-red-400">
              <WifiOff size={16} />
              Streaming Offline
            </span>
          ) : (
            <span className="text-sm text-white/40">Checking...</span>
          )}
        </div>
      </div>

      {stats.isLoading ? (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {[...Array(4)].map((_, i) => (
            <GlassCard key={i} className="p-6 animate-pulse">
              <div className="h-4 w-20 bg-white/10 rounded mb-3" />
              <div className="h-8 w-16 bg-white/10 rounded" />
            </GlassCard>
          ))}
        </div>
      ) : stats.isError ? (
        <GlassCard className="p-6">
          <p className="text-red-400 text-sm">Failed to load stats</p>
        </GlassCard>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {cards.map(({ label, value, icon: Icon }) => (
            <GlassCard
              key={label}
              className="p-6 hover:scale-[1.02] transition-transform"
            >
              <div className="flex items-center gap-2 text-white/50 text-sm mb-2">
                <Icon size={16} />
                {label}
              </div>
              <p className="text-3xl font-bold text-white/90">{value}</p>
            </GlassCard>
          ))}
        </div>
      )}
    </div>
  );
}
