"use client";

import Link from "next/link";
import { FormEvent, useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

type Repository = {
  id: string;
  name: string;
  url: string;
  description: string;
  created_at: string;
  updated_at: string;
  disabled_at: string | null;
  enabled: boolean;
};

type RepositoryForm = {
  name: string;
  url: string;
  description: string;
};

type CurrentUser = { role: string };

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
    let message = `Request failed (${response.status})`;
    try {
      const body = (await response.json()) as {
        error?: string;
        action?: string;
      };
      if (body.error) message = body.error;
      if (body.action === "re_enable")
        message += "; re-enable retained repository";
      if (body.action === "disable_old_create_new")
        message += "; disable old source and create a new repository";
    } catch {
      // Keep status fallback when server has no JSON error body.
    }
    throw new Error(message);
  }
  return response.status === 204 ? (undefined as T) : response.json();
}

const emptyForm: RepositoryForm = { name: "", url: "", description: "" };

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).length;
}

function validateForm(form: RepositoryForm): string | null {
  if (!form.name.trim()) return "Name is required";
  if (utf8Bytes(form.name.trim()) > 100)
    return "Name must be at most 100 UTF-8 bytes";
  if (!form.url.trim()) return "Git URL is required";
  if (utf8Bytes(form.url.trim()) > 2048)
    return "Git URL must be at most 2,048 UTF-8 bytes";
  if (utf8Bytes(form.description.trim()) > 10000) {
    return "Description must be at most 10,000 UTF-8 bytes";
  }
  return null;
}

function compareRepositories(left: Repository, right: Repository): number {
  const leftName = left.name.trim().toLowerCase();
  const rightName = right.name.trim().toLowerCase();
  if (leftName < rightName) return -1;
  if (leftName > rightName) return 1;
  return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
}

export function RepositorySettings() {
  const [repositories, setRepositories] = useState<Repository[]>([]);
  const [form, setForm] = useState<RepositoryForm>(emptyForm);
  const [editing, setEditing] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [authorized, setAuthorized] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    async function load() {
      try {
        const user = await request<CurrentUser>("/auth/me");
        const canManage = user.role === "Owner" || user.role === "Admin";
        if (!active) return;
        setAuthorized(canManage);
        if (!canManage) return;
        const value = await request<Repository[]>("/repositories");
        if (active) setRepositories(value);
      } catch (cause) {
        if (active) {
          setAuthorized(false);
          setError(
            cause instanceof Error
              ? cause.message
              : "Unable to load repositories",
          );
        }
      } finally {
        if (active) setLoading(false);
      }
    }

    void load();
    return () => {
      active = false;
    };
  }, []);

  function startEdit(repository: Repository) {
    setEditing(repository.id);
    setForm({
      name: repository.name,
      url: repository.url,
      description: repository.description,
    });
    setError(null);
  }

  function resetForm() {
    setEditing(null);
    setForm(emptyForm);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const validationError = validateForm(form);
    if (validationError) {
      setError(validationError);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const value = editing
        ? await request<Repository>(
            `/repositories/${encodeURIComponent(editing)}`,
            {
              method: "PATCH",
              body: JSON.stringify({
                name: form.name,
                description: form.description,
                url: form.url,
              }),
            },
          )
        : await request<Repository>("/repositories", {
            method: "POST",
            body: JSON.stringify(form),
          });
      setRepositories((current) => {
        const next = editing
          ? current.map((repository) =>
              repository.id === value.id ? value : repository,
            )
          : [...current, value];
        return next.sort(compareRepositories);
      });
      resetForm();
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Unable to save repository",
      );
    } finally {
      setSaving(false);
    }
  }

  async function changeLifecycle(
    repository: Repository,
    action: "disable" | "re-enable",
  ) {
    setBusyId(repository.id);
    setError(null);
    try {
      const value = await request<Repository>(
        `/repositories/${encodeURIComponent(repository.id)}/${action}`,
        { method: "POST" },
      );
      setRepositories((current) =>
        current.map((item) => (item.id === value.id ? value : item)),
      );
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : "Unable to change repository status",
      );
    } finally {
      setBusyId(null);
    }
  }

  if (loading || authorized === null)
    return <p role="status">Loading repositories…</p>;
  if (!authorized) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Repository settings</CardTitle>
          <CardDescription>
            Repository management requires an Admin or Owner role.
          </CardDescription>
        </CardHeader>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Repository settings</CardTitle>
        <CardDescription>
          Manage credential-free repository metadata. Git access stays on daemon
          hosts.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        {error && (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        )}
        <form
          className="grid gap-4 rounded-md border p-4"
          onSubmit={(event) => void submit(event)}
        >
          <h3 className="font-medium">
            {editing ? "Edit repository" : "Add repository"}
          </h3>
          <div className="grid gap-2">
            <label htmlFor="repository-name">Name</label>
            <input
              id="repository-name"
              required
              maxLength={100}
              value={form.name}
              onChange={(event) =>
                setForm({ ...form, name: event.target.value })
              }
              className="rounded-md border bg-background px-3 py-2"
            />
          </div>
          <div className="grid gap-2">
            <label htmlFor="repository-url">Git URL</label>
            <input
              id="repository-url"
              required
              readOnly={Boolean(editing)}
              maxLength={2048}
              value={form.url}
              onChange={(event) =>
                setForm({ ...form, url: event.target.value })
              }
              className="rounded-md border bg-background px-3 py-2 read-only:opacity-60"
            />
            <p className="text-xs text-muted-foreground">
              HTTPS or literal-git SSH/SCP URL; immutable after creation.
            </p>
          </div>
          <div className="grid gap-2">
            <label htmlFor="repository-description">Description</label>
            <textarea
              id="repository-description"
              maxLength={10000}
              value={form.description}
              onChange={(event) =>
                setForm({ ...form, description: event.target.value })
              }
              className="min-h-20 rounded-md border bg-background px-3 py-2"
            />
          </div>
          <div className="flex gap-2">
            <Button disabled={saving} type="submit">
              {saving ? "Saving…" : editing ? "Save changes" : "Add repository"}
            </Button>
            {editing && (
              <Button type="button" variant="outline" onClick={resetForm}>
                Cancel
              </Button>
            )}
          </div>
        </form>

        {repositories.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No repositories configured.
          </p>
        ) : (
          <div className="overflow-x-auto rounded-md border">
            <table className="w-full text-left text-sm">
              <thead className="bg-muted/50">
                <tr>
                  <th className="px-4 py-3 font-medium">Repository</th>
                  <th className="px-4 py-3 font-medium">Status</th>
                  <th className="px-4 py-3 font-medium">URL</th>
                  <th className="px-4 py-3 font-medium">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {repositories.map((repository) => (
                  <tr className="border-t" key={repository.id}>
                    <td className="px-4 py-3">
                      <div className="font-medium">{repository.name}</div>
                      <div className="text-xs text-muted-foreground">
                        {repository.description || "No description"}
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <Badge>
                        {repository.enabled
                          ? "Enabled"
                          : `Disabled ${repository.disabled_at ?? ""}`}
                      </Badge>
                    </td>
                    <td
                      className="max-w-sm truncate px-4 py-3"
                      title={repository.url}
                    >
                      {repository.url}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <div className="flex justify-end gap-2">
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => startEdit(repository)}
                        >
                          Edit
                        </Button>
                        <Button
                          size="sm"
                          variant={
                            repository.enabled ? "destructive" : "secondary"
                          }
                          disabled={busyId === repository.id}
                          onClick={() =>
                            void changeLifecycle(
                              repository,
                              repository.enabled ? "disable" : "re-enable",
                            )
                          }
                        >
                          {repository.enabled ? "Remove" : "Re-enable"}
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <Button asChild variant="outline">
          <Link href="/">Back to North</Link>
        </Button>
      </CardContent>
    </Card>
  );
}
