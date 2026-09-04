import * as React from "react";

import { cn } from "@/lib/utils";

function Input({ className, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      data-slot="input"
      className={cn(
        "north-control h-8 w-full rounded-md border bg-background px-2.5 text-[13px] text-foreground outline-none placeholder:text-muted-foreground/70 focus-visible:border-primary disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
