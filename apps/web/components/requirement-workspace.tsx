"use client";

import { useState } from "react";

import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/tabs";
import { PageHeader } from "@/components/north-shell";
import { RequirementBoard } from "@/components/requirement-board";
import { RequirementCreate } from "@/components/requirement-create";
import { RequirementList } from "@/components/requirement-list";

type View = "board" | "list" | "create";

export function RequirementWorkspace() {
  const [view, setView] = useState<View>("board");

  return (
    <main className="north-container">
      <PageHeader
        actions={
          <>
            <SegmentedControl
              ariaLabel="Requirement views"
              items={[
                { accessibleName: "Board", label: "看板", value: "board" },
                { accessibleName: "List", label: "清單", value: "list" },
              ]}
              value={view === "create" ? "board" : view}
              onValueChangeAction={(value) => {
                if (value === "board" || value === "list") setView(value);
              }}
            />
            <Button
              aria-label="New requirement"
              type="button"
              onClick={() => setView("create")}
            >
              新增需求
            </Button>
          </>
        }
        description="依 canonical lifecycle state 分組 · 搜尋與篩選即時查詢"
        title="需求看板"
      />
      <div className="grid gap-4 pt-4">
        {view === "board" && (
          <RequirementBoard onCreateAction={() => setView("create")} />
        )}
        {view === "list" && (
          <RequirementList onCreateAction={() => setView("create")} />
        )}
        {view === "create" && (
          <RequirementCreate onCancelAction={() => setView("board")} />
        )}
      </div>
    </main>
  );
}
