"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

import { PageHeader } from "@/components/north-shell";
import { ApiError, requestJson } from "@/lib/api/client";
import { getCurrentUser } from "@/lib/api/current-user";
import type { CurrentUser } from "@/lib/api/contracts";
import { useEffect } from "react";

export function UserProfile() {
  const router = useRouter();
  const [user, setUser] = useState<CurrentUser | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [loggingOut, setLoggingOut] = useState(false);

  useEffect(() => {
    let active = true;
    void getCurrentUser().then((current) => {
      if (active) setUser(current);
    }).catch((cause: unknown) => {
      if (!active) return;
      setError(cause instanceof ApiError && cause.status === 401 ? "登入已失效，請重新登入。" : "無法載入個人資料，請重試。");
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, []);

  async function logout() {
    setLoggingOut(true);
    setError(null);
    try {
      await requestJson("/auth/logout", { method: "POST" });
      setUser(null);
      window.dispatchEvent(new Event("north:logout"));
      router.replace("/login");
    } catch {
      setError("登出失敗。登入狀態未變更，請重試。");
      setLoggingOut(false);
    }
  }

  return (
    <main className="north-container">
      <PageHeader description="檢視目前登入帳戶資訊。" title="個人資料" />
      {loading ? <p role="status" className="pt-6">正在載入個人資料…</p> : null}
      {error ? <p role="alert" className="pt-6 text-destructive">{error}</p> : null}
      {user ? (
        <section aria-label="帳戶資訊" className="mt-6 max-w-xl rounded-lg border border-border bg-card p-6">
          <dl className="grid gap-5">
            <div><dt className="text-sm text-muted-foreground">電子郵件</dt><dd className="mt-1 font-medium" data-testid="profile-email">{user.email}</dd></div>
            <div><dt className="text-sm text-muted-foreground">角色</dt><dd className="mt-1 font-medium" data-testid="profile-role">{user.role}</dd></div>
          </dl>
          <button className="mt-6 rounded-md border border-border px-4 py-2 text-sm font-medium hover:bg-muted disabled:opacity-50" disabled={loggingOut} onClick={logout} type="button">
            {loggingOut ? "正在登出…" : "登出"}
          </button>
        </section>
      ) : null}
    </main>
  );
}
