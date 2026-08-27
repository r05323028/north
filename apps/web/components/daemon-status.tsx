"use client";

import Link from "next/link";
import { useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

type Daemon = {
  daemon_id: string;
  label: string;
  created_by: string;
  created_at: string;
  revoked_at: string | null;
  last_seen_at: string | null;
  connected: boolean;
  protocol_version: string;
  capabilities: string[];
};

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: "include",
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });
  if (!response.ok) {
    throw new Error(`Request failed (${response.status})`);
  }
  return response.status === 204 ? (undefined as T) : response.json();
}

function statusFor(daemon: Daemon): string {
  if (daemon.revoked_at) return "Revoked";
  return daemon.connected ? "Live" : "Offline";
}

export function DaemonStatus() {
  const [daemons, setDaemons] = useState<Daemon[]>([]);
  const [loading, setLoading] = useState(true);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void request<Daemon[]>("/daemons")
      .then((value) => {
        if (active) setDaemons(value);
      })
      .catch((cause) => {
        if (active) {
          setError(
            cause instanceof Error ? cause.message : "Unable to load daemons",
          );
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  async function revoke(daemon: Daemon) {
    if (
      daemon.revoked_at ||
      !window.confirm(`Revoke access for ${daemon.label}?`)
    ) {
      return;
    }
    setRevoking(daemon.daemon_id);
    setError(null);
    try {
      await request<void>(
        `/daemons/${encodeURIComponent(daemon.daemon_id)}/revoke`,
        { method: "POST" },
      );
      setDaemons((current) =>
        current.map((item) =>
          item.daemon_id === daemon.daemon_id
            ? {
                ...item,
                connected: false,
                revoked_at: new Date().toISOString(),
              }
            : item,
        ),
      );
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Unable to revoke daemon",
      );
    } finally {
      setRevoking(null);
    }
  }

  if (loading) return <p role="status">Loading daemons…</p>;
  if (error && daemons.length === 0) return <p role="alert">{error}</p>;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Daemon status</CardTitle>
        <CardDescription>
          Connected execution hosts and their server-owned credentials.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {error && (
          <p className="mb-4 text-sm text-destructive" role="alert">
            {error}
          </p>
        )}
        {daemons.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No daemons registered.
          </p>
        ) : (
          <div className="overflow-x-auto rounded-md border">
            <table className="w-full text-left text-sm">
              <thead className="bg-muted/50">
                <tr>
                  <th className="px-4 py-3 font-medium">Daemon</th>
                  <th className="px-4 py-3 font-medium">Status</th>
                  <th className="px-4 py-3 font-medium">Owner</th>
                  <th className="px-4 py-3 font-medium">Last seen</th>
                  <th className="px-4 py-3 font-medium">Capabilities</th>
                  <th className="px-4 py-3 font-medium">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {daemons.map((daemon) => (
                  <tr className="border-t" key={daemon.daemon_id}>
                    <td className="px-4 py-3">
                      <div className="font-medium">{daemon.label}</div>
                      <div className="text-xs text-muted-foreground">
                        {daemon.daemon_id} · protocol {daemon.protocol_version}
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <Badge>{statusFor(daemon)}</Badge>
                    </td>
                    <td className="px-4 py-3">{daemon.created_by}</td>
                    <td className="px-4 py-3">
                      {daemon.last_seen_at ? (
                        <time dateTime={daemon.last_seen_at}>
                          {daemon.last_seen_at}
                        </time>
                      ) : (
                        "Never"
                      )}
                    </td>
                    <td className="px-4 py-3">
                      {daemon.capabilities.length > 0
                        ? daemon.capabilities.join(", ")
                        : "None"}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <Button
                        disabled={
                          Boolean(daemon.revoked_at) ||
                          revoking === daemon.daemon_id
                        }
                        onClick={() => void revoke(daemon)}
                        size="sm"
                        variant="destructive"
                      >
                        {daemon.revoked_at ? "Revoked" : "Revoke"}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <p className="mt-4 text-xs text-muted-foreground">
          Runtime internals stay on daemon hosts. Server authorization controls
          every status and revocation request.
        </p>
        <Button className="mt-4" asChild variant="outline">
          <Link href="/">Back to North</Link>
        </Button>
      </CardContent>
    </Card>
  );
}
