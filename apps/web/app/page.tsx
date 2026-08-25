import { Button } from "@/components/ui/button";

export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-6 px-6">
      <h1 className="text-4xl font-semibold tracking-tight">North</h1>
      <p className="max-w-md text-center text-muted-foreground">
        Turn ambiguous requests into structured, reviewable requirements.
      </p>
      <Button disabled variant="outline">
        Sign in — lands with introduce-email-auth-and-owner-bootstrap
      </Button>
    </main>
  );
}
