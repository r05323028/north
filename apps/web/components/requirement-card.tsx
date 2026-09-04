import Link from "next/link";

import { StatusBadge } from "@/components/ui/status";
import { type Requirement } from "@/lib/requirements";

export function RequirementCard({ requirement }: { requirement: Requirement }) {
  return (
    <Link
      className="north-req-card block rounded-md border bg-background p-3 shadow-none transition-colors hover:bg-accent/50 focus-visible:outline-none"
      data-testid={`requirement-card-${requirement.id}`}
      href={`/requirements/${encodeURIComponent(requirement.id)}`}
    >
      <div className="flex items-start justify-between gap-3">
        <h3 className="min-w-0 font-medium leading-snug text-pretty">
          {requirement.title}
        </h3>
        <StatusBadge status={requirement.status} />
      </div>
      <dl className="mt-3 grid gap-1 text-xs text-muted-foreground">
        <div className="flex justify-between gap-3">
          <dt>建立者</dt>
          <dd className="truncate font-medium text-foreground">
            {requirement.created_by}
          </dd>
        </div>
        <div className="flex justify-between gap-3">
          <dt>更新</dt>
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
