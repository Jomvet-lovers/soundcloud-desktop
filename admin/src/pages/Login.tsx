import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { setAuth } from "../lib/auth";
import { checkNestHealth, checkStreamingHealth } from "../lib/api";
import { Server, Radio, Loader2 } from "lucide-react";

export default function Login() {
  const navigate = useNavigate();
  const [nestUrl, setNestUrl] = useState("");
  const [nestToken, setNestToken] = useState("");
  const [streamingUrl, setStreamingUrl] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);

    try {
      const [nestOk, streamOk] = await Promise.all([
        checkNestHealth(nestUrl, nestToken),
        checkStreamingHealth(streamingUrl),
      ]);

      if (!nestOk && !streamOk) {
        setError("Failed to connect to both services");
        return;
      }
      if (!nestOk) {
        setError("Failed to connect to NestJS backend");
        return;
      }
      if (!streamOk) {
        setError("Failed to connect to Streaming service");
        return;
      }

      setAuth({ nestUrl, nestToken, streamingUrl });
      navigate("/", { replace: true });
    } catch {
      setError("Connection failed");
    } finally {
      setLoading(false);
    }
  }

  const inputClass =
    "w-full px-4 py-3 rounded-xl bg-white/5 border border-white/10 text-white placeholder-white/30 outline-none focus:border-white/25 transition-colors";

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-md backdrop-blur-xl bg-white/5 border border-white/10 rounded-2xl shadow-2xl p-8 space-y-6"
      >
        <h1 className="text-2xl font-semibold text-center text-white/90">
          Admin Panel
        </h1>

        <div className="space-y-4">
          <div className="flex items-center gap-2 text-white/50 text-sm font-medium">
            <Server size={16} />
            NestJS Backend
          </div>
          <input
            className={inputClass}
            placeholder="URL (e.g. https://api.example.com)"
            value={nestUrl}
            onChange={(e) => setNestUrl(e.target.value)}
            required
          />
          <input
            className={inputClass}
            placeholder="Admin Token"
            type="password"
            value={nestToken}
            onChange={(e) => setNestToken(e.target.value)}
            required
          />
        </div>

        <div className="space-y-4">
          <div className="flex items-center gap-2 text-white/50 text-sm font-medium">
            <Radio size={16} />
            Streaming Service
          </div>
          <input
            className={inputClass}
            placeholder="URL (e.g. https://stream.example.com)"
            value={streamingUrl}
            onChange={(e) => setStreamingUrl(e.target.value)}
            required
          />
        </div>

        {error && (
          <p className="text-red-400 text-sm text-center">{error}</p>
        )}

        <button
          type="submit"
          disabled={loading}
          className="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 border border-white/10 text-white font-medium transition-all disabled:opacity-50 flex items-center justify-center gap-2"
        >
          {loading && <Loader2 size={18} className="animate-spin" />}
          Connect
        </button>
      </form>
    </div>
  );
}
