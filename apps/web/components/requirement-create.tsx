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
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
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
        <CardTitle>新增需求</CardTitle>
        <CardDescription>僅需標題與描述。</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="grid gap-4" onSubmit={(event) => void submit(event)}>
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <div className="grid gap-2">
            <label htmlFor="requirement-title">標題</label>
            <Input
              aria-label="Title"
              autoFocus
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
            <label htmlFor="requirement-description">描述</label>
            <Textarea
              aria-label="Description"
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
            <Button
              aria-label="Create requirement"
              disabled={saving}
              type="submit"
            >
              {saving ? "建立中…" : "建立需求"}
            </Button>
            <Button
              aria-label="Cancel"
              disabled={saving}
              type="button"
              variant="outline"
              onClick={onCancelAction}
            >
              取消
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
