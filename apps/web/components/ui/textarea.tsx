import * as React from "react";

import { cn } from "@/lib/utils";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "north-control min-h-20 w-full resize-y rounded-md border bg-background px-2.5 py-2 text-[13px] text-foreground outline-none placeholder:text-muted-foreground/70 focus-visible:border-primary disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Textarea };
