import { PageHeader } from "@/components/north-shell";
import { RepositorySettings } from "@/components/repository-settings";

export default function RepositorySettingsPage() {
  return (
    <main className="north-container">
      <PageHeader
        description="管理 Repository metadata · Git access stays on daemon hosts"
        title="儲存庫"
      />
      <div className="pt-4">
        <RepositorySettings />
      </div>
    </main>
  );
}
