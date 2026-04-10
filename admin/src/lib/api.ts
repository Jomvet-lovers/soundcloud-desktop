import { clearAuth, getAuth } from "./auth";

async function request<T>(
  base: "nest" | "streaming",
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const auth = getAuth();
  if (!auth) throw new Error("Not authenticated");

  const url = base === "nest" ? auth.nestUrl : auth.streamingUrl;
  const token = auth.nestToken;

  const res = await fetch(`${url.replace(/\/$/, "")}${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      "x-admin-token": token,
      ...options.headers,
    },
  });

  if (res.status === 401 || res.status === 403) {
    clearAuth();
    window.location.href = "/login";
    throw new Error("Unauthorized");
  }

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${res.status}: ${text}`);
  }

  const text = await res.text();
  if (!text) {
    return undefined as T;
  }

  const contentType = res.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    return JSON.parse(text) as T;
  }

  try {
    return JSON.parse(text) as T;
  } catch {
    return text as T;
  }
}

// NestJS
export const nestGet = <T>(path: string) => request<T>("nest", path);
export const nestPost = <T>(path: string, body: unknown) =>
  request<T>("nest", path, { method: "POST", body: JSON.stringify(body) });
export const nestPatch = <T>(path: string, body: unknown) =>
  request<T>("nest", path, { method: "PATCH", body: JSON.stringify(body) });
export const nestDelete = (path: string) =>
  request<void>("nest", path, { method: "DELETE" });

// Streaming
export const streamGet = <T>(path: string) => request<T>("streaming", path);

// Health checks (no auth redirect)
export async function checkNestHealth(url: string, token: string): Promise<boolean> {
  try {
    const res = await fetch(`${url.replace(/\/$/, "")}/admin/stats`, {
      headers: { "x-admin-token": token },
    });
    return res.ok;
  } catch {
    return false;
  }
}

export async function checkStreamingHealth(url: string): Promise<boolean> {
  try {
    const res = await fetch(`${url.replace(/\/$/, "")}/health`);
    return res.ok;
  } catch {
    return false;
  }
}
