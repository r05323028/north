import Link from "next/link";

import { PageHeader } from "@/components/north-shell";
import { Button } from "@/components/ui/button";
import { DaemonStatus } from "@/components/daemon-status";

export default function DaemonStatusPage() {
  return (
    <main className="north-container">
      <PageHeader
        actions={
          <Button asChild size="sm" variant="outline">
            <Link href="/settings/repositories">儲存庫</Link>
          </Button>
        }
        description="Connected execution hosts · server-owned status"
        title="執行狀態"
      />
      <div className="pt-4">
        <DaemonStatus />
      </div>
    </main>
  );
}
