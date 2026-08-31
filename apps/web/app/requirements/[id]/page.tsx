import { RequirementDetail } from "@/components/requirement-detail";

type RequirementPageProps = {
  params: Promise<{ id: string }>;
};

export default async function RequirementPage({
  params,
}: RequirementPageProps) {
  const { id } = await params;
  return (
    <main className="min-h-screen bg-background px-6 py-8 text-foreground">
      <div className="mx-auto max-w-4xl">
        <RequirementDetail id={id} />
      </div>
    </main>
  );
}
