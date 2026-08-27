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
import { Select } from "@/components/ui/select";

const roles = ["Owner", "Admin", "RequirementManager", "Requester"] as const;
type Role = (typeof roles)[number];

type User = {
  id: string;
  email: string;
  role: Role;
};

function isRole(value: unknown): value is Role {
  return typeof value === "string" && roles.includes(value as Role);
}

function isAdminRole(role: Role | null): boolean {
  return role === "Owner" || role === "Admin";
}

function roleLabel(role: Role): string {
  return role === "RequirementManager" ? "Requirement Manager" : role;
}

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
  return response.json() as Promise<T>;
}

export function UserManagement() {
  const [currentUser, setCurrentUser] = useState<User | null>(null);
  const [users, setUsers] = useState<User[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    async function load() {
      try {
        const user = await request<User>("/auth/me");
        if (!isRole(user.role)) {
          throw new Error("Unknown current-user role");
        }
        if (!active) return;
        setCurrentUser(user);
        if (isAdminRole(user.role)) {
          const listedUsers = await request<User[]>("/users");
          if (!active) return;
          setUsers(listedUsers);
        }
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof Error ? cause.message : "Unable to load users",
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

  async function changeRole(userId: string, role: Role) {
    setSaving(userId);
    setError(null);
    try {
      const updated = await request<User>(
        `/users/${encodeURIComponent(userId)}/role`,
        {
          method: "PATCH",
          body: JSON.stringify({ role }),
        },
      );
      setUsers((current) =>
        current.map((user) => (user.id === updated.id ? updated : user)),
      );
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Unable to update role",
      );
    } finally {
      setSaving(null);
    }
  }

  if (loading) {
    return <p role="status">Loading users…</p>;
  }

  if (error && !currentUser) {
    return <p role="alert">{error}</p>;
  }

  if (!currentUser || !isAdminRole(currentUser.role)) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>User management</CardTitle>
          <CardDescription>Admin or Owner access required.</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <div>
            <CardTitle>User management</CardTitle>
            <CardDescription>
              Assign one instance role per user.
            </CardDescription>
          </div>
          <Badge>{roleLabel(currentUser.role)}</Badge>
        </div>
      </CardHeader>
      <CardContent>
        {error && (
          <p className="mb-4 text-sm text-destructive" role="alert">
            {error}
          </p>
        )}
        <div className="overflow-x-auto rounded-md border">
          <table className="w-full text-left text-sm">
            <thead className="bg-muted/50">
              <tr>
                <th className="px-4 py-3 font-medium">Email</th>
                <th className="px-4 py-3 font-medium">Role</th>
                <th className="px-4 py-3 font-medium">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {users.map((user) => {
                const isSelf = user.id === currentUser.id;
                return (
                  <tr className="border-t" key={user.id}>
                    <td className="px-4 py-3">{user.email}</td>
                    <td className="px-4 py-3">
                      <Badge>{roleLabel(user.role)}</Badge>
                    </td>
                    <td className="px-4 py-3 text-right">
                      <label className="sr-only" htmlFor={`role-${user.id}`}>
                        Role for {user.email}
                      </label>
                      <Select
                        id={`role-${user.id}`}
                        value={user.role}
                        disabled={isSelf || saving === user.id}
                        onChange={(event) => {
                          const role = event.target.value;
                          if (isRole(role)) void changeRole(user.id, role);
                        }}
                      >
                        {roles
                          .filter(
                            (role) =>
                              currentUser.role === "Owner" ||
                              role !== "Owner" ||
                              user.role === "Owner",
                          )
                          .map((role) => (
                            <option key={role} value={role}>
                              {roleLabel(role)}
                            </option>
                          ))}
                      </Select>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <p className="mt-4 text-muted-foreground text-xs">
          Role controls are affordances only; server authorization applies every
          change.
        </p>
        <Button className="mt-4" asChild variant="outline">
          <Link href="/">Back to North</Link>
        </Button>
      </CardContent>
    </Card>
  );
}
