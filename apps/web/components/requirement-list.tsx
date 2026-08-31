"use client";

import { useMemo, useState } from "react";
import Link from "next/link";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Select } from "@/components/ui/select";
import {
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
        return <p role="status">Loading requirements…</p>;
    }

    return (
        <Card>
            <CardHeader>
                <div className="flex flex-wrap items-center justify-between gap-4">
                    <CardTitle>Requirement list</CardTitle>
                    <Button type="button" onClick={onCreateAction}>
                        New requirement
                    </Button>
                </div>
            </CardHeader>
            <CardContent className="space-y-4">
                <div className="grid gap-3 rounded-md border p-4 md:grid-cols-4">
                    <div className="grid gap-2 md:col-span-2">
                        <label htmlFor="requirement-search">Search</label>
                        <input
                            className="rounded-md border bg-background px-3 py-2"
                            id="requirement-search"
                            placeholder="Search requirements"
                            type="search"
                            value={search}
                            onChange={(event) =>
                                onSearchChange(event.target.value)
                            }
                        />
                    </div>
                    <div className="grid gap-2">
                        <label htmlFor="requirement-status">Status</label>
                        <Select
                            id="requirement-status"
                            value={status.toLowerCase()}
                            onChange={(event) => {
                                const value = event.target.value;
                                onStatusChange(
                                    requirementStatuses.find(
                                        (candidate) =>
                                            candidate.toLowerCase() === value,
                                    ) ?? "",
                                );
                            }}
                        >
                            <option value="">All statuses</option>
                            {requirementStatuses.map((candidate) => (
                                <option
                                    key={candidate}
                                    value={candidate.toLowerCase()}
                                >
                                    {candidate}
                                </option>
                            ))}
                        </Select>
                    </div>
                    <div className="grid gap-2">
                        <label htmlFor="requirement-sort">Updated</label>
                        <Select
                            id="requirement-sort"
                            value={sort}
                            onChange={(event) =>
                                onSortChange(
                                    event.target.value as RequirementSort,
                                )
                            }
                        >
                            <option value="updated">Newest first</option>
                            <option value="updated_asc">Oldest first</option>
                        </Select>
                    </div>
                    <div className="grid gap-2 md:col-span-2">
                        <label htmlFor="requirement-creator">Creator</label>
                        <input
                            className="rounded-md border bg-background px-3 py-2"
                            id="requirement-creator"
                            placeholder="Creator ID"
                            value={creator}
                            onChange={(event) =>
                                onCreatorChange(event.target.value)
                            }
                        />
                    </div>
                </div>
                {error && (
                    <p className="text-sm text-destructive" role="alert">
                        {error} Showing last successful results.
                    </p>
                )}
                {refreshing && (
                    <p className="text-xs text-muted-foreground" role="status">
                        Refreshing…
                    </p>
                )}
                {requirements.length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                        No requirements match current filters.
                    </p>
                ) : (
                    <div className="overflow-x-auto rounded-md border">
                        <table className="w-full text-left text-sm">
                            <thead className="bg-muted/50">
                                <tr>
                                    <th className="px-4 py-3 font-medium">
                                        Requirement
                                    </th>
                                    <th className="px-4 py-3 font-medium">
                                        Status
                                    </th>
                                    <th className="px-4 py-3 font-medium">
                                        Creator
                                    </th>
                                    <th className="px-4 py-3 font-medium">
                                        Updated
                                    </th>
                                </tr>
                            </thead>
                            <tbody>
                                {requirements.map((requirement) => (
                                    <tr
                                        className="border-t"
                                        key={requirement.id}
                                    >
                                        <td className="px-4 py-3">
                                            <Link
                                                className="font-medium hover:underline"
                                                href={`/requirements/${encodeURIComponent(requirement.id)}`}
                                            >
                                                {requirement.title}
                                            </Link>
                                        </td>
                                        <td className="px-4 py-3">
                                            <Badge>{requirement.status}</Badge>
                                        </td>
                                        <td className="px-4 py-3">
                                            {requirement.created_by}
                                        </td>
                                        <td className="px-4 py-3">
                                            <time
                                                dateTime={
                                                    requirement.updated_at
                                                }
                                            >
                                                {requirement.updated_at}
                                            </time>
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                )}
            </CardContent>
        </Card>
    );
}
