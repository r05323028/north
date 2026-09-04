"use client";

import { useEffect, useState } from "react";
import Link from "next/link";

import { PageHeader } from "@/components/north-shell";
import { StatusBadge } from "@/components/ui/status";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { getRequirement, type Requirement } from "@/lib/requirements";

export function RequirementDetail({ id }: { id: string }) {
  const [requirement, setRequirement] = useState<Requirement | null>(null);
  const [loadedId, setLoadedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<{ id: string; message: string } | null>(
    null,
  );

  useEffect(() => {
    let active = true;
    void getRequirement(id)
      .then((value) => {
        if (active) {
          setRequirement(value);
          setLoadedId(id);
        }
      })
      .catch((cause: unknown) => {
        if (active) {
          setError({
            id,
            message:
              cause instanceof Error
                ? cause.message
                : "Unable to load requirement",
          });
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [id]);

  if (error?.id === id) return <p role="alert">{error.message}</p>;
  if (loading || loadedId !== id) {
    return <p role="status">載入需求中…</p>;
  }
  if (!requirement) return <p role="alert">找不到需求。</p>;
  return <RequirementDetailView requirement={requirement} />;
}

function CanonicalList({ items, title }: { items: string[]; title: string }) {
  return (
    <section className="grid gap-2">
      <h2 className="font-semibold">{title}</h2>
      {items.length === 0 ? (
        <p className="text-sm text-muted-foreground">尚無紀錄。</p>
      ) : (
        <ul className="list-disc space-y-1 pl-5 text-sm">
          {items.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function RequirementDetailView({
  requirement,
}: {
  requirement: Requirement;
}) {
  return (
    <div className="grid gap-4 pt-4">
      <PageHeader
        actions={<StatusBadge status={requirement.status} />}
        description="Canonical Requirement state · 重新整理即為最新"
        eyebrow="Requirement"
        title={requirement.title}
      />
      <Card>
        <CardHeader>
          <CardTitle>概覽</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-6 pt-0">
          <section className="grid gap-2">
            <h2 className="font-semibold">描述</h2>
            <p className="whitespace-pre-wrap text-sm">
              {requirement.description}
            </p>
          </section>
          <section className="grid gap-2 text-sm">
            <h2 className="font-semibold">Metadata</h2>
            <dl className="grid gap-3 sm:grid-cols-2">
              <div>
                <dt className="text-muted-foreground">建立者</dt>
                <dd>{requirement.created_by}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">建立時間</dt>
                <dd>
                  <time dateTime={requirement.created_at}>
                    {requirement.created_at}
                  </time>
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">更新時間</dt>
                <dd>
                  <time dateTime={requirement.updated_at}>
                    {requirement.updated_at}
                  </time>
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Revision</dt>
                <dd>{requirement.revision}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">State version</dt>
                <dd>{requirement.state_version}</dd>
              </div>
            </dl>
          </section>
          <section className="grid gap-2">
            <h2 className="font-semibold">摘要</h2>
            <p className="whitespace-pre-wrap text-sm">
              {requirement.summary || "尚無紀錄。"}
            </p>
          </section>
          <CanonicalList
            items={requirement.acceptance_criteria}
            title="Acceptance criteria"
          />
          <CanonicalList items={requirement.assumptions} title="Assumptions" />
          <CanonicalList
            items={requirement.open_questions}
            title="Open questions"
          />
        </CardContent>
      </Card>
      <Link className="text-sm font-medium hover:underline" href="/">
        ← 返回需求看板
      </Link>
    </div>
  );
}
