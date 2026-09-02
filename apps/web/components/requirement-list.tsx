"use client";

import { useMemo, useState } from "react";
import Link from "next/link";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { StatusBadge } from "@/components/ui/status";
import {
        requirementStatusLabels,
        requirementStatuses,
        type RequirementQuery,
        type RequirementSort,
        type RequirementStatus,
} from "@/lib/requirements";
import { useRequirementCollection } from "@/lib/use-requirement-collection";
import type { RequirementCollectionState } from "@/lib/use-requirement-collection";

export function RequirementList({
        onCreateAction,
}: {
        onCreateAction: () => void;
}) {
        const [search, setSearch] = useState("");
        const [status, setStatus] = useState<RequirementStatus | "">("");
        const [creator, setCreator] = useState("");
        const [sort, setSort] = useState<RequirementSort>("updated");
        const query = useMemo<RequirementQuery>(
                () => ({
                        search: search || undefined,
                        status: status || undefined,
                        created_by: creator || undefined,
                        sort,
                }),
                [creator, search, sort, status],
        );
        const collection = useRequirementCollection(query);

        return (
                <RequirementListView
                        {...collection}
                        creator={creator}
                        onCreateAction={onCreateAction}
                        onCreatorChange={setCreator}
                        onSearchChange={setSearch}
                        onSortChange={setSort}
                        onStatusChange={setStatus}
                        search={search}
                        sort={sort}
                        status={status}
                />
        );
}

type RequirementListViewProps = RequirementCollectionState & {
        creator: string;
        onCreateAction: () => void;
        onCreatorChange: (value: string) => void;
        onSearchChange: (value: string) => void;
        onSortChange: (value: RequirementSort) => void;
        onStatusChange: (value: RequirementStatus | "") => void;
        search: string;
        sort: RequirementSort;
        status: RequirementStatus | "";
};

export function RequirementListView({
        requirements,
        loading,
        refreshing,
        error,
        creator,
        onCreateAction,
        onCreatorChange,
        onSearchChange,
        onSortChange,
        onStatusChange,
        search,
        sort,
        status,
}: RequirementListViewProps) {
        if (loading && requirements.length === 0) {
                return <p role="status">載入需求中…</p>;
        }

        return (
                <Card>
                        <CardHeader>
                                <div className="flex flex-wrap items-center justify-between gap-4">
                                        <CardTitle aria-label="Requirement list">
                                                需求清單
                                        </CardTitle>
                                        <Button
                                                aria-label="New requirement"
                                                type="button"
                                                onClick={onCreateAction}
                                        >
                                                新增需求
                                        </Button>
                                </div>
                        </CardHeader>
                        <CardContent className="space-y-4 pt-0">
                                <div
                                        aria-label="搜尋與篩選"
                                        className="north-toolbar"
                                        role="search"
                                >
                                        <label className="north-search">
                                                <span className="sr-only">
                                                        Search
                                                </span>
                                                <svg
                                                        aria-hidden="true"
                                                        fill="none"
                                                        height="16"
                                                        stroke="currentColor"
                                                        strokeLinecap="round"
                                                        strokeLinejoin="round"
                                                        strokeWidth="1.7"
                                                        viewBox="0 0 24 24"
                                                        width="16"
                                                >
                                                        <circle
                                                                cx="11"
                                                                cy="11"
                                                                r="6.5"
                                                        />
                                                        <path d="m16 16 4 4" />
                                                </svg>
                                                <Input
                                                        aria-label="Search"
                                                        placeholder="搜尋需求…"
                                                        type="search"
                                                        value={search}
                                                        onChange={(event) =>
                                                                onSearchChange(
                                                                        event
                                                                                .target
                                                                                .value,
                                                                )
                                                        }
                                                />
                                        </label>
                                        <div className="north-filter-row">
                                                <Select
                                                        aria-label="Status"
                                                        id="requirement-status"
                                                        value={status}
                                                        onChange={(event) => {
                                                                const value =
                                                                        event
                                                                                .target
                                                                                .value;
                                                                onStatusChange(
                                                                        requirementStatuses.find(
                                                                                (
                                                                                        candidate,
                                                                                ) =>
                                                                                        candidate ===
                                                                                        value,
                                                                        ) ?? "",
                                                                );
                                                        }}
                                                >
                                                        <option value="">
                                                                全部狀態
                                                        </option>
                                                        {requirementStatuses.map(
                                                                (candidate) => (
                                                                        <option
                                                                                key={
                                                                                        candidate
                                                                                }
                                                                                value={
                                                                                        candidate
                                                                                }
                                                                        >
                                                                                {
                                                                                        requirementStatusLabels[
                                                                                                candidate
                                                                                        ]
                                                                                }
                                                                        </option>
                                                                ),
                                                        )}
                                                </Select>
                                                <Select
                                                        aria-label="Updated"
                                                        id="requirement-sort"
                                                        value={sort}
                                                        onChange={(event) =>
                                                                onSortChange(
                                                                        event
                                                                                .target
                                                                                .value as RequirementSort,
                                                                )
                                                        }
                                                >
                                                        <option value="updated">
                                                                最近更新
                                                        </option>
                                                        <option value="updated_asc">
                                                                最早更新
                                                        </option>
                                                </Select>
                                                <Input
                                                        aria-label="Creator"
                                                        className="w-32 sm:w-40"
                                                        id="requirement-creator"
                                                        placeholder="建立者"
                                                        value={creator}
                                                        onChange={(event) =>
                                                                onCreatorChange(
                                                                        event
                                                                                .target
                                                                                .value,
                                                                )
                                                        }
                                                />
                                        </div>
                                        <span className="north-meta">
                                                由伺服器排序 · 自動更新
                                        </span>
                                </div>
                                {error && (
                                        <p
                                                className="text-sm text-destructive"
                                                role="alert"
                                        >
                                                {error} · 顯示上次成功結果
                                        </p>
                                )}
                                {refreshing && (
                                        <p
                                                className="text-xs text-muted-foreground"
                                                role="status"
                                        >
                                                重新整理中…
                                        </p>
                                )}
                                {requirements.length === 0 ? (
                                        <p className="text-sm text-muted-foreground">
                                                無符合需求 · 調整搜尋或篩選
                                        </p>
                                ) : (
                                        <div className="overflow-x-auto rounded-md border">
                                                <table
                                                        aria-label="需求清單"
                                                        className="north-table"
                                                >
                                                        <thead>
                                                                <tr>
                                                                        <th scope="col">
                                                                                需求
                                                                        </th>
                                                                        <th scope="col">
                                                                                狀態
                                                                        </th>
                                                                        <th scope="col">
                                                                                建立者
                                                                        </th>
                                                                        <th scope="col">
                                                                                更新
                                                                        </th>
                                                                </tr>
                                                        </thead>
                                                        <tbody>
                                                                {requirements.map(
                                                                        (
                                                                                requirement,
                                                                        ) => (
                                                                                <tr
                                                                                        key={
                                                                                                requirement.id
                                                                                        }
                                                                                >
                                                                                        <td>
                                                                                                <Link
                                                                                                        className="font-medium hover:underline"
                                                                                                        href={`/requirements/${encodeURIComponent(requirement.id)}`}
                                                                                                >
                                                                                                        {
                                                                                                                requirement.title
                                                                                                        }
                                                                                                </Link>
                                                                                        </td>
                                                                                        <td>
                                                                                                <StatusBadge
                                                                                                        status={
                                                                                                                requirement.status
                                                                                                        }
                                                                                                />
                                                                                        </td>
                                                                                        <td>
                                                                                                {
                                                                                                        requirement.created_by
                                                                                                }
                                                                                        </td>
                                                                                        <td>
                                                                                                <time
                                                                                                        dateTime={
                                                                                                                requirement.updated_at
                                                                                                        }
                                                                                                >
                                                                                                        {
                                                                                                                requirement.updated_at
                                                                                                        }
                                                                                                </time>
                                                                                        </td>
                                                                                </tr>
                                                                        ),
                                                                )}
                                                        </tbody>
                                                </table>
                                        </div>
                                )}
                        </CardContent>
                </Card>
        );
}
