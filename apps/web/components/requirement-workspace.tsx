"use client";

import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/tabs";
import { PageHeader } from "@/components/north-shell";
import { RequirementBoard } from "@/components/requirement-board";
import { RequirementCreate } from "@/components/requirement-create";
import {
  getRequirementWorkspacePreferences,
  saveRequirementWorkspacePreferences,
  type RequirementWorkspacePreferences,
} from "@/lib/api/requirement-workspace-preferences";
import {
  RequirementList,
  type RequirementListPreferences,
} from "@/components/requirement-list";

type View = "board" | "list" | "create";

const defaultListPreferences: RequirementListPreferences = {
  status: "",
  creator: "",
  sort: "updated",
};

export function RequirementWorkspace() {
  const [view, setView] = useState<View>("board");
  const [listPreferences, setListPreferences] = useState(defaultListPreferences);
  const [preferencesLoaded, setPreferencesLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const editGeneration = useRef(0);
  const saveInFlight = useRef(false);

  useEffect(() => {
    let active = true;
    getRequirementWorkspacePreferences()
      .then((preferences) => {
        if (!active) return;
        setView(preferences.default_view);
        setListPreferences({
          status: preferences.list_status ?? "",
          creator: preferences.list_created_by ?? "",
          sort: preferences.list_sort,
        });
      })
      .catch(() => {
        if (active) setLoadError("無法載入個人預設；使用預設看板。");
      })
      .finally(() => {
        if (active) setPreferencesLoaded(true);
      });
    return () => {
      active = false;
    };
  }, []);

  const saveAsDefault = async () => {
    if (saveInFlight.current) return;
    saveInFlight.current = true;
    const saveGeneration = editGeneration.current;
    setSaving(true);
    setSaveError(null);
    setSaved(false);
    const currentView: RequirementWorkspacePreferences["default_view"] =
      view === "list" ? "list" : "board";
    try {
      const preferences = await saveRequirementWorkspacePreferences({
        default_view: currentView,
        list_status: listPreferences.status || null,
        list_created_by: listPreferences.creator.trim() || null,
        list_sort: listPreferences.sort,
      });
      if (editGeneration.current === saveGeneration) {
        setListPreferences({
          status: preferences.list_status ?? "",
          creator: preferences.list_created_by ?? "",
          sort: preferences.list_sort,
        });
        setSaved(true);
      }
    } catch {
      setSaveError("無法儲存個人預設。");
    } finally {
      saveInFlight.current = false;
      setSaving(false);
    }
  };

  if (!preferencesLoaded) {
    return (
      <main className="north-container">
        <p role="status">載入個人預設中…</p>
      </main>
    );
  }

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
                if (value === "board" || value === "list") {
                  editGeneration.current += 1;
                  setSaved(false);
                  setSaveError(null);
                  setView(value);
                }
              }}
            />
            {view !== "create" && (
              <Button
                aria-label="Save as default"
                disabled={saving}
                type="button"
                onClick={saveAsDefault}
              >
                {saving ? "儲存中…" : "設為預設"}
              </Button>
            )}
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
      {loadError && (
        <p className="pt-3 text-sm text-destructive" role="alert">
          {loadError}
        </p>
      )}
      {saveError && (
        <p className="pt-3 text-sm text-destructive" role="alert">
          {saveError}
        </p>
      )}
      {saved && (
        <p className="pt-3 text-sm" role="status">
          已儲存個人預設。
        </p>
      )}
      <div className="grid gap-4 pt-4">
        {view === "board" && (
          <RequirementBoard onCreateAction={() => setView("create")} />
        )}
        {view === "list" && (
          <RequirementList
            onCreateAction={() => setView("create")}
            onPreferencesChange={(preferences) => {
              editGeneration.current += 1;
              setSaved(false);
              setSaveError(null);
              setListPreferences(preferences);
            }}
            preferences={listPreferences}
          />
        )}
        {view === "create" && (
          <RequirementCreate onCancelAction={() => setView("board")} />
        )}
      </div>
    </main>
  );
}
