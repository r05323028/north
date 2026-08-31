"use client";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  groupRequirements,
  requirementStatuses,
  type Requirement,
} from "@/lib/requirements";
import { useRequirementCollection } from "@/lib/use-requirement-collection";
import type { RequirementCollectionState } from "@/lib/use-requirement-collection";
import { RequirementCard } from "@/components/requirement-card";

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
    return <p role="status">Loading requirements…</p>;
  }

  const groups = groupRequirements(requirements);
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <CardTitle>Board</CardTitle>
          <Button type="button" onClick={onCreateAction}>
            New requirement
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {error && (
          <p className="text-sm text-destructive" role="alert">
            {error} Showing last successful results.
          </p>
        )}
        {refreshing && (
          <p className="text-xs text-muted-foreground" role="status">
            Refreshing…
          </p>
        )}
        <div className="grid gap-4 xl:grid-cols-5">
          {requirementStatuses.map((status) => {
            const items = groups[status];
            return (
              <section
                aria-labelledby={`requirement-column-${status}`}
                className="min-h-56 rounded-lg border bg-muted/20 p-3"
                data-status={status}
                key={status}
              >
                <div className="mb-3 flex items-center justify-between gap-2">
                  <h2
                    className="font-semibold"
                    id={`requirement-column-${status}`}
                  >
                    {status}
                  </h2>
                  <Badge>{items.length}</Badge>
                </div>
                <div className="grid gap-3">
                  {items.length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      No requirements
                    </p>
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
