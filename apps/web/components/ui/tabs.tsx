"use client";

import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

export type TabItem = {
    value: string;
    label: ReactNode;
    accessibleName?: string;
    disabled?: boolean;
};

type SelectableItemsProps = {
    ariaLabel: string;
    className?: string;
    items: TabItem[];
    onValueChangeAction: (value: string) => void;
    value: string;
};

export function Tabs({
    ariaLabel = "頁籤",
    className,
    items,
    onValueChangeAction,
    value,
}: Omit<SelectableItemsProps, "ariaLabel"> & { ariaLabel?: string }) {
    return (
        <div
            aria-label={ariaLabel}
            className={cn("north-tabs", className)}
            role="tablist"
        >
            {items.map((item) => {
                const selected = item.value === value;
                return (
                    <button
                        aria-label={item.accessibleName}
                        aria-selected={selected}
                        className={cn(
                            "north-tab-trigger",
                            selected && "is-active",
                        )}
                        disabled={item.disabled}
                        key={item.value}
                        role="tab"
                        type="button"
                        onClick={() => onValueChangeAction(item.value)}
                    >
                        {item.label}
                    </button>
                );
            })}
        </div>
    );
}

export function SegmentedControl({
    ariaLabel = "選項",
    className,
    items,
    onValueChangeAction,
    value,
}: Omit<SelectableItemsProps, "ariaLabel"> & { ariaLabel?: string }) {
    return (
        <div
            aria-label={ariaLabel}
            className={cn("north-tabs", className)}
            role="group"
        >
            {items.map((item) => {
                const selected = item.value === value;
                return (
                    <button
                        aria-label={item.accessibleName}
                        aria-pressed={selected}
                        className={cn(
                            "north-tab-trigger",
                            selected && "is-active",
                        )}
                        disabled={item.disabled}
                        key={item.value}
                        type="button"
                        onClick={() => onValueChangeAction(item.value)}
                    >
                        {item.label}
                    </button>
                );
            })}
        </div>
    );
}
