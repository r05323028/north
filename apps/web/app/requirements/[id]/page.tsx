import { RequirementDetail } from "@/components/requirement-detail";

type RequirementPageProps = {
  params: Promise<{ id: string }>;
};

export default async function RequirementPage({
  params,
}: RequirementPageProps) {
  const { id } = await params;
  return (
    <main className="north-container">
      <RequirementDetail id={id} />
    </main>
  );
}
