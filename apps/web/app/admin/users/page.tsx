import { UserManagement } from "@/components/user-management";

export default function UsersPage() {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-5xl items-start justify-center px-6 py-16">
      <UserManagement />
    </main>
  );
}
