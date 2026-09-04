import { cn } from "@/lib/utils";
import {
  requirementStatusLabels,
  type RequirementStatus,
} from "@/lib/requirements";

const statusClasses: Record<RequirementStatus, string> = {
  draft: "north-status-draft",
  discussing: "north-status-discussing",
  ready: "north-status-ready",
  accepted: "north-status-accepted",
  rejected: "north-status-rejected",
};

export function StatusBadge({
  label,
  status,
}: {
  label?: string;
  status: RequirementStatus;
}) {
  return (
    <span className={cn("north-status", statusClasses[status])}>
      <span aria-hidden="true" className="north-status-dot" />
      {label ?? requirementStatusLabels[status]}
    </span>
  );
}
