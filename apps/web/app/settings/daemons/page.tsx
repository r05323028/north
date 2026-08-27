import { DaemonStatus } from "@/components/daemon-status";

export default function DaemonStatusPage() {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-6xl flex-col gap-6 px-6 py-12">
      <div>
        <h1 className="text-3xl font-semibold tracking-tight">Settings</h1>
        <p className="text-muted-foreground">Daemon status</p>
      </div>
      <DaemonStatus />
    </main>
  );
}
