import { RepositorySettings } from "@/components/repository-settings";

export default function RepositorySettingsPage() {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-6xl flex-col gap-6 px-6 py-12">
      <div>
        <h1 className="text-3xl font-semibold tracking-tight">Settings</h1>
        <p className="text-muted-foreground">Repositories</p>
      </div>
      <RepositorySettings />
    </main>
  );
}
