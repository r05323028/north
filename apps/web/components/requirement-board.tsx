"use client";

import { DragDropProvider, type DragEndEvent, type DragOverEvent, useDroppable } from "@dnd-kit/react";
import { isSortable, useSortable } from "@dnd-kit/react/sortable";
import { useCallback, useEffect, useRef, useState } from "react";

import { RequirementCard } from "@/components/requirement-card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { reopenRequirement } from "@/lib/api/review";
import {
  beginRequirementDiscussion,
  groupRequirements,
  reorderRequirement,
  requirementStatusLabels,
  requirementStatuses,
  type Requirement,
  type RequirementStatus,
} from "@/lib/requirements";
import { useRequirementCollection } from "@/lib/use-requirement-collection";
import type { RequirementCollectionState } from "@/lib/use-requirement-collection";

function statusForColumnTarget(id: unknown): RequirementStatus | undefined {
  return requirementStatuses.find((status) => `column:${status}` === String(id));
}

function DroppableColumnBody({ status, items, dropSettling }: { status: RequirementStatus; items: Requirement[]; dropSettling: boolean }) {
  const { ref } = useDroppable({ id: `column:${status}`, disabled: items.length > 0 });
  return (
    <div ref={ref} className="north-column-body">
      {items.length === 0 ? <p className="north-column-empty">無符合需求</p> : items.map((requirement, index) => (
        <SortableRequirementCard key={requirement.id} requirement={requirement} index={index} dropSettling={dropSettling} />
      ))}
    </div>
  );
}

export function RequirementBoard({ onCreateAction }: { onCreateAction: () => void }) {
  const collection = useRequirementCollection({ sort: "board" });
  return <RequirementBoardView {...collection} onCreateAction={onCreateAction} />;
}

type RequirementBoardViewProps = RequirementCollectionState & {
  refresh?: () => void;
  onCreateAction: () => void;
};

function SortableRequirementCard({ requirement, index, dropSettling }: { requirement: Requirement; index: number; dropSettling: boolean }) {
  const { ref, handleRef, isDragging } = useSortable({ id: requirement.id, index, group: requirement.status, disabled: dropSettling });
  return (
    <div ref={ref} data-requirement-id={requirement.id} className={isDragging ? "opacity-60" : undefined}>
      <div className="flex items-center gap-2">
        <RequirementCard requirement={requirement} />
        <Button ref={handleRef} type="button" variant="ghost" size="sm" disabled={dropSettling} aria-label={`Move ${requirement.title}`} aria-roledescription="sortable requirement" className="touch-none cursor-grab">⠿</Button>
      </div>
    </div>
  );
}

export function RequirementBoardView({
  requirements, loading, refreshing, error, refresh = () => {}, onCreateAction,
}: RequirementBoardViewProps) {
  const [moveError, setMoveError] = useState<string | null>(null);
  const [boardRequirements, setBoardRequirements] = useState(requirements);
  const [dropSettling, setDropSettling] = useState(false);
  const isDragging = useRef(false);
  const dragSnapshot = useRef(requirements);
  const latestRequirements = useRef(requirements);
  const pendingBoardRequirements = useRef<Requirement[] | null>(null);
  const canonicalChangedDuringDrag = useRef(false);
  const dropCompletionPending = useRef(false);
  const dropApplied = useRef(false);
  const dropSettleTimer = useRef<number | null>(null);

  useEffect(() => {
    latestRequirements.current = requirements;
    if (isDragging.current) {
      if (requirements !== dragSnapshot.current) canonicalChangedDuringDrag.current = true;
    } else {
      setBoardRequirements(requirements);
    }
  }, [requirements]);

  useEffect(() => () => {
    if (dropSettleTimer.current !== null) window.clearTimeout(dropSettleTimer.current);
  }, []);

  const handleDropComplete = useCallback(() => {
    if (!dropCompletionPending.current) return;
    dropCompletionPending.current = false;
    if (dropSettleTimer.current !== null) {
      window.clearTimeout(dropSettleTimer.current);
      dropSettleTimer.current = null;
    }
    isDragging.current = false;
    const nextRequirements = canonicalChangedDuringDrag.current
      ? latestRequirements.current
      : pendingBoardRequirements.current ?? latestRequirements.current;
    const shouldSyncBoard = canonicalChangedDuringDrag.current || !dropApplied.current;
    canonicalChangedDuringDrag.current = false;
    pendingBoardRequirements.current = null;
    dropApplied.current = false;
    setDropSettling(false);
    if (shouldSyncBoard) setBoardRequirements([...nextRequirements]);
  }, []);

  if (loading && requirements.length === 0) return <p role="status">載入需求中…</p>;
  const groups = groupRequirements(boardRequirements);

  const persistDrag = async (
    source: Requirement,
    initialStatus: RequirementStatus,
    destinationStatus: RequirementStatus,
    initialIndex: number,
    index: number,
    hasTarget: boolean,
  ) => {
    setMoveError(null);
    try {
      if (initialStatus !== destinationStatus) {
        if (source.status === "draft" && destinationStatus === "discussing") {
          await beginRequirementDiscussion(source.id, source.state_version);
        } else if (source.status === "rejected" && destinationStatus === "discussing") {
          await reopenRequirement(source.id, { expected_state_version: source.state_version });
        } else {
          setMoveError(source.status === "accepted"
            ? "Accepted requirements cannot be moved."
            : "This lifecycle move is not available from the board.");
          refresh();
          return;
        }
      } else {
        const column = groups[source.status];
        const remaining = column.filter((item) => item.id !== source.id);
        const insertionIndex = Math.max(0, Math.min(index, remaining.length));
        const before = remaining[insertionIndex - 1];
        const after = remaining[insertionIndex];
        if (!hasTarget || initialIndex === index) return;
        await reorderRequirement(source.id, {
          expected_state_version: source.state_version,
          before_id: before?.id ?? null,
          after_id: after?.id ?? null,
        });
      }
      refresh();
    } catch (cause) {
      setMoveError(cause instanceof Error ? cause.message : "Move failed. Board refreshed from server.");
      refresh();
    }
  };

  const handleDragStart = () => {
    dropCompletionPending.current = true;
    dragSnapshot.current = boardRequirements;
    pendingBoardRequirements.current = null;
    canonicalChangedDuringDrag.current = false;
    dropApplied.current = false;
    isDragging.current = true;
  };

  const handleDragOver = (event: DragOverEvent) => {
    const source = event.operation.source;
    const target = event.operation.target;
    if (!isSortable(source) || !target) return;
    const targetRequirement = dragSnapshot.current.find((item) => item.id === String(target.id));
    const targetStatus = targetRequirement?.status ?? statusForColumnTarget(target.id);
    if (targetStatus && source.sortable.initialGroup !== targetStatus) event.preventDefault();
  };

  const handleDragEnd = (event: DragEndEvent) => {
    window.setTimeout(() => setDropSettling(true), 0);
    dropSettleTimer.current = window.setTimeout(handleDropComplete, 300);
    const sourceId = event.operation.source?.id;
    const targetId = event.operation.target?.id;
    const dragSource = event.operation.source;
    if (event.canceled || sourceId == null || !isSortable(dragSource)) {
      pendingBoardRequirements.current = null;
      if (event.canceled) window.setTimeout(refresh, 0);
      return;
    }
    const source = dragSnapshot.current.find((item) => item.id === String(sourceId));
    if (!source) {
      pendingBoardRequirements.current = null;
      window.setTimeout(refresh, 0);
      return;
    }
    const initialStatus = dragSource.sortable.initialGroup as RequirementStatus | undefined;
    const targetRequirement = dragSnapshot.current.find((item) => item.id === String(targetId));
    const destinationStatus = targetRequirement?.status ?? statusForColumnTarget(targetId) ?? dragSource.sortable.group as RequirementStatus | undefined;
    if (!initialStatus || !destinationStatus) {
      pendingBoardRequirements.current = null;
      window.setTimeout(refresh, 0);
      return;
    }
    const { initialIndex, index } = dragSource.sortable;
    const allowedTransition =
      (source.status === "draft" && destinationStatus === "discussing") ||
      (source.status === "rejected" && destinationStatus === "discussing");
    const supportedDrop = initialStatus === destinationStatus || allowedTransition;

    if (supportedDrop && targetId != null && (initialStatus !== destinationStatus || initialIndex !== index)) {
      const currentGroups = groupRequirements(dragSnapshot.current);
      const card = currentGroups[initialStatus].find((item) => item.id === String(sourceId));
      if (card) {
        const sourceItems = currentGroups[initialStatus].filter((item) => item.id !== String(sourceId));
        const destinationItems = initialStatus === destinationStatus
          ? sourceItems
          : [...currentGroups[destinationStatus]];
        const insertionIndex = Math.max(0, Math.min(index, destinationItems.length));
        destinationItems.splice(insertionIndex, 0, { ...card, status: destinationStatus });
        const nextGroups = { ...currentGroups, [initialStatus]: sourceItems, [destinationStatus]: destinationItems };
        pendingBoardRequirements.current = requirementStatuses.flatMap((status) => nextGroups[status]);
      }
    }

    if (targetId == null) return;
    window.setTimeout(() => {
      if (initialStatus === destinationStatus) {
        const nextRequirements = canonicalChangedDuringDrag.current
          ? latestRequirements.current
          : pendingBoardRequirements.current;
        if (nextRequirements) {
          setBoardRequirements([...nextRequirements]);
          dropApplied.current = true;
        }
      }
      void persistDrag(source, initialStatus, destinationStatus, initialIndex, index, true);
    }, 0);
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <CardTitle aria-label="Board">看板</CardTitle>
          <div className="flex gap-2">
            <Button aria-label="Refresh board" size="sm" type="button" variant="ghost" onClick={refresh}>重新整理</Button>
            <Button aria-label="New requirement" size="sm" type="button" variant="ghost" onClick={onCreateAction}>新增需求</Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-4 pt-0">
        {error && <p className="text-sm text-destructive" role="alert">{error} · 顯示上次成功結果</p>}
        {moveError && <p className="text-sm text-destructive" role="alert">{moveError}</p>}
        {refreshing && <p className="text-xs text-muted-foreground" role="status">重新整理中…</p>}
        <DragDropProvider onDragStart={handleDragStart} onDragOver={handleDragOver} onDragEnd={handleDragEnd}>
          <div className="north-board" data-drop-settling={dropSettling || undefined}>
            {requirementStatuses.map((status: RequirementStatus) => {
              const items = groups[status];
              return (
                <section aria-labelledby={`requirement-column-${status}`} className="north-column" data-status={status} key={status}>
                  <div className="north-column-head"><h2 id={`requirement-column-${status}`}>{requirementStatusLabels[status]}</h2><Badge>{items.length}</Badge></div>
                  <DroppableColumnBody status={status} items={items} dropSettling={dropSettling} />
                </section>
              );
            })}
          </div>
        </DragDropProvider>
      </CardContent>
    </Card>
  );
}
