import { RequirementConversationWorkspace } from "@/components/requirement-conversation-workspace";

type RequirementPageProps = {
  params: Promise<{ id: string }>;
};

export default async function RequirementPage({
  params,
}: RequirementPageProps) {
  const { id } = await params;
  return (
    <main className="north-container">
      <RequirementConversationWorkspace id={id} />
    </main>
  );
}
