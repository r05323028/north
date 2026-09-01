import Link from "next/link";

import { Badge } from "@/components/ui/badge";
import { requirementStatusLabels, type Requirement } from "@/lib/requirements";

export function RequirementCard({ requirement }: { requirement: Requirement }) {
  return (
    <Link
      className="block rounded-lg border bg-background p-4 shadow-xs transition-colors hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      data-testid={`requirement-card-${requirement.id}`}
      href={`/requirements/${encodeURIComponent(requirement.id)}`}
    >
      <div className="flex items-start justify-between gap-3">
        <h3 className="font-medium leading-snug">{requirement.title}</h3>
        <Badge>{requirementStatusLabels[requirement.status]}</Badge>
      </div>
      <dl className="mt-4 grid gap-1 text-xs text-muted-foreground">
        <div className="flex justify-between gap-3">
          <dt>Creator</dt>
          <dd className="truncate font-medium text-foreground">
            {requirement.created_by}
          </dd>
        </div>
        <div className="flex justify-between gap-3">
          <dt>Updated</dt>
          <dd>
            <time dateTime={requirement.updated_at}>
              {requirement.updated_at}
            </time>
          </dd>
        </div>
      </dl>
    </Link>
  );
}
