"use client";

import { useState } from "react";

import { Button } from "@/components/ui/button";
import { RequirementBoard } from "@/components/requirement-board";
import { RequirementCreate } from "@/components/requirement-create";
import { RequirementList } from "@/components/requirement-list";

type View = "board" | "list" | "create";

export function RequirementWorkspace() {
  const [view, setView] = useState<View>("board");

  return (
    <main className="min-h-screen bg-background px-6 py-8 text-foreground">
      <div className="mx-auto grid max-w-7xl gap-8">
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <p className="text-sm font-medium text-muted-foreground">North</p>
            <h1 className="text-3xl font-semibold tracking-tight">
              Requirement board
            </h1>
            <p className="mt-2 text-muted-foreground">
              Track Requirements by canonical lifecycle state.
            </p>
          </div>
          <nav aria-label="Requirement views" className="flex flex-wrap gap-2">
            <Button
              aria-pressed={view === "board"}
              type="button"
              variant={view === "board" ? "default" : "outline"}
              onClick={() => setView("board")}
            >
              Board
            </Button>
            <Button
              aria-pressed={view === "list"}
              type="button"
              variant={view === "list" ? "default" : "outline"}
              onClick={() => setView("list")}
            >
              List
            </Button>
            <Button type="button" onClick={() => setView("create")}>
              New requirement
            </Button>
          </nav>
        </header>

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
