"use client";

import type { SyntheticEvent } from "react";
import { useState } from "react";
import { useRouter } from "next/navigation";

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { createRequirement } from "@/lib/requirements";

type CreateForm = {
  title: string;
  description: string;
};

const emptyForm: CreateForm = { title: "", description: "" };

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).length;
}

export function validateCreateForm(form: CreateForm): string | null {
  const title = form.title.trim();
  const description = form.description.trim();
  if (!title) return "Title is required";
  if (utf8Bytes(title) > 500) return "Title must be at most 500 UTF-8 bytes";
  if (!description) return "Description is required";
  if (utf8Bytes(description) > 10000) {
    return "Description must be at most 10,000 UTF-8 bytes";
  }
  return null;
}

export function RequirementCreate({
  onCancelAction,
}: {
  onCancelAction: () => void;
}) {
  const router = useRouter();
  const [form, setForm] = useState<CreateForm>(emptyForm);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    const validationError = validateCreateForm(form);
    if (validationError) {
      setError(validationError);
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const requirement = await createRequirement({
        title: form.title.trim(),
        description: form.description.trim(),
      });
      router.push(`/requirements/${encodeURIComponent(requirement.id)}`);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Unable to create requirement",
      );
      setSaving(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>New requirement</CardTitle>
        <CardDescription>Start with title and description.</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="grid gap-4" onSubmit={(event) => void submit(event)}>
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <div className="grid gap-2">
            <label htmlFor="requirement-title">Title</label>
            <input
              autoFocus
              className="rounded-md border bg-background px-3 py-2"
              id="requirement-title"
              maxLength={500}
              required
              value={form.title}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  title: event.target.value,
                }))
              }
            />
          </div>
          <div className="grid gap-2">
            <label htmlFor="requirement-description">Description</label>
            <textarea
              className="min-h-32 rounded-md border bg-background px-3 py-2"
              id="requirement-description"
              maxLength={10000}
              required
              value={form.description}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  description: event.target.value,
                }))
              }
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <Button disabled={saving} type="submit">
              {saving ? "Creating…" : "Create requirement"}
            </Button>
            <Button
              disabled={saving}
              type="button"
              variant="outline"
              onClick={onCancelAction}
            >
              Cancel
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
