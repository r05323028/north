"use client";

import { RequirementCard } from "@/components/requirement-card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  groupRequirements,
  requirementStatusLabels,
  requirementStatuses,
} from "@/lib/requirements";
import { useRequirementCollection } from "@/lib/use-requirement-collection";
import type { RequirementCollectionState } from "@/lib/use-requirement-collection";

export function RequirementBoard({
  onCreateAction,
}: {
  onCreateAction: () => void;
}) {
  const collection = useRequirementCollection();
  return (
    <RequirementBoardView {...collection} onCreateAction={onCreateAction} />
  );
}

type RequirementBoardViewProps = RequirementCollectionState & {
  onCreateAction: () => void;
};

export function RequirementBoardView({
  requirements,
  loading,
  refreshing,
  error,
  onCreateAction,
}: RequirementBoardViewProps) {
  if (loading && requirements.length === 0) {
    return <p role="status">載入需求中…</p>;
  }

  const groups = groupRequirements(requirements);
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <CardTitle aria-label="Board">看板</CardTitle>
          <Button
            aria-label="New requirement"
            size="sm"
            type="button"
            variant="ghost"
            onClick={onCreateAction}
          >
            新增需求
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4 pt-0">
        {error && (
          <p className="text-sm text-destructive" role="alert">
            {error} · 顯示上次成功結果
          </p>
        )}
        {refreshing && (
          <p className="text-xs text-muted-foreground" role="status">
            重新整理中…
          </p>
        )}
        <div className="north-board">
          {requirementStatuses.map((status) => {
            const items = groups[status];
            return (
              <section
                aria-labelledby={`requirement-column-${status}`}
                className="north-column"
                data-status={status}
                key={status}
              >
                <div className="north-column-head">
                  <h2 id={`requirement-column-${status}`}>
                    {requirementStatusLabels[status]}
                  </h2>
                  <Badge>{items.length}</Badge>
                </div>
                <div className="north-column-body">
                  {items.length === 0 ? (
                    <p className="north-column-empty">無符合需求</p>
                  ) : (
                    items.map((requirement) => (
                      <RequirementCard
                        key={requirement.id}
                        requirement={requirement}
                      />
                    ))
                  )}
                </div>
              </section>
            );
          })}
        </div>
      </CardContent>
    </Card>
  );
}
