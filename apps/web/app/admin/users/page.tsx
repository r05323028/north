import { PageHeader } from "@/components/north-shell";
import { UserManagement } from "@/components/user-management";

export default function UsersPage() {
  return (
    <main className="north-container">
      <PageHeader
        description="管理 instance roles · server authorization applies every change"
        title="成員"
      />
      <div className="pt-4">
        <UserManagement />
      </div>
    </main>
  );
}
